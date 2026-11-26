//! Deterministic stand-in for an AI CLI, driven by environment variables.
//! Used by the end-to-end tests to exercise success, quota exhaustion,
//! crashes, hangs (with child processes), scope violations and reviews.
//!
//! FAKE_AI_NAME          name written into files and output
//! FAKE_AI_PLAN          comma-separated steps, one per invocation (last repeats)
//! FAKE_AI_COUNTER_FILE  invocation counter (required for multi-step plans)
//! FAKE_AI_PROMPT_LOG    append every received prompt here
//! FAKE_AI_CHILD_PID_FILE  where `hang-tree` records its child's pid
//!
//! Steps: ok[:file] | partial-quota[:file] | quota | auth | crash | hang | hang-tree |
//!        out-of-scope | touch:file | approve | reject | noverdict

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{exit, Command, Stdio};
use std::time::Duration;

fn var(k: &str) -> Option<String> {
    std::env::var(k).ok().filter(|v| !v.is_empty())
}

fn append_line(path: &str, line: &str) {
    let p = Path::new(path);
    if let Some(parent) = p.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut f = OpenOptions::new().create(true).append(true).open(p).expect("open output");
    writeln!(f, "{line}").expect("write output");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--child-hang") {
        if let Some(p) = var("FAKE_AI_CHILD_PID_FILE") {
            let _ = fs::write(p, std::process::id().to_string());
        }
        std::thread::sleep(Duration::from_secs(3600));
        return;
    }

    let name = var("FAKE_AI_NAME").unwrap_or_else(|| "fake".into());
    let mut prompt = String::new();
    let _ = std::io::stdin().read_to_string(&mut prompt);
    if let Some(log) = var("FAKE_AI_PROMPT_LOG") {
        append_line(&log, &format!("=== {name} args={:?}\n{prompt}", &args[1..]));
    }

    let plan: Vec<String> = var("FAKE_AI_PLAN").unwrap_or_else(|| "ok".into()).split(',').map(|s| s.trim().to_string()).collect();
    let n = match var("FAKE_AI_COUNTER_FILE") {
        Some(c) => {
            let n: usize = fs::read_to_string(&c).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0);
            fs::write(&c, (n + 1).to_string()).expect("write counter");
            n
        }
        None => 0,
    };
    let step = plan[n.min(plan.len() - 1)].clone();
    let (mode, arg) = match step.split_once(':') {
        Some((m, a)) => (m.to_string(), Some(a.to_string())),
        None => (step.clone(), None),
    };
    let file = arg.clone().unwrap_or_else(|| "output.txt".into());

    match mode.as_str() {
        "ok" => {
            append_line(&file, &format!("by {name}"));
            println!("Implemented the change in {file} (by {name}).");
        }
        "partial-quota" => {
            append_line(&file, &format!("partial by {name}"));
            println!("Starting work...");
            eprintln!("Claude AI usage limit reached. You've hit your limit · try again in 2h 5m");
            exit(1);
        }
        "quota" => {
            eprintln!("Error: 429 Too Many Requests: rate limit exceeded. Please try again in 30 seconds");
            exit(1);
        }
        "auth" => {
            eprintln!("Not logged in · Please run /login");
            exit(1);
        }
        "crash" => {
            eprintln!("thread 'main' panicked at src/agent.rs:1: boom");
            exit(3);
        }
        "hang" => {
            println!("thinking...");
            std::thread::sleep(Duration::from_secs(3600));
        }
        "hang-tree" => {
            let me = std::env::current_exe().expect("exe");
            let _child = Command::new(me).arg("--child-hang").stdin(Stdio::null()).spawn().expect("spawn child");
            println!("spawned helper process; thinking...");
            std::thread::sleep(Duration::from_secs(3600));
        }
        "out-of-scope" => {
            append_line("outside.txt", &format!("rogue edit by {name}"));
            println!("Done (edited outside.txt).");
        }
        "bad-float-money" => {
            fs::create_dir_all("src").expect("mkdir src");
            fs::write("src/invoice.rs", "pub struct Invoice { pub total_amount: f64 }\n").expect("write bad float");
            println!("Created src/invoice.rs with f64 money.");
        }
        "good-decimal-money" => {
            fs::create_dir_all("src").expect("mkdir src");
            fs::write("src/invoice.rs", "pub struct Invoice { pub total_minor_units: i64 }\n").expect("write fixed money");
            println!("Fixed src/invoice.rs to use exact integer minor units.");
        }
        "touch" => {
            append_line(&file, "ok");
            println!("Created {file}.");
        }
        "approve" => println!("Reviewed the diff; logic and scope look correct.\nVERDICT: APPROVE"),
        "reject" => println!("Problems found:\n1. add tests for the new behaviour\nVERDICT: CHANGES_REQUESTED"),
        "noverdict" => println!("I looked at it and it seems fine."),
        other => {
            eprintln!("fake-ai: unknown step `{other}`");
            exit(64);
        }
    }
}
