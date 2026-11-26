//! Asynchronous process supervision: streaming output, wall-clock and idle
//! timeouts, cancellation, and guaranteed termination of the whole process
//! tree (Windows Job Objects / Unix process groups).

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio::time::{sleep_until, Instant};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamKind {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone)]
pub struct ProcSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Complete environment for the child (the parent's env is not inherited).
    pub env: Vec<(String, String)>,
    pub stdin: Option<String>,
    pub timeout: Duration,
    pub idle_timeout: Duration,
    pub log_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
pub struct ProcResult {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stalled: bool,
    pub cancelled: bool,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub duration_ms: u64,
    /// Processes still alive in the tree after the main process exited
    /// (before the engine terminated them).
    pub lingering_processes: u32,
}

const TAIL_LIMIT: usize = 64 * 1024;
/// After the main process exits, how long to keep draining output from
/// pipes that grandchildren may still hold open.
const DRAIN_GRACE: Duration = Duration::from_millis(1500);

fn push_tail(buf: &mut String, line: &str) {
    buf.push_str(line);
    buf.push('\n');
    if buf.len() > TAIL_LIMIT {
        let mut cut = buf.len() - TAIL_LIMIT;
        while !buf.is_char_boundary(cut) {
            cut += 1;
        }
        buf.drain(..cut);
    }
}

async fn pump<R: AsyncRead + Unpin>(reader: R, kind: StreamKind, tx: mpsc::UnboundedSender<(StreamKind, String)>) {
    let mut reader = BufReader::new(reader);
    let mut buf = Vec::with_capacity(4096);
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let line = String::from_utf8_lossy(&buf);
                let line = line.trim_end_matches(['\r', '\n']).to_string();
                if tx.send((kind, line)).is_err() {
                    break;
                }
            }
        }
    }
}

/// Run a process to completion under supervision. `on_line` sees every
/// output line as it arrives.
pub async fn run_process<F>(spec: ProcSpec, mut on_line: F, cancel: CancellationToken) -> std::io::Result<ProcResult>
where
    F: FnMut(StreamKind, &str) + Send,
{
    let started = Instant::now();
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args)
        .current_dir(&spec.cwd)
        .env_clear()
        .envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .stdin(if spec.stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    cmd.process_group(0);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    }

    let mut child = cmd.spawn()?;
    let tree = tree::ProcessTree::attach(&child);

    if let Some(input) = spec.stdin.clone() {
        if let Some(mut stdin) = child.stdin.take() {
            tokio::spawn(async move {
                let _ = stdin.write_all(input.as_bytes()).await;
                let _ = stdin.shutdown().await;
            });
        }
    }

    let (tx, mut rx) = mpsc::unbounded_channel();
    if let Some(out) = child.stdout.take() {
        tokio::spawn(pump(out, StreamKind::Stdout, tx.clone()));
    }
    if let Some(err) = child.stderr.take() {
        tokio::spawn(pump(err, StreamKind::Stderr, tx.clone()));
    }
    drop(tx);

    let mut log = match &spec.log_path {
        Some(p) => {
            if let Some(parent) = p.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            Some(tokio::fs::OpenOptions::new().create(true).append(true).open(p).await?)
        }
        None => None,
    };

    let deadline = started + spec.timeout;
    let far_future = started + Duration::from_secs(60 * 60 * 24 * 365);
    let mut last_activity = Instant::now();
    let mut drain_deadline = far_future;
    let mut readers_open = true;
    let mut exit_status: Option<std::process::ExitStatus> = None;
    let mut result = ProcResult::default();

    loop {
        if exit_status.is_some() && !readers_open {
            break;
        }
        let idle_deadline = if exit_status.is_some() { far_future } else { last_activity + spec.idle_timeout };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => { result.cancelled = true; break; }
            msg = rx.recv(), if readers_open => match msg {
                Some((kind, line)) => {
                    last_activity = Instant::now();
                    if let Some(f) = log.as_mut() {
                        let prefix = if kind == StreamKind::Stderr { "[stderr] " } else { "" };
                        let _ = f.write_all(format!("{prefix}{line}\n").as_bytes()).await;
                    }
                    match kind {
                        StreamKind::Stdout => push_tail(&mut result.stdout_tail, &line),
                        StreamKind::Stderr => push_tail(&mut result.stderr_tail, &line),
                    }
                    on_line(kind, &line);
                }
                None => readers_open = false,
            },
            status = child.wait(), if exit_status.is_none() => {
                exit_status = Some(status?);
                drain_deadline = Instant::now() + DRAIN_GRACE;
            }
            _ = sleep_until(drain_deadline), if exit_status.is_some() => break,
            _ = sleep_until(deadline), if exit_status.is_none() => { result.timed_out = true; break; }
            _ = sleep_until(idle_deadline), if exit_status.is_none() => { result.stalled = true; break; }
        }
    }

    if exit_status.is_some() {
        result.lingering_processes = tree.active_processes();
    }
    // Always tear down the whole tree: nothing spawned by the agent may
    // outlive its run and keep writing into the worktree.
    tree.kill();
    if exit_status.is_none() {
        let _ = child.start_kill();
        if let Ok(Ok(st)) = tokio::time::timeout(Duration::from_secs(10), child.wait()).await {
            exit_status = Some(st);
        }
    }
    if let Some(f) = log.as_mut() {
        let _ = f.flush().await;
    }
    result.exit_code = exit_status.and_then(|s| s.code());
    if result.timed_out || result.stalled || result.cancelled {
        // A killed process's exit code is meaningless.
        result.exit_code = None;
    }
    result.duration_ms = started.elapsed().as_millis() as u64;
    Ok(result)
}

#[cfg(windows)]
mod tree {
    use std::mem::{size_of, zeroed};
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
        JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// A Job Object containing the agent and every process it spawns.
    /// Closing the handle (on drop) kills anything still running.
    pub struct ProcessTree {
        job: HANDLE,
    }

    // The job handle is a kernel object usable from any thread.
    unsafe impl Send for ProcessTree {}
    unsafe impl Sync for ProcessTree {}

    impl ProcessTree {
        pub fn attach(child: &tokio::process::Child) -> Self {
            unsafe {
                let job = CreateJobObjectW(null(), null());
                if job.is_null() {
                    return ProcessTree { job };
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if let Some(h) = child.raw_handle() {
                    AssignProcessToJobObject(job, h as HANDLE);
                }
                ProcessTree { job }
            }
        }

        pub fn active_processes(&self) -> u32 {
            if self.job.is_null() {
                return 0;
            }
            unsafe {
                let mut acct: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = zeroed();
                let ok = QueryInformationJobObject(
                    self.job,
                    JobObjectBasicAccountingInformation,
                    &mut acct as *mut _ as *mut _,
                    size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                    null_mut(),
                );
                if ok == 0 {
                    0
                } else {
                    acct.ActiveProcesses
                }
            }
        }

        pub fn kill(&self) {
            if !self.job.is_null() {
                unsafe {
                    TerminateJobObject(self.job, 1);
                }
            }
        }
    }

    impl Drop for ProcessTree {
        fn drop(&mut self) {
            if !self.job.is_null() {
                unsafe {
                    CloseHandle(self.job);
                }
            }
        }
    }
}

#[cfg(unix)]
mod tree {
    /// The agent runs as the leader of its own process group; killing the
    /// group reaches every descendant that did not deliberately escape it.
    pub struct ProcessTree {
        pgid: Option<i32>,
    }

    impl ProcessTree {
        pub fn attach(child: &tokio::process::Child) -> Self {
            ProcessTree { pgid: child.id().map(|p| p as i32) }
        }

        pub fn active_processes(&self) -> u32 {
            match self.pgid {
                Some(pg) if unsafe { libc::kill(-pg, 0) } == 0 => 1,
                _ => 0,
            }
        }

        pub fn kill(&self) {
            if let Some(pg) = self.pgid {
                unsafe {
                    libc::kill(-pg, libc::SIGKILL);
                }
            }
        }
    }

    impl Drop for ProcessTree {
        fn drop(&mut self) {
            self.kill();
        }
    }
}

/// Run a shell command (verification, hooks) with the same supervision.
pub async fn run_shell(
    command: &str,
    cwd: &std::path::Path,
    timeout: Duration,
    log_path: Option<PathBuf>,
    cancel: CancellationToken,
) -> std::io::Result<ProcResult> {
    let (program, args): (PathBuf, Vec<String>) = if cfg!(windows) {
        ("cmd".into(), vec!["/D".into(), "/S".into(), "/C".into(), command.to_string()])
    } else {
        ("sh".into(), vec!["-c".into(), command.to_string()])
    };
    let spec = ProcSpec {
        program,
        args,
        cwd: cwd.to_path_buf(),
        env: std::env::vars().collect(),
        stdin: None,
        timeout,
        idle_timeout: timeout,
        log_path,
    };
    run_process(spec, |_, _| {}, cancel).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell(cmd: &str) -> (PathBuf, Vec<String>) {
        if cfg!(windows) {
            ("cmd".into(), vec!["/D".into(), "/S".into(), "/C".into(), cmd.into()])
        } else {
            ("sh".into(), vec!["-c".into(), cmd.into()])
        }
    }

    fn spec(cmd: &str, timeout: u64, idle: u64) -> ProcSpec {
        let (program, args) = shell(cmd);
        ProcSpec {
            program,
            args,
            cwd: std::env::temp_dir(),
            env: std::env::vars().collect(),
            stdin: None,
            timeout: Duration::from_secs(timeout),
            idle_timeout: Duration::from_secs(idle),
            log_path: None,
        }
    }

    #[tokio::test]
    async fn captures_output_and_exit_code() {
        let mut lines = Vec::new();
        let r = run_process(spec("echo hello&& echo oops 1>&2&& exit 3", 30, 30), |k, l| lines.push((k, l.to_string())), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(r.exit_code, Some(3));
        assert!(r.stdout_tail.contains("hello"));
        assert!(r.stderr_tail.contains("oops"));
        assert!(lines.iter().any(|(k, l)| *k == StreamKind::Stdout && l.trim() == "hello"));
    }

    #[tokio::test]
    async fn idle_timeout_kills_silent_process() {
        let cmd = if cfg!(windows) {
            "for /L %i in (1,0,2) do @ver >nul"
        } else {
            "sleep 30"
        };
        let r = run_process(spec(cmd, 60, 1), |_, _| {}, CancellationToken::new()).await.unwrap();
        assert!(r.stalled, "expected stalled=true, got {r:?}");
        assert!(r.duration_ms < 15_000, "took {}ms", r.duration_ms);
    }

    #[tokio::test]
    async fn cancellation_stops_process() {
        let cmd = if cfg!(windows) {
            "for /L %i in (1,0,2) do @ver >nul"
        } else {
            "sleep 30"
        };
        let token = CancellationToken::new();
        let t2 = token.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            t2.cancel();
        });
        let r = run_process(spec(cmd, 60, 60), |_, _| {}, token).await.unwrap();
        assert!(r.cancelled, "expected cancelled=true, got {r:?}");
        assert!(r.duration_ms < 15_000);
    }
}
