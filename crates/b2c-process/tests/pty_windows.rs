//! Sessions on Windows (run on the windows-2025 runner in CI; `ConPTY` works
//! headless there): an echo round trip through the pseudo console, resizing,
//! Ctrl+C, the Job Object on stop and on drop, the explicit handle list and
//! input of pipe mode, and complete output.
#![cfg(windows)]
// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// One test makes a handle inheritable in this process, which needs a Win32
// call (each block has a SAFETY comment).
#![allow(unsafe_code)]

use std::fmt::Write as _;
use std::io::{ErrorKind, Read, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use b2c_process::{
    Command, ContainmentLevel, Crash, ExitStatus, IoMode, ProcessError, PtyChild, PtyExit, PtySize,
    spawn_piped, spawn_pty,
};

const SIZE: PtySize = PtySize { cols: 100, rows: 30 };
/// Generous bound for things that should take a moment, for loaded CI
/// runners.
const PATIENCE: Duration = Duration::from_secs(30);

fn system32() -> PathBuf {
    PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32")
}

/// A System32 program with only `SystemRoot` and a `PATH` of System32 in
/// its environment.
fn system_program(name: &str, args: &[&str]) -> Command {
    let mut command = Command::new(system32().join(name), std::env::temp_dir()).unwrap();
    command
        .args(args)
        .env("SystemRoot", std::env::var_os("SystemRoot").unwrap())
        .env("PATH", system32());
    command
}

/// `cmd.exe /d /c <script>`.
fn cmd(script: &str) -> Command {
    system_program("cmd.exe", &["/d", "/c", script])
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

    fn finish(&self) -> String {
        let until = Instant::now() + PATIENCE;
        while !self.eof.load(Ordering::SeqCst) {
            assert!(
                Instant::now() < until,
                "no end-of-file; output so far: {:?}",
                self.text()
            );
            thread::sleep(Duration::from_millis(10));
        }
        self.text()
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

/// The number after `label` in `text` (the console may put escape
/// sequences and spaces in between).
fn number_after(text: &str, label: &str) -> Option<u32> {
    let start = text.rfind(label)? + label.len();
    let digits: String = text[start..]
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

fn wait_for_file(path: &Path) {
    let until = Instant::now() + PATIENCE;
    while !path.exists() {
        assert!(Instant::now() < until, "{} never appeared", path.display());
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn an_echo_round_trip_through_the_pseudo_console() {
    let mut child = spawn_pty(
        &system_program("cmd.exe", &["/d", "/v:on", "/c", "set /p line=& echo got:!line!"]),
        SIZE,
    )
    .unwrap();
    assert_eq!(child.mode(), IoMode::Pty);
    assert_eq!(child.containment(), ContainmentLevel::JobObject);
    let output = Collector::start(&mut child);
    child.writer().write_all(b"hello\r").unwrap();
    output.wait_for("got:hello");
    let exit = wait_within(&mut child, PATIENCE);
    assert_eq!(exit.status, ExitStatus::Exited(0));
    assert!(!exit.stopped);
    output.finish();
}

#[test]
fn the_console_size_follows_a_resize() {
    let mut child = spawn_pty(&cmd("set /p line=& mode con"), SIZE).unwrap();
    let output = Collector::start(&mut child);
    child.resize(PtySize { cols: 120, rows: 40 }).unwrap();
    child.writer().write_all(b"\r").unwrap();
    output.wait_for("Columns");
    let text = output.finish();
    wait_within(&mut child, PATIENCE);
    assert_eq!(number_after(&text, "Columns:"), Some(120), "{text:?}");
    assert_eq!(number_after(&text, "Lines:"), Some(40), "{text:?}");
}

#[test]
fn ctrl_c_ends_a_console_program() {
    let mut child = spawn_pty(&system_program("findstr.exe", &["unlikely-text"]), SIZE).unwrap();
    let output = Collector::start(&mut child);
    // Give the program time to attach to the console and wait for input.
    thread::sleep(Duration::from_millis(500));
    child.writer().write_all(&[0x03]).unwrap();
    let exit = wait_within(&mut child, PATIENCE);
    assert!(!exit.stopped);
    // The default handler ends the program with STATUS_CONTROL_C_EXIT; a
    // program with its own handler may exit with a code of its own.
    assert!(
        exit.status.crash() == Some(Crash::Interrupted) || !exit.status.success(),
        "{:?}",
        exit.status
    );
    output.finish();
}

/// Starts `cmd` running a script that starts a grandchild which writes
/// `started.txt` at once and `marker.txt` after about 3 s, then waits.
fn session_with_grandchild(dir: &Path) -> PtyChild {
    std::fs::write(
        dir.join("child.cmd"),
        "@echo off\r\necho started> \"%~dp0started.txt\"\r\nping -n 4 127.0.0.1 > nul\r\necho alive> \"%~dp0marker.txt\"\r\n",
    )
    .unwrap();
    std::fs::write(
        dir.join("parent.cmd"),
        "@echo off\r\nstart \"\" /b cmd /d /c \"%~dp0child.cmd\"\r\nping -n 600 127.0.0.1 > nul\r\n",
    )
    .unwrap();
    let script = dir.join("parent.cmd");
    let mut command = system_program("cmd.exe", &["/d", "/c"]);
    command.arg(&script);
    let child = spawn_pty(&command, SIZE).unwrap();
    wait_for_file(&dir.join("started.txt"));
    child
}

#[test]
fn stop_kills_the_grandchild_with_the_job() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = session_with_grandchild(dir.path());
    let output = Collector::start(&mut child);
    child.stop();
    let exit = wait_within(&mut child, PATIENCE);
    assert!(exit.stopped);
    output.finish();
    thread::sleep(Duration::from_secs(6));
    assert!(
        !dir.path().join("marker.txt").exists(),
        "the grandchild survived the stop"
    );
}

#[test]
fn dropping_the_session_kills_the_grandchild() {
    let dir = tempfile::tempdir().unwrap();
    let child = session_with_grandchild(dir.path());
    drop(child);
    thread::sleep(Duration::from_secs(6));
    assert!(
        !dir.path().join("marker.txt").exists(),
        "the grandchild survived the drop"
    );
}

#[test]
fn pipe_children_do_not_inherit_other_handles() {
    use std::os::windows::io::{AsRawHandle as _, OwnedHandle};
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};

    let (mut reader, writer) = std::io::pipe().unwrap();
    let writer = OwnedHandle::from(writer);
    // SAFETY: `writer` is a valid handle owned by this test.
    let ok =
        unsafe { SetHandleInformation(writer.as_raw_handle(), HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) };
    assert_ne!(ok, 0, "SetHandleInformation failed");
    // ping runs for about 5 s; if it inherited the write end, reading the
    // pipe would block until it exits.
    let mut child = spawn_piped(&system_program("ping.exe", &["-n", "6", "127.0.0.1"])).unwrap();
    let output = Collector::start(&mut child);
    drop(writer);
    let started = Instant::now();
    let mut rest = Vec::new();
    reader.read_to_end(&mut rest).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the child held an inherited handle: {:?}",
        started.elapsed()
    );
    assert_eq!(
        child.try_wait().unwrap(),
        None,
        "the child should still be running"
    );
    child.stop();
    let exit = wait_within(&mut child, PATIENCE);
    assert!(exit.stopped);
    output.finish();
}

#[test]
fn output_printed_before_exit_is_complete() {
    let script = "for /l %i in (1,1,2000) do @echo line %i";
    let mut piped = spawn_piped(&cmd(script)).unwrap();
    let output = Collector::start(&mut piped);
    let text = output.finish();
    assert!(wait_within(&mut piped, PATIENCE).status.success());
    let mut expected = String::new();
    for i in 1..=2000 {
        write!(expected, "line {i}\r\n").unwrap();
    }
    assert_eq!(text, expected);

    let mut console = spawn_pty(&cmd(&format!("{script}& echo END-OF-OUTPUT")), SIZE).unwrap();
    let output = Collector::start(&mut console);
    let text = output.finish();
    assert!(wait_within(&mut console, PATIENCE).status.success());
    assert!(text.contains("line 2000"), "{text:?}");
    assert!(text.contains("END-OF-OUTPUT"), "{text:?}");
}

#[test]
fn exit_codes_and_stop_are_reported() {
    let mut child = spawn_pty(&cmd("exit 7"), SIZE).unwrap();
    let output = Collector::start(&mut child);
    assert_eq!(wait_within(&mut child, PATIENCE).status, ExitStatus::Exited(7));
    output.finish();

    let mut child = spawn_piped(&system_program("ping.exe", &["-n", "600", "127.0.0.1"])).unwrap();
    assert_eq!(child.mode(), IoMode::Pipes);
    let output = Collector::start(&mut child);
    let stopped_at = Instant::now();
    child.stop();
    let exit = wait_within(&mut child, PATIENCE);
    assert!(exit.stopped);
    assert!(stopped_at.elapsed() < Duration::from_secs(5));
    // Input after the end is refused.
    let error = child.writer().write(b"late").unwrap_err();
    assert_eq!(error.kind(), ErrorKind::BrokenPipe);
    output.finish();
}

#[test]
fn pipe_mode_delivers_input() {
    // The program must answer while it runs: a session cannot end its input
    // yet (that comes with M3), so a program that only writes at the end of
    // its input would never answer. findstr, for example, buffers its output
    // when it goes to a pipe. cmd.exe does no such buffering: `set /p` reads
    // one line from standard input and `echo` writes the answer straight to
    // the pipe, then cmd exits. The builder quotes the script as one argument
    // without escapes, `"set /p line=& echo got !line!"`, and since `&`
    // stands between its two quotes, cmd /c drops just those two quotes and
    // runs the text as written (the same script as the pseudo-console test
    // above; see also the command-line unit tests).
    let mut child = spawn_piped(&system_program(
        "cmd.exe",
        &["/d", "/v:on", "/c", "set /p line=& echo got !line!"],
    ))
    .unwrap();
    assert_eq!(child.mode(), IoMode::Pipes);
    // The line waits in the pipe until `set /p` reads it.
    child.writer().write_all(b"hello\r\n").unwrap();
    let output = Collector::start(&mut child);
    output.wait_for("got hello");
    let exit = wait_within(&mut child, PATIENCE);
    assert_eq!(exit.status, ExitStatus::Exited(0));
    assert!(!exit.stopped);
    output.finish();
}

#[test]
fn commands_windows_cannot_represent_are_refused() {
    let mut command = cmd("echo x");
    command.arg("a\0b");
    assert!(matches!(
        spawn_piped(&command),
        Err(ProcessError::InvalidCommand { .. })
    ));
    assert!(matches!(
        spawn_pty(&command, SIZE),
        Err(ProcessError::InvalidCommand { .. })
    ));
    let mut long = cmd("echo x");
    long.arg("x".repeat(40_000));
    assert!(matches!(
        spawn_piped(&long),
        Err(ProcessError::InvalidCommand { .. })
    ));
    assert!(matches!(
        spawn_pty(&cmd("echo x"), PtySize { cols: 0, rows: 1 }),
        Err(ProcessError::InvalidPtySize { .. })
    ));
}
