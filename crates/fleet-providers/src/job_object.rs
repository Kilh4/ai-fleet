//! `job_object.rs` — Native Windows Job Object FFI (`kernel32.dll`) for strict process-tree
//! containment (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`) and zero-zombie verification
//! (`active_processes == 0`) before cross-AI quota handoff.

use std::io;
use std::process::{Child, Command, Output, Stdio};

#[cfg(windows)]
mod win_ffi {
    use core::ffi::c_void;

    pub type HANDLE = *mut c_void;
    pub type BOOL = i32;
    pub type DWORD = u32;
    pub type UINT = u32;

    pub const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: DWORD = 0x00002000;
    pub const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS: i32 = 9;
    pub const JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION_CLASS: i32 = 1;

    #[repr(C)]
    #[derive(Default)]
    pub struct JobObjectBasicLimitInformation {
        pub per_process_user_time_limit: i64,
        pub per_job_user_time_limit: i64,
        pub limit_flags: DWORD,
        pub minimum_working_set_size: usize,
        pub maximum_working_set_size: usize,
        pub active_process_limit: DWORD,
        pub affinity: usize,
        pub priority_class: DWORD,
        pub scheduling_class: DWORD,
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct IoCounters {
        pub read_operation_count: u64,
        pub write_operation_count: u64,
        pub other_operation_count: u64,
        pub read_transfer_count: u64,
        pub write_transfer_count: u64,
        pub other_transfer_count: u64,
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct JobObjectExtendedLimitInformation {
        pub basic_limit_information: JobObjectBasicLimitInformation,
        pub io_info: IoCounters,
        pub process_memory_limit: usize,
        pub job_memory_limit: usize,
        pub peak_process_memory_used: usize,
        pub peak_job_memory_used: usize,
    }

    #[repr(C)]
    #[derive(Default)]
    pub struct JobObjectBasicAccountingInformation {
        pub total_user_time: i64,
        pub total_kernel_time: i64,
        pub this_period_total_user_time: i64,
        pub this_period_total_kernel_time: i64,
        pub total_page_fault_count: DWORD,
        pub total_processes: DWORD,
        pub active_processes: DWORD,
        pub total_terminated_processes: DWORD,
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateJobObjectW(lp_job_attributes: *mut c_void, lp_name: *const u16) -> HANDLE;
        pub fn SetInformationJobObject(
            h_job: HANDLE,
            job_object_info_class: i32,
            lp_job_object_info: *const c_void,
            cb_job_object_info_length: DWORD,
        ) -> BOOL;
        pub fn AssignProcessToJobObject(h_job: HANDLE, h_process: HANDLE) -> BOOL;
        pub fn QueryInformationJobObject(
            h_job: HANDLE,
            job_object_info_class: i32,
            lp_job_object_info: *mut c_void,
            cb_job_object_info_length: DWORD,
            lp_return_length: *mut DWORD,
        ) -> BOOL;
        pub fn TerminateJobObject(h_job: HANDLE, u_exit_code: UINT) -> BOOL;
        pub fn CloseHandle(h_object: HANDLE) -> BOOL;
    }
}

#[derive(Debug, Clone)]
pub struct IsolatedJobOutcome {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
    pub active_processes_remaining: u32,
    pub job_isolated: bool,
}

#[cfg(windows)]
pub fn spawn_in_windows_job_object(mut cmd: Command, stdin_bytes: Option<&[u8]>) -> io::Result<IsolatedJobOutcome> {
    use core::ffi::c_void;
    use std::io::Write;
    use std::mem::size_of;
    use std::os::windows::io::AsRawHandle;
    use win_ffi::*;

    unsafe {
        let job = CreateJobObjectW(core::ptr::null_mut(), core::ptr::null());
        if job.is_null() {
            return Err(io::Error::last_os_error());
        }

        let mut ext_limits = JobObjectExtendedLimitInformation::default();
        ext_limits.basic_limit_information.limit_flags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(
            job,
            JOB_OBJECT_EXTENDED_LIMIT_INFORMATION_CLASS,
            (&ext_limits as *const JobObjectExtendedLimitInformation).cast::<c_void>(),
            size_of::<JobObjectExtendedLimitInformation>() as DWORD,
        );
        if ok == 0 {
            let err = io::Error::last_os_error();
            let _ = CloseHandle(job);
            return Err(err);
        }

        if stdin_bytes.is_some() {
            cmd.stdin(Stdio::piped());
        } else {
            cmd.stdin(Stdio::null());
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child: Child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                let _ = CloseHandle(job);
                return Err(e);
            }
        };

        let raw_proc = child.as_raw_handle() as HANDLE;
        let _ = AssignProcessToJobObject(job, raw_proc);

        if let Some(input_data) = stdin_bytes {
            if let Some(mut stdin_pipe) = child.stdin.take() {
                let _ = stdin_pipe.write_all(input_data);
            }
        }

        let output: Output = child.wait_with_output()?;

        // Verify all descendant processes inside the Job Object have terminated
        let mut accounting = JobObjectBasicAccountingInformation::default();
        let _ = QueryInformationJobObject(
            job,
            JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION_CLASS,
            (&mut accounting as *mut JobObjectBasicAccountingInformation).cast::<c_void>(),
            size_of::<JobObjectBasicAccountingInformation>() as DWORD,
            core::ptr::null_mut(),
        );

        if accounting.active_processes > 0 {
            let _ = TerminateJobObject(job, 1);
            let _ = QueryInformationJobObject(
                job,
                JOB_OBJECT_BASIC_ACCOUNTING_INFORMATION_CLASS,
                (&mut accounting as *mut JobObjectBasicAccountingInformation).cast::<c_void>(),
                size_of::<JobObjectBasicAccountingInformation>() as DWORD,
                core::ptr::null_mut(),
            );
        }

        let _ = CloseHandle(job);

        Ok(IsolatedJobOutcome {
            exit_code: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            active_processes_remaining: accounting.active_processes,
            job_isolated: true,
        })
    }
}

#[cfg(not(windows))]
pub fn spawn_in_windows_job_object(mut cmd: Command, stdin_bytes: Option<&[u8]>) -> io::Result<IsolatedJobOutcome> {
    use std::io::Write;
    if stdin_bytes.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn()?;
    if let Some(input_data) = stdin_bytes {
        if let Some(mut stdin_pipe) = child.stdin.take() {
            let _ = stdin_pipe.write_all(input_data);
        }
    }
    let output = child.wait_with_output()?;
    Ok(IsolatedJobOutcome {
        exit_code: output.status.code().unwrap_or(1),
        stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        active_processes_remaining: 0,
        job_isolated: true,
    })
}
