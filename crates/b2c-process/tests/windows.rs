//! Process behaviour on Windows (run in CI): output capture, exit codes,
//! input, the environment, timeouts and the `.exe`-only rule, through the
//! Job Object containment of `platform/windows.rs`.
#![cfg(windows)]
// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use b2c_process::{Command, ExitStatus, Limits, ProcessError, Stdin, run_captured};

fn system32() -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32")
}

/// `cmd.exe /d /c <script>` with only `SystemRoot` in its environment.
fn cmd(script: &str) -> Command {
    let mut command = Command::new(system32().join("cmd.exe"), std::env::temp_dir()).unwrap();
    command
        .args(["/d", "/c", script])
        .env("SystemRoot", std::env::var_os("SystemRoot").unwrap());
    command
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).replace("\r\n", "\n")
}

#[test]
fn output_and_exit_codes_are_captured() {
    let captured = run_captured(&cmd("echo out& echo err 1>&2& exit 3")).unwrap();
    assert_eq!(captured.status, ExitStatus::Exited(3));
    assert_eq!(text(&captured.stdout).trim(), "out");
    assert_eq!(text(&captured.stderr).trim(), "err");
    assert!(!captured.timed_out);
}

#[test]
fn input_bytes_reach_the_program() {
    let mut command = cmd("findstr x");
    command.stdin(Stdin::Bytes(b"one\nx marks the spot\nthree\n".to_vec()));
    let captured = run_captured(&command).unwrap();
    assert!(captured.status.success(), "{:?}", captured.status);
    assert_eq!(text(&captured.stdout).trim(), "x marks the spot");
}

#[test]
fn only_the_given_environment_is_passed() {
    let mut command = cmd("set B2C_");
    command.env("B2C_PASSED", "yes");
    // Variables of this test process (CI sets many) never reach the child.
    let captured = run_captured(&command).unwrap();
    let listing = text(&captured.stdout);
    assert!(listing.contains("B2C_PASSED=yes"), "{listing}");
    assert_eq!(
        listing.lines().filter(|line| line.starts_with("B2C_")).count(),
        1,
        "{listing}"
    );
}

#[test]
fn a_timeout_stops_the_program() {
    let mut command = Command::new(system32().join("ping.exe"), std::env::temp_dir()).unwrap();
    command
        .args(["-n", "30", "127.0.0.1"])
        .env("SystemRoot", std::env::var_os("SystemRoot").unwrap())
        .limits(Limits {
            timeout: Some(Duration::from_millis(500)),
            ..Limits::default()
        });
    let started = Instant::now();
    let captured = run_captured(&command).unwrap();
    assert!(captured.timed_out);
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn a_program_and_its_children_are_stopped_together() {
    // cmd starts ping, which would keep running for ~30 s if only cmd were
    // stopped; the Job Object ends both.
    let mut command = cmd("ping -n 30 127.0.0.1 > nul");
    command.limits(Limits {
        timeout: Some(Duration::from_millis(500)),
        ..Limits::default()
    });
    let started = Instant::now();
    let captured = run_captured(&command).unwrap();
    assert!(captured.timed_out);
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn scripts_are_never_run() {
    for name in ["build.bat", "build.cmd", "tool.com"] {
        assert!(matches!(
            Command::new(std::env::temp_dir().join(name), std::env::temp_dir()),
            Err(ProcessError::NotAnExe(_))
        ));
    }
}
