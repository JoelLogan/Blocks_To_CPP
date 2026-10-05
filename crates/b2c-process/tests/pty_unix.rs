//! Sessions on Unix (run on Linux in CI, headless: the tests create their
//! own pseudo-terminals and need no real terminal): the terminal the program
//! sees, input and output, resizing, Ctrl+C, stop with a grace period,
//! whole-tree cleanup, and the pipe fallback.
#![cfg(unix)]
// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fmt::Write as _;
use std::io::{ErrorKind, Read, Write as _};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use b2c_process::{
    CancelToken, Captured, Command, Containment, ContainmentLevel, Crash, ExitStatus, IoMode, Limits,
    ProcessError, PtyChild, PtyExit, PtySize, Stdin, containment_level, run_captured, spawn_piped, spawn_pty,
};

const SIZE: PtySize = PtySize { cols: 100, rows: 30 };
/// Generous bound for things that should take milliseconds, for loaded CI
/// runners.
const PATIENCE: Duration = Duration::from_secs(20);

fn sh(script: &str) -> Command {
    let mut command = Command::new("/bin/sh", std::env::temp_dir()).unwrap();
    command.args(["-c", script]).env("PATH", "/usr/bin:/bin");
    command
}

/// Collects a session's output on a thread of its own.
struct Collector {
    data: Arc<Mutex<Vec<u8>>>,
    eof: Arc<AtomicBool>,
}

impl Collector {
    fn start(child: &mut PtyChild) -> Self {
        let mut reader = child.take_reader().expect("the reader is taken once");
        let data = Arc::new(Mutex::new(Vec::new()));
        let eof = Arc::new(AtomicBool::new(false));
        let (sink, done) = (Arc::clone(&data), Arc::clone(&eof));
        thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(read) => sink.lock().unwrap().extend_from_slice(&buffer[..read]),
                }
            }
            done.store(true, Ordering::SeqCst);
        });
        Self { data, eof }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.data.lock().unwrap()).into_owned()
    }

    /// Waits until the output contains `needle`.
    fn wait_for(&self, needle: &str) {
        let until = Instant::now() + PATIENCE;
        while !self.text().contains(needle) {
            assert!(
                Instant::now() < until,
                "{needle:?} never appeared in {:?}",
                self.text()
            );
            thread::sleep(Duration::from_millis(10));
        }
    }

    /// Waits for end-of-file and returns everything, failing after `limit`.
    fn finish_within(&self, limit: Duration) -> String {
        let until = Instant::now() + limit;
        while !self.eof.load(Ordering::SeqCst) {
            assert!(
                Instant::now() < until,
                "no end-of-file after {limit:?}; output so far: {:?}",
                self.text()
            );
            thread::sleep(Duration::from_millis(10));
        }
        self.text()
    }

    fn finish(&self) -> String {
        self.finish_within(PATIENCE)
    }
}

/// Waits for the session to end, killing it (and failing) after `limit`.
fn wait_within(child: &mut PtyChild, limit: Duration) -> PtyExit {
    let until = Instant::now() + limit;
    loop {
        if let Some(exit) = child.try_wait().unwrap() {
            return exit;
        }
        if Instant::now() > until {
            child.kill();
            panic!("the program was still running after {limit:?}");
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// The first number after `label` in `text`.
fn number_after(text: &str, label: &str) -> i32 {
    let start = text
        .find(label)
        .unwrap_or_else(|| panic!("{label:?} not in {text:?}"))
        + label.len();
    text[start..]
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .unwrap()
}

/// Whether a process is gone: it no longer exists, or it is a zombie
/// waiting for a (possibly non-reaping) init to collect it.
#[cfg(target_os = "linux")]
fn is_gone(pid: i32) -> bool {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        let state = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| {
                let after = stat.rsplit_once(')')?.1.to_owned();
                after.split_whitespace().next()?.chars().next()
            });
        match state {
            None | Some('Z' | 'X') => return true,
            Some(_) if Instant::now() > until => return false,
            Some(_) => thread::sleep(Duration::from_millis(10)),
        }
    }
}

/// Whether a process is gone right now (no waiting).
#[cfg(target_os = "linux")]
fn is_gone_now(pid: i32) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).map_or(true, |stat| {
        stat.rsplit_once(')')
            .and_then(|(_, after)| after.split_whitespace().next())
            .is_none_or(|state| state == "Z" || state == "X")
    })
}

/// Kills a process that escaped its session.
fn kill_pid(pid: i32) {
    if let Some(pid) = rustix::process::Pid::from_raw(pid) {
        let _ = rustix::process::kill_process(pid, rustix::process::Signal::KILL);
    }
}

#[test]
fn the_program_sees_a_terminal() {
    let mut child = spawn_pty(&sh("test -t 0 && test -t 1 && test -t 2 && echo is-a-tty"), SIZE).unwrap();
    assert_eq!(child.mode(), IoMode::Pty);
    // The level this system offers: a cgroup scope where the user's
    // service manager gives one (CI's test-cgroup job), else the group.
    assert_eq!(child.containment(), containment_level());
    assert_ne!(child.containment(), ContainmentLevel::JobObject);
    let output = Collector::start(&mut child);
    assert!(output.finish().contains("is-a-tty\r\n"), "{:?}", output.text());
    let exit = child.wait().unwrap();
    assert_eq!(exit.status, ExitStatus::Exited(0));
    assert!(!exit.stopped && !exit.timed_out && !exit.too_many_processes);
}

#[test]
fn stty_reports_the_size_and_follows_a_resize() {
    let mut child = spawn_pty(&sh("stty size; read line; stty size"), SIZE).unwrap();
    let output = Collector::start(&mut child);
    output.wait_for("30 100\r\n");
    child.resize(PtySize { cols: 120, rows: 40 }).unwrap();
    child.writer().write_all(b"\n").unwrap();
    let text = output.finish();
    assert!(text.ends_with("40 120\r\n"), "{text:?}");
    assert!(child.wait().unwrap().status.success());
}

#[test]
fn input_reaches_the_program_and_is_echoed() {
    let mut child = spawn_pty(&sh("read line; echo \"got:$line\""), SIZE).unwrap();
    let output = Collector::start(&mut child);
    child.writer().write_all(b"hello world\n").unwrap();
    let text = output.finish();
    // The terminal echoes the line, then the program answers.
    assert!(text.contains("hello world\r\n"), "{text:?}");
    assert!(text.contains("got:hello world\r\n"), "{text:?}");
    assert!(child.wait().unwrap().status.success());
}

#[test]
fn writer_clones_write_to_the_same_program() {
    let mut child = spawn_pty(&sh("read a; read b; echo \"$a+$b\""), SIZE).unwrap();
    let output = Collector::start(&mut child);
    let mut first = child.writer();
    let mut second = first.clone();
    first.write_all(b"one\n").unwrap();
    second.write_all(b"two\n").unwrap();
    first.flush().unwrap();
    assert!(output.finish().contains("one+two\r\n"), "{:?}", output.text());
    child.wait().unwrap();
}

#[test]
fn ctrl_c_interrupts_the_program() {
    let mut command = Command::new("/bin/sleep", std::env::temp_dir()).unwrap();
    command.arg("600");
    let mut child = spawn_pty(&command, SIZE).unwrap();
    let output = Collector::start(&mut child);
    // The program is a session leader with the terminal as its controlling
    // terminal by the time spawn_pty returns, so the line discipline turns
    // 0x03 into SIGINT for it.
    child.writer().write_all(&[0x03]).unwrap();
    let exit = wait_within(&mut child, PATIENCE);
    assert_eq!(exit.status, ExitStatus::Signaled(2));
    assert_eq!(exit.status.crash(), Some(Crash::Interrupted));
    assert_eq!(exit.status.describe(), "Stopped by Ctrl+C (SIGINT).");
    assert!(!exit.stopped);
    output.finish();
}

/// The signals a session resets to their default action in the program.
#[cfg(target_os = "linux")]
const RESET_SIGNALS: [rustix::process::Signal; 10] = {
    use rustix::process::Signal;
    [
        Signal::HUP,
        Signal::INT,
        Signal::QUIT,
        Signal::PIPE,
        Signal::ALARM,
        Signal::TERM,
        Signal::CHILD,
        Signal::TSTP,
        Signal::TTIN,
        Signal::TTOU,
    ]
};

/// The signal set on the line `field:` of a `/proc/<pid>/status` text (bit
/// `n - 1` stands for signal `n`).
#[cfg(target_os = "linux")]
fn signal_set(status: &str, field: &str) -> u64 {
    let line = status
        .lines()
        .find_map(|line| line.strip_prefix(field)?.strip_prefix(':'))
        .unwrap_or_else(|| panic!("no {field} line in {status:?}"));
    u64::from_str_radix(line.trim(), 16).unwrap()
}

/// The bits of `signals` in a `/proc/<pid>/status` signal set.
#[cfg(target_os = "linux")]
fn bits(signals: &[rustix::process::Signal]) -> u64 {
    signals
        .iter()
        .map(|signal| 1_u64 << (signal.as_raw() - 1))
        .fold(0, |set, bit| set | bit)
}

#[cfg(target_os = "linux")]
#[test]
fn programs_start_with_default_signals() {
    // The program reads its own status: nothing blocked, and none of the
    // signals a console program relies on ignored, whatever this process
    // inherited (see the next test).
    let mut command = Command::new("/bin/cat", std::env::temp_dir()).unwrap();
    command.arg("/proc/self/status");
    let mut child = spawn_pty(&command, SIZE).unwrap();
    let output = Collector::start(&mut child);
    let status = output.finish();
    assert!(child.wait().unwrap().status.success());
    assert_eq!(signal_set(&status, "SigBlk"), 0, "{status}");
    assert_eq!(
        signal_set(&status, "SigIgn") & bits(&RESET_SIGNALS),
        0,
        "{status}"
    );
}

/// Runs `script` with `sh -c` in this process's environment, with SIGINT,
/// SIGQUIT, SIGHUP and SIGTERM ignored; `$0` is this test binary and `$@`
/// is `args`.
fn with_signals_ignored(script: &str, args: &[&str]) -> Captured {
    let mut command = Command::new("/bin/sh", std::env::temp_dir()).unwrap();
    command
        .args(["-c", &format!("trap '' INT QUIT HUP TERM; {script}")])
        .arg(std::env::current_exe().unwrap())
        .args(args)
        .envs(std::env::vars_os())
        .timeout(PATIENCE * 3);
    let captured = run_captured(&command).unwrap();
    assert!(!captured.timed_out, "{script} timed out");
    captured
}

/// An ignored signal survives `exec`, so the app can start with SIGINT and
/// SIGQUIT ignored (a background job of a non-interactive shell, as in
/// `app &` from a script) or SIGHUP ignored (`nohup`). Its programs must
/// not inherit that, or Ctrl+C would do nothing: this runs the Ctrl+C test
/// (on Linux also the test above) again in a copy of this test binary that
/// ignores SIGINT, SIGQUIT, SIGHUP and SIGTERM.
#[test]
fn ctrl_c_works_even_when_this_process_ignores_sigint() {
    let mut tests = vec!["ctrl_c_interrupts_the_program"];
    #[cfg(target_os = "linux")]
    {
        use rustix::process::Signal;
        tests.push("programs_start_with_default_signals");
        // The premise: a program started that way has them ignored.
        let premise = with_signals_ignored("exec /bin/cat /proc/self/status", &[]);
        let status = String::from_utf8_lossy(&premise.stdout);
        let expected = bits(&[Signal::INT, Signal::QUIT, Signal::HUP, Signal::TERM]);
        assert_eq!(signal_set(&status, "SigIgn") & expected, expected, "{status}");
    }
    let rerun = with_signals_ignored("exec \"$0\" --exact --test-threads=1 \"$@\"", &tests);
    let report = String::from_utf8_lossy(&rerun.stdout);
    assert!(
        rerun.status.success(),
        "{report}\n{}",
        String::from_utf8_lossy(&rerun.stderr)
    );
    // The filters matched: the tests did run.
    assert!(
        report.contains(&format!("test result: ok. {} passed", tests.len())),
        "{report}"
    );
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn backspace_erases_a_whole_utf8_character() {
    // The console sends UTF-8, and 0x7f for Backspace. Like a terminal
    // emulator, the session's terminal is in UTF-8 mode (IUTF8), so the line
    // discipline erases both bytes of `é`, not just the last one (which
    // would leave a lone 0xC3 in the line the program reads).
    let mut child = spawn_pty(&sh("read line; printf %s \"$line\" | od -An -tx1"), SIZE).unwrap();
    let output = Collector::start(&mut child);
    child.writer().write_all("a\u{e9}\x7f\r".as_bytes()).unwrap();
    let text = output.finish();
    assert!(child.wait().unwrap().status.success());
    assert!(text.ends_with(" 61\r\n"), "{text:?}");
}

#[test]
fn the_last_output_before_exit_is_complete() {
    let script = "i=0; while [ $i -lt 2000 ]; do echo \"line $i\"; i=$((i+1)); done; \
                  head -c 100000 /dev/zero | tr '\\0' x; printf END";
    for _ in 0..3 {
        let mut child = spawn_pty(&sh(script), SIZE).unwrap();
        let output = Collector::start(&mut child);
        let text = output.finish();
        assert!(child.wait().unwrap().status.success());
        assert!(
            text.starts_with("line 0\r\nline 1\r\n"),
            "{:?}",
            &text[..40.min(text.len())]
        );
        assert!(text.contains("line 1999\r\n"));
        assert_eq!(text.matches("\r\n").count(), 2000);
        assert_eq!(text.bytes().filter(|&byte| byte == b'x').count(), 100_000);
        assert!(
            text.ends_with("xEND"),
            "{:?}",
            &text[text.len().saturating_sub(40)..]
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn the_program_leads_its_own_session_with_the_terminal() {
    let mut child = spawn_pty(&sh("cat /proc/$$/stat"), SIZE).unwrap();
    let output = Collector::start(&mut child);
    let stat = output.finish();
    child.wait().unwrap();
    let pid: u32 = stat.split_whitespace().next().unwrap().parse().unwrap();
    // pid (comm) state ppid pgrp session tty_nr ...
    let fields: Vec<i64> = stat
        .rsplit_once(')')
        .unwrap()
        .1
        .split_whitespace()
        .skip(1)
        .take(4)
        .map(|field| field.parse().unwrap())
        .collect();
    let (pgrp, session, tty) = (fields[1], fields[2], fields[3]);
    assert_eq!(pid, child.pid());
    assert_eq!(pgrp, i64::from(pid), "pgid == pid");
    assert_eq!(session, i64::from(pid), "a session leader");
    assert_ne!(tty, 0, "the terminal is its controlling terminal");
}

#[cfg(target_os = "linux")]
#[test]
fn stop_kills_after_the_grace_period_and_takes_the_grandchild() {
    let mut child = spawn_pty(
        &sh("trap '' TERM; sleep 600 & echo \"grandchild $!\"; wait"),
        SIZE,
    )
    .unwrap();
    let output = Collector::start(&mut child);
    output.wait_for("grandchild ");
    output.wait_for("\r\n");
    let grandchild = number_after(&output.text(), "grandchild ");
    let stopped_at = Instant::now();
    child.stop();
    let exit = wait_within(&mut child, PATIENCE);
    let took = stopped_at.elapsed();
    assert!(exit.stopped);
    assert!(!exit.timed_out);
    // Both ignore SIGTERM, so SIGKILL ends them after the 2 s grace.
    assert_eq!(exit.status, ExitStatus::Signaled(9));
    assert!(
        took >= Duration::from_millis(1950) && took < Duration::from_millis(2500),
        "{took:?}"
    );
    assert!(is_gone(grandchild), "grandchild {grandchild} survived");
    output.finish();
}

#[test]
fn stop_ends_a_cooperative_program_at_once() {
    let mut child = spawn_pty(&sh("sleep 600"), SIZE).unwrap();
    let output = Collector::start(&mut child);
    let stopped_at = Instant::now();
    child.stop();
    let exit = wait_within(&mut child, PATIENCE);
    assert!(exit.stopped);
    assert_eq!(exit.status, ExitStatus::Signaled(15));
    assert!(stopped_at.elapsed() < Duration::from_secs(2));
    // Stopping again, or after the end, changes nothing.
    child.stop();
    child.kill();
    assert_eq!(child.wait().unwrap(), exit);
    output.finish();
}

#[test]
fn one_thread_can_wait_while_another_stops() {
    let mut child = spawn_pty(&sh("sleep 600"), SIZE).unwrap();
    let output = Collector::start(&mut child);
    let child = Arc::new(child);
    let waiter = {
        let child = Arc::clone(&child);
        thread::spawn(move || child.wait().unwrap())
    };
    thread::sleep(Duration::from_millis(100));
    assert!(!waiter.is_finished());
    child.stop();
    let exit = waiter.join().unwrap();
    assert!(exit.stopped);
    assert_eq!(child.try_wait().unwrap(), Some(exit));
    output.finish();
}

#[test]
fn kill_is_immediate() {
    let mut child = spawn_pty(&sh("trap '' TERM; sleep 600"), SIZE).unwrap();
    let output = Collector::start(&mut child);
    let killed_at = Instant::now();
    child.kill();
    let exit = wait_within(&mut child, PATIENCE);
    assert!(exit.stopped);
    assert_eq!(exit.status, ExitStatus::Signaled(9));
    assert!(killed_at.elapsed() < Duration::from_secs(1));
    output.finish();
}

#[cfg(target_os = "linux")]
#[test]
fn dropping_the_session_kills_the_tree() {
    let mut child = spawn_pty(&sh("sleep 600 & echo \"grandchild $!\"; wait"), SIZE).unwrap();
    let output = Collector::start(&mut child);
    output.wait_for("grandchild ");
    output.wait_for("\r\n");
    let grandchild = number_after(&output.text(), "grandchild ");
    let pid = i32::try_from(child.pid()).unwrap();
    drop(child);
    assert!(is_gone(pid), "the program {pid} survived");
    assert!(is_gone(grandchild), "grandchild {grandchild} survived");
    output.finish();
}

#[cfg(target_os = "linux")]
#[test]
fn leftovers_are_killed_when_the_program_exits() {
    let started = Instant::now();
    let mut child = spawn_pty(&sh("sleep 600 & echo \"grandchild $!\""), SIZE).unwrap();
    let output = Collector::start(&mut child);
    // The background sleep holds the terminal open; end-of-file comes only
    // because it is killed with the group when the shell exits.
    let text = output.finish();
    let exit = child.wait().unwrap();
    assert!(exit.status.success());
    assert!(!exit.stopped);
    assert!(started.elapsed() < Duration::from_secs(10));
    let grandchild = number_after(&text, "grandchild ");
    assert!(is_gone(grandchild), "grandchild {grandchild} survived");
}

#[cfg(target_os = "linux")]
#[test]
fn an_escaped_process_cannot_hold_the_output_open() {
    if !std::path::Path::new("/usr/bin/setsid").exists() {
        eprintln!("skipped: /usr/bin/setsid is not installed");
        return;
    }
    // The shell waits until the background process has left its session
    // (field 6 of its stat line, the session ID, is its own PID); otherwise
    // the group cleanup at the shell's exit could kill it first.
    // Only without a cgroup scope can a process escape the session (a
    // scope kills it when the program exits; tests/containment.rs).
    let mut command = sh("/usr/bin/setsid /bin/sleep 30 & p=$!; \
                          while [ \"$(cut -d' ' -f6 /proc/$p/stat)\" != \"$p\" ]; do sleep 0.01; done; \
                          echo \"escaped $p\"");
    command.containment(Containment::ProcessGroupOnly);
    let mut child = spawn_pty(&command, SIZE).unwrap();
    assert_eq!(child.containment(), ContainmentLevel::ProcessGroupOnly);
    let output = Collector::start(&mut child);
    let exit = child.wait().unwrap();
    assert!(exit.status.success());
    let ended = Instant::now();
    // The escaped sleep keeps the terminal open, so the reader gives up
    // after 2 s without output.
    let text = output.finish_within(Duration::from_secs(15));
    let waited = ended.elapsed();
    let escaped = number_after(&text, "escaped ");
    let still_running = !is_gone_now(escaped);
    kill_pid(escaped);
    assert!(
        still_running,
        "the escaped process should have outlived the session"
    );
    assert!(waited < Duration::from_secs(10), "{waited:?}");
    assert!(waited >= Duration::from_millis(1500), "{waited:?}");
}

#[test]
fn writes_fail_once_the_program_has_ended() {
    let mut child = spawn_pty(&sh("exit 0"), SIZE).unwrap();
    let output = Collector::start(&mut child);
    child.wait().unwrap();
    let error = child.writer().write(b"late\n").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::BrokenPipe);
    assert_eq!(child.writer().write(b"").unwrap(), 0);
    output.finish();
}

#[test]
fn resize_is_validated_and_harmless_after_the_end() {
    let mut child = spawn_pty(&sh("exit 0"), SIZE).unwrap();
    let output = Collector::start(&mut child);
    assert!(matches!(
        child.resize(PtySize { cols: 0, rows: 10 }),
        Err(ProcessError::InvalidPtySize { cols: 0, rows: 10 })
    ));
    child.wait().unwrap();
    child.resize(PtySize { cols: 90, rows: 20 }).unwrap();
    output.finish();
}

#[test]
fn the_reader_is_taken_once_and_wait_repeats() {
    let mut child = spawn_pty(&sh("sleep 0.3; exit 4"), SIZE).unwrap();
    assert!(child.take_reader().is_some());
    assert!(child.take_reader().is_none());
    assert_eq!(child.try_wait().unwrap(), None);
    let exit = child.wait().unwrap();
    assert_eq!(exit.status, ExitStatus::Exited(4));
    assert!(exit.duration >= Duration::from_millis(250), "{:?}", exit.duration);
    assert_eq!(child.wait().unwrap(), exit);
    assert_eq!(child.try_wait().unwrap(), Some(exit));
}

#[test]
fn bad_requests_are_refused_before_anything_starts() {
    assert!(matches!(
        spawn_pty(&sh("exit 0"), PtySize { cols: 80, rows: 0 }),
        Err(ProcessError::InvalidPtySize { cols: 80, rows: 0 })
    ));
    let mut with_input = sh("cat");
    with_input.stdin(Stdin::Bytes(b"x".to_vec()));
    assert!(matches!(
        spawn_pty(&with_input, SIZE),
        Err(ProcessError::InvalidCommand { .. })
    ));
    assert!(matches!(
        spawn_piped(&with_input),
        Err(ProcessError::InvalidCommand { .. })
    ));
    let missing = Command::new("/nonexistent/b2c-no-such-program", std::env::temp_dir()).unwrap();
    assert!(matches!(
        spawn_pty(&missing, SIZE),
        Err(ProcessError::Spawn { .. })
    ));
    assert!(matches!(spawn_piped(&missing), Err(ProcessError::Spawn { .. })));
    let token = CancelToken::new();
    token.cancel();
    let mut cancelled = sh("exit 0");
    cancelled.cancel_token(&token);
    assert!(matches!(
        spawn_pty(&cancelled, SIZE),
        Err(ProcessError::Cancelled)
    ));
}

#[test]
fn the_cancel_token_stops_the_session() {
    let token = CancelToken::new();
    let mut command = sh("sleep 600");
    command.cancel_token(&token);
    let mut child = spawn_pty(&command, SIZE).unwrap();
    let output = Collector::start(&mut child);
    token.cancel();
    let exit = wait_within(&mut child, PATIENCE);
    assert!(exit.stopped);
    assert!(!exit.timed_out);
    output.finish();
}

#[test]
fn the_timeout_stops_the_session() {
    let mut command = sh("sleep 600");
    command.limits(Limits {
        timeout: Some(Duration::from_millis(300)),
        grace: Some(Duration::from_millis(200)),
        ..Limits::default()
    });
    let mut child = spawn_pty(&command, SIZE).unwrap();
    let output = Collector::start(&mut child);
    let exit = wait_within(&mut child, PATIENCE);
    assert!(exit.timed_out);
    assert!(!exit.stopped);
    assert!(exit.duration >= Duration::from_millis(250), "{:?}", exit.duration);
    output.finish();
}

#[test]
fn a_grace_period_too_long_to_reach_never_forces_the_kill() {
    let limits = Limits {
        grace: Some(Duration::MAX),
        ..Limits::default()
    };
    // Stopped by the supervisor thread (the cancel token): SIGTERM ends a
    // cooperative program, and the session reports it.
    let token = CancelToken::new();
    let mut command = sh("sleep 600");
    command.limits(limits.clone()).cancel_token(&token);
    let mut child = spawn_pty(&command, SIZE).unwrap();
    let output = Collector::start(&mut child);
    token.cancel();
    let exit = wait_within(&mut child, PATIENCE);
    assert!(exit.stopped);
    assert_eq!(exit.status, ExitStatus::Signaled(15));
    output.finish();

    // Stopped by the caller: a program that ignores SIGTERM keeps running,
    // as the forced kill would come after the end of time; kill() ends it.
    let mut command = sh("trap '' TERM; echo ready; sleep 600");
    command.limits(limits);
    let mut child = spawn_pty(&command, SIZE).unwrap();
    let output = Collector::start(&mut child);
    output.wait_for("ready\r\n");
    child.stop();
    thread::sleep(Duration::from_millis(300));
    assert_eq!(child.try_wait().unwrap(), None);
    child.kill();
    let exit = wait_within(&mut child, PATIENCE);
    assert!(exit.stopped);
    assert_eq!(exit.status, ExitStatus::Signaled(9));
    output.finish();
}

#[cfg(target_os = "linux")]
#[test]
fn the_process_watchdog_stops_a_growing_tree() {
    let mut command = sh("for i in 1 2 3 4 5 6 7 8; do sleep 600 & done; wait");
    command.limits(Limits {
        processes: Some(4),
        ..Limits::default()
    });
    let mut child = spawn_pty(&command, SIZE).unwrap();
    let output = Collector::start(&mut child);
    let exit = wait_within(&mut child, PATIENCE);
    assert!(exit.too_many_processes);
    assert!(!exit.stopped && !exit.timed_out);
    output.finish();
}

#[test]
fn pipe_mode_streams_output_delivers_input_and_stops() {
    let mut child = spawn_piped(&sh(
        "if test -t 1; then echo tty; else echo no-tty; fi; read line; echo \"got:$line\"; \
         echo \"err:$line\" >&2; sleep 600",
    ))
    .unwrap();
    assert_eq!(child.mode(), IoMode::Pipes);
    // Resizing a pipe session does nothing.
    child.resize(PtySize { cols: 10, rows: 10 }).unwrap();
    let output = Collector::start(&mut child);
    output.wait_for("no-tty\n");
    child.writer().write_all(b"hi\n").unwrap();
    output.wait_for("got:hi\n");
    output.wait_for("err:hi\n");
    // Pipes do not echo input.
    assert!(!output.text().contains("hi\ngot"), "{:?}", output.text());
    let stopped_at = Instant::now();
    child.stop();
    let exit = wait_within(&mut child, PATIENCE);
    assert!(exit.stopped);
    assert_eq!(exit.status, ExitStatus::Signaled(15));
    assert!(stopped_at.elapsed() < Duration::from_secs(2));
    output.finish();
}

#[test]
fn pipe_mode_reports_the_exit_and_all_output() {
    let mut child = spawn_piped(&sh(
        "i=0; while [ $i -lt 2000 ]; do echo \"line $i\"; i=$((i+1)); done; printf END; exit 3",
    ))
    .unwrap();
    let output = Collector::start(&mut child);
    let text = output.finish();
    assert_eq!(child.wait().unwrap().status, ExitStatus::Exited(3));
    let mut expected = String::new();
    for i in 0..2000 {
        writeln!(expected, "line {i}").unwrap();
    }
    expected.push_str("END");
    assert_eq!(text, expected);
}

#[cfg(target_os = "linux")]
#[test]
fn pipe_mode_programs_lead_their_own_session_without_a_terminal() {
    let mut child = spawn_piped(&sh("cat /proc/$$/stat")).unwrap();
    let output = Collector::start(&mut child);
    let stat = output.finish();
    child.wait().unwrap();
    let fields: Vec<i64> = stat
        .rsplit_once(')')
        .unwrap()
        .1
        .split_whitespace()
        .skip(1)
        .take(4)
        .map(|field| field.parse().unwrap())
        .collect();
    assert_eq!(fields[1], i64::from(child.pid()));
    assert_eq!(fields[2], i64::from(child.pid()));
    assert_eq!(fields[3], 0, "no controlling terminal");
}

#[test]
fn pipe_mode_writes_fail_once_the_program_has_ended() {
    let mut child = spawn_piped(&sh("exit 0")).unwrap();
    let output = Collector::start(&mut child);
    child.wait().unwrap();
    let error = child.writer().write(b"late\n").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::BrokenPipe);
    output.finish();
}

#[test]
fn the_environment_is_exactly_the_commands() {
    let mut command = Command::new("/usr/bin/env", std::env::temp_dir()).unwrap();
    command.env("ONLY_THIS", "yes").env("TERM", "xterm-256color");
    let mut child = spawn_pty(&command, SIZE).unwrap();
    let output = Collector::start(&mut child);
    let text = output.finish();
    child.wait().unwrap();
    let mut lines: Vec<&str> = text.lines().map(str::trim_end).collect();
    lines.sort_unstable();
    assert_eq!(lines, ["ONLY_THIS=yes", "TERM=xterm-256color"]);
}
