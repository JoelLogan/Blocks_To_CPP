//! Process behaviour on Unix (run on Linux in CI): limits, whole-tree
//! termination, environment isolation, input and exit decoding.
#![cfg(unix)]
// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use b2c_process::{
    CancelToken, Captured, Command, Containment, Crash, ExitStatus, Limits, ProcessError, ProcessGroup,
    Stdin, run_captured, run_interactive,
};

const SH: &str = "/bin/sh";

fn sh(script: &str) -> Command {
    let mut command = Command::new(SH, std::env::temp_dir()).unwrap();
    command.args(["-c", script]).env("PATH", "/usr/bin:/bin");
    command
}

fn captured(script: &str) -> Captured {
    run_captured(&sh(script)).unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The state letter of a process (`None` once it no longer exists).
#[cfg(target_os = "linux")]
fn process_state(pid: i32) -> Option<char> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after = stat.rsplit_once(')')?.1;
    after.split_whitespace().next()?.chars().next()
}

/// Whether a process is gone: it no longer exists, or it is a zombie
/// waiting for a (possibly non-reaping) init to collect it.
#[cfg(target_os = "linux")]
fn is_gone(pid: i32) -> bool {
    // Killing is asynchronous: give the kernel a moment.
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        match process_state(pid) {
            None | Some('Z' | 'X') => return true,
            Some(_) if Instant::now() > until => return false,
            Some(_) => thread::sleep(Duration::from_millis(10)),
        }
    }
}

#[cfg(target_os = "linux")]
fn pgrp_of(stat: &str) -> i32 {
    let after = stat.rsplit_once(')').unwrap().1;
    after.split_whitespace().nth(2).unwrap().parse().unwrap()
}

fn read_pid(path: &Path) -> i32 {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(content) = std::fs::read_to_string(path)
            && let Ok(pid) = content.trim().parse()
        {
            return pid;
        }
        assert!(Instant::now() < until, "no PID written to {}", path.display());
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn exit_codes_are_reported() {
    let result = captured("exit 7");
    assert_eq!(result.status, ExitStatus::Exited(7));
    assert!(!result.timed_out && !result.cancelled && !result.too_many_processes);
    assert_eq!(captured("true").status, ExitStatus::Exited(0));
    assert!(captured("true").status.success());
}

#[test]
fn signals_are_decoded() {
    let segv = captured("kill -SEGV $$");
    assert_eq!(segv.status, ExitStatus::Signaled(11));
    assert_eq!(segv.status.crash(), Some(Crash::MemoryAccess));
    assert_eq!(segv.status.shell_code(), 139);
    let fpe = captured("kill -FPE $$");
    assert_eq!(fpe.status.crash(), Some(Crash::DivisionByZero));
    let abrt = captured("kill -ABRT $$");
    assert_eq!(abrt.status.crash(), Some(Crash::Aborted));
}

#[test]
fn stdout_and_stderr_are_separate() {
    let result = captured("echo out; echo err >&2");
    assert_eq!(text(&result.stdout), "out\n");
    assert_eq!(text(&result.stderr), "err\n");
    assert!(!result.stdout_truncated && !result.stderr_truncated);
}

#[test]
fn environment_is_isolated() {
    // Without a scope: exactly the command's environment. (A cgroup scope
    // adds the bus variables `systemd-run` needs; tests/containment.rs.)
    let mut command = Command::new("/usr/bin/env", std::env::temp_dir()).unwrap();
    command
        .env("ONLY_THIS", "yes")
        .containment(Containment::ProcessGroupOnly);
    let result = run_captured(&command).unwrap();
    assert_eq!(text(&result.stdout), "ONLY_THIS=yes\n");

    let mut empty = Command::new("/usr/bin/env", std::env::temp_dir()).unwrap();
    empty.containment(Containment::ProcessGroupOnly);
    assert_eq!(text(&run_captured(&empty).unwrap().stdout), "");
}

#[test]
fn working_directory_is_used() {
    let dir = tempfile::tempdir().unwrap();
    let canonical = dir.path().canonicalize().unwrap();
    let mut command = Command::new("/bin/pwd", &canonical).unwrap();
    command.arg("-P");
    let result = run_captured(&command).unwrap();
    assert_eq!(PathBuf::from(text(&result.stdout).trim()), canonical);
}

#[test]
fn arguments_are_not_interpreted_by_a_shell() {
    let mut command = Command::new("/bin/echo", std::env::temp_dir()).unwrap();
    command.args(["$HOME", "; rm -rf /", "`id`", "a b"]);
    let result = run_captured(&command).unwrap();
    assert_eq!(text(&result.stdout), "$HOME ; rm -rf / `id` a b\n");
}

#[test]
fn stdin_bytes_are_delivered() {
    let mut command = Command::new("/bin/cat", std::env::temp_dir()).unwrap();
    command.stdin(Stdin::Bytes(b"line one\nline two\n".to_vec()));
    let result = run_captured(&command).unwrap();
    assert_eq!(text(&result.stdout), "line one\nline two\n");
}

#[test]
fn large_stdin_and_stdout_do_not_deadlock() {
    let input: Vec<u8> = (0..3_000_000_u32)
        .map(|i| b"0123456789abcdef"[(i % 16) as usize])
        .collect();
    let mut command = Command::new("/bin/cat", std::env::temp_dir()).unwrap();
    command
        .stdin(Stdin::Bytes(input.clone()))
        .timeout(Duration::from_secs(30));
    let result = run_captured(&command).unwrap();
    assert!(!result.timed_out);
    assert_eq!(result.stdout, input);
}

#[test]
fn unread_stdin_does_not_block() {
    let mut command = sh("exit 0");
    command.stdin(Stdin::Bytes(vec![b'x'; 5_000_000]));
    let start = Instant::now();
    assert!(run_captured(&command).unwrap().status.success());
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn stdin_file_is_delivered() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.txt");
    std::fs::write(&input, "from a file\n").unwrap();
    let mut command = Command::new("/bin/cat", std::env::temp_dir()).unwrap();
    command.stdin(Stdin::File(input));
    assert_eq!(text(&run_captured(&command).unwrap().stdout), "from a file\n");

    command.stdin(Stdin::File(dir.path().join("missing.txt")));
    assert!(matches!(
        run_captured(&command),
        Err(ProcessError::StdinFile { .. })
    ));
}

#[test]
fn null_stdin_reads_end_of_file() {
    let result = captured("if read line; then echo got; else echo eof; fi");
    assert_eq!(text(&result.stdout), "eof\n");
}

#[test]
fn output_cap_truncates_a_flood() {
    let mut command = sh("head -c 20000000 /dev/zero; echo done >&2");
    command.limits(Limits {
        stdout_cap: 1000,
        timeout: Some(Duration::from_mins(1)),
        ..Limits::default()
    });
    let result = run_captured(&command).unwrap();
    assert!(result.status.success(), "{:?}", result.status);
    assert_eq!(result.stdout.len(), 1000);
    assert!(result.stdout_truncated);
    assert_eq!(text(&result.stderr), "done\n");
    assert!(!result.stderr_truncated);
}

#[test]
fn relative_and_missing_programs_are_refused() {
    assert!(matches!(
        Command::new("sh", std::env::temp_dir()),
        Err(ProcessError::RelativeProgram(_))
    ));
    let missing = Command::new("/nonexistent/b2c-no-such-program", std::env::temp_dir()).unwrap();
    let error = run_captured(&missing).unwrap_err();
    assert!(matches!(error, ProcessError::Spawn { .. }));
    assert!(
        error
            .to_string()
            .starts_with("cannot start /nonexistent/b2c-no-such-program")
    );
}

#[test]
fn missing_working_directory_is_a_spawn_error() {
    let command = Command::new(SH, "/nonexistent/b2c-dir").unwrap();
    assert!(matches!(run_captured(&command), Err(ProcessError::Spawn { .. })));
}

#[cfg(target_os = "linux")]
#[test]
fn timeout_kills_the_child_and_its_grandchildren() {
    let mut command = sh("sleep 30 & echo $!; sleep 30");
    // The group: the shell must have started within the 300 ms, which the
    // start of a cgroup scope could delay on a slow runner.
    command
        .timeout(Duration::from_millis(300))
        .containment(Containment::ProcessGroupOnly);
    let start = Instant::now();
    let result = run_captured(&command).unwrap();
    assert!(result.timed_out);
    assert!(start.elapsed() < Duration::from_secs(10));
    assert_eq!(result.status, ExitStatus::Signaled(9));
    let grandchild: i32 = text(&result.stdout).trim().parse().unwrap();
    assert!(is_gone(grandchild), "grandchild {grandchild} survived");
}

#[cfg(target_os = "linux")]
#[test]
fn leftover_background_processes_are_killed_at_exit() {
    // Without whole-tree cleanup this would wait 30 s for the pipe to close.
    let start = Instant::now();
    let result = captured("sleep 30 & echo $!");
    assert!(result.status.success());
    assert!(start.elapsed() < Duration::from_secs(10));
    let grandchild: i32 = text(&result.stdout).trim().parse().unwrap();
    assert!(is_gone(grandchild));
}

#[cfg(target_os = "linux")]
#[test]
fn escaped_processes_cannot_hold_the_run_open() {
    if !Path::new("/usr/bin/setsid").exists() {
        return;
    }
    // `setsid` leaves the process group but keeps the stdout pipe open. The
    // shell waits until it has (field 6 of its stat line, the session ID, is
    // its own PID): the group cleanup at the shell's exit would otherwise
    // kill it first, and the test would prove nothing. Only without a cgroup
    // scope can a process escape like this (a scope kills it when the
    // program exits; tests/containment.rs).
    let mut command = sh("/usr/bin/setsid sleep 6 & p=$!; \
                          while [ \"$(cut -d' ' -f6 /proc/$p/stat)\" != \"$p\" ]; do sleep 0.01; done; \
                          echo started");
    command
        .containment(Containment::ProcessGroupOnly)
        .timeout(Duration::from_secs(30));
    let start = Instant::now();
    let result = run_captured(&command).unwrap();
    assert!(result.status.success());
    assert!(start.elapsed() < Duration::from_secs(5), "{:?}", start.elapsed());
    assert_eq!(text(&result.stdout), "started\n");
}

#[test]
fn cancel_token_stops_a_run_from_another_thread() {
    let token = CancelToken::new();
    let mut command = sh("sleep 30");
    command.cancel_token(&token);
    let canceller = {
        let token = token.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            token.cancel();
        })
    };
    let start = Instant::now();
    let result = run_captured(&command).unwrap();
    canceller.join().unwrap();
    assert!(result.cancelled);
    assert!(!result.timed_out);
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn captured_runs_get_sigterm_first_when_a_grace_is_set() {
    // The compiler's limits set a 2 s grace (07 §7.5.4): a timeout or a
    // cancel sends SIGTERM to the group, and the program may clean up.
    let mut command = sh("trap 'echo got-term; exit 7' TERM; while :; do sleep 0.05; done");
    // The trap must be in place within the 300 ms (see above).
    command.containment(Containment::ProcessGroupOnly).limits(Limits {
        timeout: Some(Duration::from_millis(300)),
        grace: Some(Duration::from_secs(10)),
        ..Limits::default()
    });
    let start = Instant::now();
    let result = run_captured(&command).unwrap();
    assert!(result.timed_out);
    assert_eq!(result.status, ExitStatus::Exited(7));
    assert_eq!(text(&result.stdout), "got-term\n");
    assert!(start.elapsed() < Duration::from_secs(5), "{:?}", start.elapsed());
}

#[test]
fn captured_runs_are_killed_after_the_grace() {
    let token = CancelToken::new();
    let mut command = sh("trap '' TERM; while :; do sleep 0.05; done");
    // The trap must be in place within the 200 ms (see above).
    command.containment(Containment::ProcessGroupOnly);
    command.cancel_token(&token).limits(Limits {
        grace: Some(Duration::from_millis(600)),
        ..Limits::default()
    });
    let canceller = {
        let token = token.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            token.cancel();
        })
    };
    let start = Instant::now();
    let result = run_captured(&command).unwrap();
    canceller.join().unwrap();
    let took = start.elapsed();
    assert!(result.cancelled);
    assert_eq!(result.status, ExitStatus::Signaled(9));
    // 200 ms until the cancel, then the 600 ms grace before SIGKILL.
    assert!(took >= Duration::from_millis(750), "{took:?}");
    assert!(took < Duration::from_secs(5), "{took:?}");
}

#[test]
fn captured_runs_without_a_grace_are_killed_at_once() {
    let mut command = sh("trap 'echo got-term' TERM; while :; do sleep 0.05; done");
    command.timeout(Duration::from_millis(200));
    let result = run_captured(&command).unwrap();
    assert!(result.timed_out);
    assert_eq!(result.status, ExitStatus::Signaled(9));
    assert_eq!(text(&result.stdout), "");
}

#[test]
fn cancelled_token_prevents_starting() {
    let token = CancelToken::new();
    token.cancel();
    let mut command = sh("exit 0");
    command.cancel_token(&token);
    assert!(matches!(run_captured(&command), Err(ProcessError::Cancelled)));
    assert!(matches!(run_interactive(&command), Err(ProcessError::Cancelled)));
}

#[cfg(target_os = "linux")]
#[test]
fn captured_runs_lead_their_own_process_group() {
    let result = captured("cat /proc/$$/stat");
    let stat = text(&result.stdout);
    let pid: i32 = stat.split_whitespace().next().unwrap().parse().unwrap();
    assert_eq!(pgrp_of(&stat), pid);
}

#[cfg(target_os = "linux")]
#[test]
fn memory_limit_is_applied() {
    // The fallback's address-space limit (a cgroup scope enforces
    // `MemoryMax` instead; tests/containment.rs). It is set on the child
    // after it has started, so the shell waits for it (at most 5 s) instead
    // of reading it at once.
    let mut command = sh(
        "i=0; while [ \"$(ulimit -v)\" = unlimited ] && [ $i -lt 500 ]; do \
                          sleep 0.01; i=$((i+1)); done; ulimit -v",
    );
    command.containment(Containment::ProcessGroupOnly).limits(Limits {
        memory: Some(512 * 1024 * 1024),
        ..Limits::default()
    });
    let result = run_captured(&command).unwrap();
    assert_eq!(text(&result.stdout).trim(), "524288");
}

#[cfg(target_os = "linux")]
#[test]
fn process_watchdog_stops_a_growing_tree() {
    let mut command = sh("for i in 1 2 3 4 5 6 7 8; do sleep 30 & done; wait");
    command.containment(Containment::ProcessGroupOnly).limits(Limits {
        processes: Some(4),
        timeout: Some(Duration::from_secs(20)),
        ..Limits::default()
    });
    let start = Instant::now();
    let result = run_captured(&command).unwrap();
    assert!(result.too_many_processes);
    assert!(!result.timed_out);
    assert!(start.elapsed() < Duration::from_secs(10));
}

#[test]
fn duration_is_measured() {
    let result = captured("sleep 0.2");
    assert!(
        result.duration >= Duration::from_millis(150),
        "{:?}",
        result.duration
    );
    assert!(result.duration < Duration::from_secs(10));
}

#[test]
fn interactive_runs_report_status() {
    let mut command = sh("exit 5");
    command.process_group(ProcessGroup::New);
    let finished = run_interactive(&command).unwrap();
    assert_eq!(finished.status, ExitStatus::Exited(5));
    assert!(!finished.timed_out);
}

#[test]
fn interactive_runs_read_a_stdin_file() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in.txt");
    std::fs::write(&input, "hello\n").unwrap();
    let mut command = sh("read x; test \"$x\" = hello");
    command.stdin(Stdin::File(input));
    assert!(run_interactive(&command).unwrap().status.success());
}

#[cfg(target_os = "linux")]
fn interactive_timeout_kills_tree(group: ProcessGroup) {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let mut command = sh("sleep 30 & echo $! > \"$1\"; sleep 30");
    command
        .arg("sh")
        .arg(&pid_file)
        .process_group(group)
        .limits(Limits {
            timeout: Some(Duration::from_millis(400)),
            grace: Some(Duration::from_millis(200)),
            ..Limits::default()
        });
    let start = Instant::now();
    let finished = run_interactive(&command).unwrap();
    assert!(finished.timed_out);
    assert!(start.elapsed() < Duration::from_secs(10));
    // `sh` dies of the polite SIGTERM. In the shared-group case `sh` can
    // instead see its `sleep` die of SIGTERM first and exit with 128 + 15
    // before the signal reaches `sh` itself; both mean the polite stop
    // ended the tree.
    assert!(
        matches!(
            finished.status,
            ExitStatus::Signaled(15) | ExitStatus::Exited(143)
        ),
        "unexpected status {:?} ({group:?})",
        finished.status
    );
    let grandchild = read_pid(&pid_file);
    assert!(
        is_gone(grandchild),
        "grandchild {grandchild} survived ({group:?})"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn an_absurd_grace_does_not_panic_the_stop() {
    // `sleep` ends at the polite SIGTERM, so the endless grace never matters.
    let mut command = sh("exec sleep 30");
    command.limits(Limits {
        timeout: Some(Duration::from_millis(200)),
        grace: Some(Duration::MAX),
        ..Limits::default()
    });
    let finished = run_interactive(&command).unwrap();
    assert!(finished.timed_out);
    assert_eq!(finished.status, ExitStatus::Signaled(15));
}

#[cfg(target_os = "linux")]
#[test]
fn interactive_timeout_kills_a_new_group() {
    interactive_timeout_kills_tree(ProcessGroup::New);
}

#[cfg(target_os = "linux")]
#[test]
fn interactive_timeout_kills_a_shared_group_tree() {
    interactive_timeout_kills_tree(ProcessGroup::Shared);
}

#[cfg(target_os = "linux")]
#[test]
fn shared_group_runs_stay_in_our_process_group() {
    // Staying in the terminal's foreground group is what lets an
    // interactive program read the terminal (a background group would get
    // SIGTTIN) and receive Ctrl+C.
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("stat");
    let mut command = sh("cat /proc/$$/stat > \"$1\"");
    command.arg("sh").arg(&out).process_group(ProcessGroup::Shared);
    assert!(run_interactive(&command).unwrap().status.success());
    let child = std::fs::read_to_string(&out).unwrap();
    let me = std::fs::read_to_string("/proc/self/stat").unwrap();
    assert_eq!(pgrp_of(&child), pgrp_of(&me));

    command.process_group(ProcessGroup::New);
    assert!(run_interactive(&command).unwrap().status.success());
    let child = std::fs::read_to_string(&out).unwrap();
    let child_pid: i32 = child.split_whitespace().next().unwrap().parse().unwrap();
    assert_eq!(pgrp_of(&child), child_pid);
}

#[cfg(target_os = "linux")]
#[test]
fn shared_group_kill_ignores_sigterm_after_grace() {
    let dir = tempfile::tempdir().unwrap();
    let pid_file = dir.path().join("pid");
    let mut command = sh("trap '' TERM; echo $$ > \"$1\"; while :; do sleep 1; done");
    command
        .arg("sh")
        .arg(&pid_file)
        .process_group(ProcessGroup::Shared)
        .limits(Limits {
            timeout: Some(Duration::from_millis(300)),
            grace: Some(Duration::from_millis(200)),
            ..Limits::default()
        });
    let finished = run_interactive(&command).unwrap();
    assert!(finished.timed_out);
    assert_eq!(finished.status, ExitStatus::Signaled(9));
}
