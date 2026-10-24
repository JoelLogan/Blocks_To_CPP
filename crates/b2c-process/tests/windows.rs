//! Process behaviour on Windows (run in CI): output capture, exit codes,
//! input, the environment, timeouts and the `.exe`-only rule, through the
//! Job Object containment of `platform/windows.rs`; the explicit handle list
//! of captured runs, and the job's limit reports.
#![cfg(windows)]
// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::print_stderr)]
// One test makes a handle inheritable in this process, which needs a Win32
// call (it has a SAFETY comment).
#![allow(unsafe_code)]

use std::io::Read as _;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use b2c_process::{
    Command, ContainmentLevel, ExitStatus, Limits, ProcessError, Stdin, containment_level, run_captured,
};

fn system32() -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32")
}

/// `cmd.exe /d /c <script>` with only `SystemRoot` and a `PATH` of System32
/// (so the script can start `findstr` and `ping`) in its environment.
fn cmd(script: &str) -> Command {
    let mut command = Command::new(system32().join("cmd.exe"), std::env::temp_dir()).unwrap();
    command
        .args(["/d", "/c", script])
        .env("SystemRoot", std::env::var_os("SystemRoot").unwrap())
        .env("PATH", system32());
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

#[test]
fn the_level_is_always_a_job_object() {
    assert_eq!(containment_level(), ContainmentLevel::JobObject);
}

#[test]
fn a_captured_child_inherits_no_other_handle() {
    use std::os::windows::io::{AsRawHandle as _, OwnedHandle};
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};

    let (mut reader, writer) = std::io::pipe().unwrap();
    let writer = OwnedHandle::from(writer);
    // SAFETY: `writer` is a valid handle owned by this test.
    let ok =
        unsafe { SetHandleInformation(writer.as_raw_handle(), HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
    assert_ne!(ok, 0, "SetHandleInformation failed");
    // ping runs for about 5 s; had it inherited the write end, reading the
    // pipe would block until it exits.
    let mut command = Command::new(system32().join("ping.exe"), std::env::temp_dir()).unwrap();
    command
        .args(["-n", "6", "127.0.0.1"])
        .env("SystemRoot", std::env::var_os("SystemRoot").unwrap());
    let run = thread::spawn(move || run_captured(&command));
    // The child is created (and would inherit) well within this time.
    thread::sleep(Duration::from_secs(1));
    drop(writer);
    let started = Instant::now();
    let mut rest = Vec::new();
    reader.read_to_end(&mut rest).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the captured child held an inherited handle: {:?}",
        started.elapsed()
    );
    let captured = run.join().unwrap().unwrap();
    assert!(captured.status.success(), "{captured:?}");
}

#[test]
fn a_captured_child_reads_a_file_or_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.txt");
    std::fs::write(&input, "x from a file\r\nnot this\r\n").unwrap();
    let mut command = cmd("findstr x");
    command.stdin(Stdin::File(input));
    let captured = run_captured(&command).unwrap();
    assert_eq!(text(&captured.stdout).trim(), "x from a file");
    // No input: findstr reads end-of-file at once and finds nothing.
    let nothing = run_captured(&cmd("findstr x")).unwrap();
    assert_eq!(nothing.status, ExitStatus::Exited(1));
    assert!(!nothing.timed_out);
}

#[test]
fn the_job_memory_limit_is_reported_as_out_of_memory() {
    let powershell = system32().join(r"WindowsPowerShell\v1.0\powershell.exe");
    if !powershell.is_file() {
        eprintln!("skipped: no Windows PowerShell at {}", powershell.display());
        return;
    }
    // A 1 GiB array is committed at once, over the job's 512 MiB.
    let mut command = Command::new(powershell, std::env::temp_dir()).unwrap();
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "$block = New-Object byte[] 1GB; 'allocated'",
        ])
        .envs(std::env::vars_os())
        .limits(Limits {
            rss_limit: Some(512 * 1024 * 1024),
            timeout: Some(Duration::from_mins(2)),
            ..Limits::default()
        });
    let captured = run_captured(&command).unwrap();
    assert!(captured.out_of_memory, "{captured:?}");
    assert!(!captured.timed_out);
    assert!(!text(&captured.stdout).contains("allocated"), "{captured:?}");
}

#[test]
fn the_job_process_limit_is_reported() {
    // Twenty background pings, far over the job's four processes.
    let mut command = cmd("for /l %i in (1,1,20) do @start /b ping -n 30 127.0.0.1 > nul");
    command.limits(Limits {
        processes: Some(4),
        timeout: Some(Duration::from_mins(1)),
        ..Limits::default()
    });
    let started = Instant::now();
    let captured = run_captured(&command).unwrap();
    assert!(captured.too_many_processes, "{captured:?}");
    assert!(!captured.timed_out);
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );
}
