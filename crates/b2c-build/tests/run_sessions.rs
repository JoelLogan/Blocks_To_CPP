//! Run sessions end to end (`docs/spec/07-toolchain-build-run.md` §7.6): real
//! programs in a pseudo-terminal (or with pipes), driven through
//! [`RunSessions`] the way the desktop backend drives them, with recording
//! sinks in place of the webview's channels.
//!
//! Most tests use `/bin/sh` and coreutils, so they need no compiler and run
//! on every Unix CI runner. The tests of exit decoding, sanitizer reports and
//! the cross-platform echo build a small C++ helper with g++; without g++
//! they are skipped, unless `B2C_REQUIRE_GXX` is set (as in CI). Windows CI
//! runs the g++ tests through `ConPTY` with MSYS2's g++.
//!
//! Every wait has a deadline, so a broken session fails a test instead of
//! hanging it.

// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
// Counting line breaks in test output needs no extra crate.
#![allow(clippy::naive_bytecount)]
// Most helpers serve the Linux tests only.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use b2c_build::{PtySize, RunEnvOptions, RunSessions, RunSpec, run_environment};
use b2c_ipc::dto::{Containment, Crash, ExitStatus, RunEvent, RunMode, SanitizerReport};
use b2c_ipc::{ByteSink, EventSink, RunId};
use b2c_toolchain::target::Platform;

/// The longest any test waits for something a program does.
const TIMEOUT: Duration = Duration::from_mins(1);

/// What a run sent on its two channels, in order.
#[derive(Debug, Clone)]
enum Entry {
    /// An output batch: when it arrived and how long it was.
    Batch { at: Instant, len: usize },
    /// A run event.
    Event(RunEvent),
}

#[derive(Debug, Default)]
struct State {
    log: Vec<Entry>,
    /// Every output byte received.
    output: Vec<u8>,
    /// The line breaks in `output`.
    newlines: u64,
    batches: u64,
    /// Keep only the line breaks and the last 64 KiB of output (floods).
    count_only: bool,
}

/// The webview's side of a run: records both channels in one log.
#[derive(Debug, Default)]
struct Console {
    state: Mutex<State>,
    changed: Condvar,
}

impl Console {
    fn counting_only() -> Arc<Self> {
        let console = Self::default();
        console.lock().count_only = true;
        Arc::new(console)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Waits until `done` holds, or fails the test with the output so far.
    fn wait_until(&self, what: &str, done: impl Fn(&State) -> bool) -> MutexGuard<'_, State> {
        let deadline = Instant::now() + TIMEOUT;
        let mut state = self.lock();
        while !done(&state) {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(
                !left.is_zero(),
                "timed out waiting for {what}; output so far: {:?}",
                String::from_utf8_lossy(&state.output)
            );
            state = self
                .changed
                .wait_timeout(state, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        state
    }

    fn wait_for_text(&self, needle: &str) {
        drop(self.wait_until(&format!("{needle:?}"), |state| {
            String::from_utf8_lossy(&state.output).contains(needle)
        }));
    }

    /// Waits for the exit event, checks the order of everything sent, and
    /// returns the exit event.
    fn wait_for_exit(&self) -> Exit {
        let state = self.wait_until("the exit event", |state| {
            state
                .log
                .iter()
                .any(|entry| matches!(entry, Entry::Event(RunEvent::Exit { .. })))
        });
        check_order(&state.log);
        match state.log.last() {
            Some(Entry::Event(RunEvent::Exit {
                after_seq,
                elapsed_ms,
                status,
                crash,
                sanitizer,
                message,
            })) => Exit {
                after_seq: *after_seq,
                elapsed: Duration::from_millis(*elapsed_ms),
                status: *status,
                crash: *crash,
                sanitizer: sanitizer.clone(),
                message: message.clone(),
            },
            other => panic!("the exit event is not last: {other:?}"),
        }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.lock().output).into_owned()
    }

    fn batches(&self) -> u64 {
        self.lock().batches
    }

    fn started(&self) -> RunEvent {
        match self.lock().log.first() {
            Some(Entry::Event(started @ RunEvent::Started { .. })) => started.clone(),
            other => panic!("the first message is not the started event: {other:?}"),
        }
    }

    fn skipped(&self) -> Vec<(u64, u64)> {
        self.lock()
            .log
            .iter()
            .filter_map(|entry| match entry {
                Entry::Event(RunEvent::Skipped { lines, after_seq }) => Some((*lines, *after_seq)),
                _ => None,
            })
            .collect()
    }

    fn batch_times(&self) -> Vec<Instant> {
        self.lock()
            .log
            .iter()
            .filter_map(|entry| match entry {
                Entry::Batch { at, .. } => Some(*at),
                Entry::Event(_) => None,
            })
            .collect()
    }
}

impl ByteSink for Console {
    fn send(&self, bytes: Vec<u8>) -> bool {
        let mut state = self.lock();
        assert!(
            !state
                .log
                .iter()
                .any(|entry| matches!(entry, Entry::Event(RunEvent::Exit { .. }))),
            "output after the exit event"
        );
        state.batches += 1;
        state.newlines += bytes.iter().filter(|&&byte| byte == b'\n').count() as u64;
        state.log.push(Entry::Batch {
            at: Instant::now(),
            len: bytes.len(),
        });
        state.output.extend_from_slice(&bytes);
        if state.count_only && state.output.len() > 128 * 1024 {
            let excess = state.output.len() - 64 * 1024;
            state.output.drain(..excess);
        }
        self.changed.notify_all();
        true
    }
}

impl EventSink<RunEvent> for Console {
    fn send(&self, event: RunEvent) -> bool {
        let mut state = self.lock();
        state.log.push(Entry::Event(event));
        self.changed.notify_all();
        true
    }
}

/// Started first; exit exactly once and last; batches numbered in order and
/// every event's `afterSeq` equal to the batches before it.
fn check_order(log: &[Entry]) {
    assert!(
        matches!(log.first(), Some(Entry::Event(RunEvent::Started { .. }))),
        "the started event comes first: {:?}",
        log.first()
    );
    let mut batches = 0;
    for (index, entry) in log.iter().enumerate() {
        match entry {
            Entry::Batch { len, .. } => {
                assert!(*len > 0, "an empty batch");
                batches += 1;
            }
            Entry::Event(RunEvent::Started { .. }) => assert_eq!(index, 0, "a second started event"),
            Entry::Event(RunEvent::Skipped { after_seq, lines }) => {
                assert_eq!(*after_seq, batches, "skipped {lines} lines after the wrong batch");
            }
            Entry::Event(RunEvent::Exit { after_seq, .. }) => {
                assert_eq!(index, log.len() - 1, "the exit event is not last");
                assert_eq!(
                    *after_seq, batches,
                    "the exit event does not follow the last batch"
                );
            }
        }
    }
}

/// The fields of the exit event.
#[derive(Debug, Clone)]
struct Exit {
    after_seq: u64,
    elapsed: Duration,
    status: ExitStatus,
    crash: Option<Crash>,
    sanitizer: Option<SanitizerReport>,
    message: String,
}

fn host_env(sanitizers: bool) -> Vec<(OsString, OsString)> {
    run_environment(
        std::env::vars_os(),
        &RunEnvOptions {
            platform: Platform::host(),
            sanitizers,
            leak_detection: false,
            toolchain_bin: None,
        },
    )
}

fn spec(program: &Path, args: &[&str]) -> RunSpec {
    RunSpec {
        project_key: String::from("project"),
        executable: program.to_path_buf(),
        args: args.iter().map(OsString::from).collect(),
        working_dir: std::env::temp_dir(),
        env: host_env(false),
        size: PtySize { cols: 80, rows: 24 },
        scrollback_lines: 10_000,
        ide_helpers: false,
        prefer_pty: true,
        hold: None,
    }
}

fn start(sessions: &RunSessions, spec: RunSpec) -> (RunId, Arc<Console>) {
    start_with(sessions, spec, Arc::new(Console::default()))
}

fn start_with(sessions: &RunSessions, spec: RunSpec, console: Arc<Console>) -> (RunId, Arc<Console>) {
    let output: Arc<dyn ByteSink> = console.clone();
    let events: Arc<dyn EventSink<RunEvent>> = console.clone();
    let id = sessions.start(spec, output, events).expect("the program starts");
    (id, console)
}

/// The g++-built helper program (see [`HELPER_SOURCE`]) in the given flavour,
/// or `None` (skip) when g++ is not installed.
fn helper(flavour: Flavour) -> Option<&'static Path> {
    static DIR: OnceLock<Option<tempfile::TempDir>> = OnceLock::new();
    static BUILT: [OnceLock<Option<PathBuf>>; 2] = [OnceLock::new(), OnceLock::new()];
    let dir = DIR
        .get_or_init(|| {
            if gxx_available() {
                Some(tempfile::tempdir().unwrap())
            } else {
                None
            }
        })
        .as_ref()?;
    BUILT[flavour as usize]
        .get_or_init(|| Some(build_helper(dir.path(), flavour)))
        .as_deref()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flavour {
    /// Optimised, no sanitizers: what a Release build runs.
    Release,
    /// With AddressSanitizer and UndefinedBehaviorSanitizer, as Debug builds
    /// on Linux have them.
    Sanitized,
}

/// A command for g++. Production code spawns processes only through
/// b2c-process; tests drive the compiler directly.
#[allow(clippy::disallowed_methods)]
fn gxx_command() -> std::process::Command {
    std::process::Command::new("g++")
}

fn gxx_available() -> bool {
    let found = gxx_command()
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success());
    assert!(
        found || std::env::var_os("B2C_REQUIRE_GXX").is_none(),
        "B2C_REQUIRE_GXX is set but g++ was not found on PATH"
    );
    found
}

const HELPER_SOURCE: &str = r#"
#include <climits>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <iostream>
#include <string>

static int deep(int n) {
    volatile char frame[1024];
    frame[0] = static_cast<char>(n);
    return n == 0 ? 0 : deep(n - 1) + frame[0];
}

int main(int argc, char** argv) {
    const char* what = argc > 1 ? argv[1] : "";
    if (std::strcmp(what, "echo") == 0) {
        std::string line;
        std::cout << "name? " << std::flush;
        std::getline(std::cin, line);
        std::cout << "hello " << line << std::endl;
        return 0;
    }
    if (std::strcmp(what, "null") == 0) {
        int* volatile pointer = nullptr;
        return *pointer;
    }
    if (std::strcmp(what, "div") == 0) {
        volatile int zero = 0;
        return 10 / zero;
    }
    if (std::strcmp(what, "abort") == 0) {
        std::abort();
    }
    if (std::strcmp(what, "exit") == 0) {
        return 42;
    }
    if (std::strcmp(what, "recurse") == 0) {
        return deep(INT_MAX / 2);
    }
    if (std::strcmp(what, "heap") == 0) {
        int* numbers = new int[4];
        volatile int index = 4;
        numbers[index] = 1;
        int first = numbers[0];
        delete[] numbers;
        return first;
    }
    if (std::strcmp(what, "overflow") == 0) {
        volatile int big = INT_MAX;
        big = big + argc;
        return big == 0;
    }
    return 0;
}
"#;

fn build_helper(dir: &Path, flavour: Flavour) -> PathBuf {
    let name = match flavour {
        Flavour::Release => "helper-release",
        Flavour::Sanitized => "helper-sanitized",
    };
    // One source file per flavour: tests build them in parallel.
    let source = dir.join(format!("{name}.cpp"));
    std::fs::write(&source, HELPER_SOURCE).unwrap();
    let executable = dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    let flags: &[&str] = match flavour {
        Flavour::Release => &["-O2"],
        Flavour::Sanitized => &[
            "-O0",
            "-g",
            "-fsanitize=address,undefined",
            "-fno-sanitize-recover=undefined",
        ],
    };
    // MSYS2's g++ links its runtime as DLLs unless told otherwise; the
    // project's builds link statically too.
    let static_runtime: &[&str] = if cfg!(windows) { &["-static"] } else { &[] };
    let output = gxx_command()
        .arg("-std=c++17")
        .args(flags)
        .args(static_runtime)
        .arg(&source)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "g++ failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    executable
}

/// Runs the release helper with `what` and returns how it ended.
fn run_helper(what: &str) -> Option<Exit> {
    let program = helper(Flavour::Release)?;
    let sessions = RunSessions::new();
    let (_, console) = start(&sessions, spec(program, &[what]));
    Some(console.wait_for_exit())
}

#[test]
fn an_echo_program_reads_a_line_through_the_terminal() {
    let Some(program) = helper(Flavour::Release) else {
        return;
    };
    let sessions = RunSessions::new();
    let mut spec = spec(program, &["echo"]);
    spec.ide_helpers = true;
    let (id, console) = start(&sessions, spec);
    // ConPTY repaints the screen and may draw the prompt's trailing space as
    // a cursor movement (ESC [ 1 C), so only the visible text is awaited.
    console.wait_for_text("name?");
    assert!(sessions.is_running("project"));
    // Enter is a carriage return in a terminal.
    sessions.input(&id, b"Ada\r").unwrap();
    let exit = console.wait_for_exit();
    assert!(console.text().contains("hello Ada"), "{:?}", console.text());
    assert_eq!(exit.status, ExitStatus::Exited { code: 0 });
    assert_eq!(exit.message, "Finished (exit code 0)");
    assert_eq!(exit.crash, None);
    let expected_containment = if cfg!(windows) {
        Containment::JobObject
    } else {
        Containment::ProcessGroupOnly
    };
    match console.started() {
        RunEvent::Started {
            containment,
            mode,
            ide_helpers,
        } => {
            assert_eq!(mode, RunMode::Pty);
            assert!(ide_helpers);
            // The cgroup scope (w2-containment) may report `cgroup` instead.
            assert!(containment == expected_containment || containment == Containment::Cgroup);
        }
        other => panic!("{other:?}"),
    }
    assert!(!sessions.is_running("project"));
}

#[test]
fn exits_are_decoded() {
    let Some(exit) = run_helper("exit") else {
        return;
    };
    assert_eq!(exit.status, ExitStatus::Exited { code: 42 });
    assert_eq!(exit.crash, None);
    assert_eq!(exit.message, "Finished with exit code 42");

    let null = run_helper("null").unwrap();
    assert_eq!(null.crash, Some(Crash::MemoryAccess));
    assert!(
        null.message
            .starts_with("Crashed: the program tried to use memory it doesn't own"),
        "{}",
        null.message
    );
    let division = run_helper("div").unwrap();
    assert_eq!(division.crash, Some(Crash::DivisionByZero));
    assert!(division.message.starts_with("Crashed: integer division by zero"));
    let abort = run_helper("abort").unwrap();
    assert_eq!(abort.crash, Some(Crash::Aborted));
    assert!(
        abort
            .message
            .starts_with("Stopped itself: an uncaught error or failed check")
    );
    for exit in [&null, &division, &abort] {
        assert_eq!(exit.sanitizer, None);
    }

    if cfg!(windows) {
        assert_eq!(
            null.status,
            ExitStatus::Exception {
                ntstatus: 0xC000_0005
            }
        );
        assert_eq!(
            division.status,
            ExitStatus::Exception {
                ntstatus: 0xC000_0094
            }
        );
        // The C runtime decides how abort() ends a program: the classic
        // exit code 3, or a fail-fast exception (0xC0000409, what UCRT does
        // when a debugger-less abort reports the fault). Both are an abort.
        assert!(
            matches!(
                abort.status,
                ExitStatus::Exited { code: 3 }
                    | ExitStatus::Exception {
                        ntstatus: 0xC000_0409
                    }
            ),
            "{:?}",
            abort.status
        );
        let recursion = run_helper("recurse").unwrap();
        assert_eq!(
            recursion.status,
            ExitStatus::Exception {
                ntstatus: 0xC000_00FD
            }
        );
        assert_eq!(recursion.crash, Some(Crash::StackOverflow));
        assert!(recursion.message.starts_with("Crashed: stack overflow"));
    } else {
        assert_eq!(null.status, ExitStatus::Signaled { signal: 11 });
        assert!(null.message.ends_with("(SIGSEGV)."));
        assert_eq!(division.status, ExitStatus::Signaled { signal: 8 });
        assert_eq!(division.message, "Crashed: integer division by zero (SIGFPE).");
        assert_eq!(abort.status, ExitStatus::Signaled { signal: 6 });
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::Read as _;

    use b2c_ipc::dto::SanitizerTool;
    use b2c_ipc::limits::{MAX_CONCURRENT_RUNS, MAX_RUN_INPUT_BYTES};
    use b2c_ipc::{InvalidReason, IpcError};

    use super::*;

    fn sh() -> &'static Path {
        Path::new("/bin/sh")
    }

    fn coreutil(name: &str) -> PathBuf {
        ["/usr/bin", "/bin"]
            .iter()
            .map(|dir| Path::new(dir).join(name))
            .find(|path| path.is_file())
            .unwrap_or_else(|| panic!("{name} is not installed"))
    }

    fn sh_spec(script: &str) -> RunSpec {
        spec(sh(), &["-c", script])
    }

    /// Whether the process `pid` is gone (or a zombie nobody reaped yet).
    fn is_gone(pid: u32) -> bool {
        let mut stat = String::new();
        match std::fs::File::open(format!("/proc/{pid}/stat")) {
            Err(_) => true,
            Ok(mut file) => {
                file.read_to_string(&mut stat).unwrap_or_default();
                // The state follows the command name in brackets.
                stat.rsplit_once(')')
                    .is_some_and(|(_, rest)| rest.trim_start().starts_with(['Z', 'X']))
            }
        }
    }

    fn wait_gone(pid: u32) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !is_gone(pid) {
            assert!(Instant::now() < deadline, "process {pid} is still running");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The `n`-th whitespace-separated number after `label` in `text`.
    fn number_after(text: &str, label: &str) -> u32 {
        let start = text.find(label).unwrap() + label.len();
        text[start..]
            .split(|c: char| !c.is_ascii_digit())
            .find(|part| !part.is_empty())
            .unwrap()
            .parse()
            .unwrap()
    }

    #[test]
    fn the_program_sees_a_terminal_of_the_requested_size() {
        let sessions = RunSessions::new();
        let mut spec = sh_spec(
            "if [ -t 0 ] && [ -t 1 ] && [ -t 2 ]; then echo on-a-tty; fi; echo \"size $(stty size)\"; \
             read line; echo \"size $(stty size)\"",
        );
        spec.size = PtySize { cols: 100, rows: 30 };
        let (id, console) = start(&sessions, spec);
        console.wait_for_text("size 30 100");
        assert!(console.text().contains("on-a-tty"));
        sessions.resize(&id, 120, 40).unwrap();
        sessions.input(&id, b"\r").unwrap();
        console.wait_for_text("size 40 120");
        let exit = console.wait_for_exit();
        assert_eq!(exit.status, ExitStatus::Exited { code: 0 });
        assert!(
            console.text().contains("\r\n"),
            "a terminal ends lines with \\r\\n"
        );
    }

    #[test]
    fn pipes_are_the_fallback_mode() {
        let sessions = RunSessions::new();
        let mut spec =
            sh_spec("if [ -t 1 ]; then echo tty; else echo pipe; fi; read line; echo \"got $line\"");
        spec.prefer_pty = false;
        let (id, console) = start(&sessions, spec);
        console.wait_for_text("pipe\n");
        // Resizing does nothing with pipes, but is no error while running.
        sessions.resize(&id, 90, 20).unwrap();
        sessions.input(&id, b"words\n").unwrap();
        let exit = console.wait_for_exit();
        assert_eq!(console.text(), "pipe\ngot words\n");
        assert_eq!(exit.status, ExitStatus::Exited { code: 0 });
        assert!(matches!(
            console.started(),
            RunEvent::Started {
                mode: RunMode::Pipes,
                ide_helpers: false,
                ..
            }
        ));
    }

    #[test]
    fn stop_ends_the_program_and_its_child() {
        let sessions = RunSessions::new();
        let (id, console) = start(&sessions, sh_spec("sleep 600 & echo \"child $!\"; wait"));
        console.wait_for_text("child ");
        let child = number_after(&console.text(), "child ");
        let stopped_at = Instant::now();
        sessions.stop(&id).unwrap();
        let exit = console.wait_for_exit();
        assert!(stopped_at.elapsed() < Duration::from_secs(5));
        assert_eq!(exit.status, ExitStatus::Stopped);
        assert_eq!(exit.crash, None);
        assert_eq!(exit.message, "Stopped");
        wait_gone(child);
        assert!(!sessions.is_running("project"));
    }

    #[test]
    fn a_program_ignoring_the_polite_stop_is_killed_after_two_seconds() {
        let sessions = RunSessions::new();
        let (id, console) = start(
            &sessions,
            sh_spec("trap '' TERM; sleep 600 & echo \"child $!\"; while :; do wait; done"),
        );
        console.wait_for_text("child ");
        let child = number_after(&console.text(), "child ");
        let stopped_at = Instant::now();
        sessions.stop(&id).unwrap();
        let exit = console.wait_for_exit();
        let took = stopped_at.elapsed();
        assert!(took >= Duration::from_secs(2), "killed after {took:?}");
        assert!(took < Duration::from_secs(6), "killed after {took:?}");
        assert_eq!(exit.status, ExitStatus::Stopped);
        wait_gone(child);
    }

    /// The resident memory of this test process, in bytes.
    fn rss() -> u64 {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        let line = status.lines().find(|line| line.starts_with("VmRSS:")).unwrap();
        u64::from(number_after(line, "VmRSS:")) * 1024
    }

    #[test]
    fn ten_million_lines_without_acknowledgements_stay_bounded() {
        let sessions = RunSessions::new();
        let console = Console::counting_only();
        let baseline = rss();
        let (_, console) = start_with(&sessions, spec(&coreutil("seq"), &["10000000"]), console);
        let mut peak = baseline;
        let exit = loop {
            peak = peak.max(rss());
            let done = console
                .lock()
                .log
                .iter()
                .any(|entry| matches!(entry, Entry::Event(RunEvent::Exit { .. })));
            if done {
                break console.wait_for_exit();
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(exit.status, ExitStatus::Exited { code: 0 }, "{}", exit.message);
        let skipped = console.skipped();
        assert!(
            !skipped.is_empty(),
            "a console that never acknowledges falls behind"
        );
        let skipped_lines: u64 = skipped.iter().map(|(lines, _)| lines).sum();
        let state = console.lock();
        assert_eq!(
            state.newlines + skipped_lines,
            10_000_000,
            "every line is delivered or counted"
        );
        assert!(state.output.ends_with(b"9999999\r\n10000000\r\n"));
        // At most 4 MiB went before the console counted as behind, then the
        // last 10,000 lines.
        let delivered: usize = state
            .log
            .iter()
            .map(|entry| match entry {
                Entry::Batch { len, .. } => *len,
                Entry::Event(_) => 0,
            })
            .sum();
        assert!(delivered < 6 * 1024 * 1024, "{delivered} bytes delivered");
        let growth = peak.saturating_sub(baseline);
        assert!(growth < 64 * 1024 * 1024, "memory grew by {growth} bytes");
    }

    #[test]
    fn an_acknowledging_console_gets_everything_or_exact_skip_counts() {
        let sessions = Arc::new(RunSessions::new());
        let console = Console::counting_only();
        let (id, console) = start_with(&sessions, spec(&coreutil("seq"), &["2000000"]), console);
        // The console acknowledges what it has written every 100 ms.
        let acker = {
            let sessions = Arc::clone(&sessions);
            let console = Arc::clone(&console);
            let id = id.clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_millis(100));
                    let batches = console.batches();
                    if sessions.ack(&id, batches).is_err() || !sessions.is_running("project") {
                        return;
                    }
                }
            })
        };
        let exit = console.wait_for_exit();
        acker.join().unwrap();
        assert_eq!(exit.status, ExitStatus::Exited { code: 0 });
        let skipped: u64 = console.skipped().iter().map(|(lines, _)| lines).sum();
        let state = console.lock();
        assert_eq!(state.newlines + skipped, 2_000_000);
        assert!(state.output.ends_with(b"2000000\r\n"));
        drop(state);
        // Late acknowledgements of the last batches are fine; batches never
        // sent cannot be acknowledged.
        sessions.ack(&id, exit.after_seq).unwrap();
        assert_eq!(
            sessions.ack(&id, exit.after_seq + 1).unwrap_err(),
            IpcError::invalid(InvalidReason::OutOfRange, Some("seq"))
        );
    }

    #[test]
    fn batches_are_at_least_an_interval_apart() {
        let sessions = RunSessions::new();
        let (_, console) = start(
            &sessions,
            sh_spec("i=0; while [ $i -lt 100 ]; do echo \"line $i\"; i=$((i+1)); sleep 0.005; done"),
        );
        let exit = console.wait_for_exit();
        assert_eq!(exit.status, ExitStatus::Exited { code: 0 });
        let times = console.batch_times();
        assert!(times.len() > 5, "{} batches", times.len());
        // Batches go at least 16 ms apart (the last one, sent when the output
        // ends, may come early); timestamps taken in the sink can be off by a
        // little, so count instead of measuring each gap.
        let span = times[times.len() - 1] - times[0];
        let most = span.as_millis() / 16 + 3;
        assert!(times.len() as u128 <= most, "{} batches in {span:?}", times.len());
        assert!(console.text().contains("line 99\r\n"));
    }

    #[test]
    fn input_is_limited_in_size_and_rate() {
        let sessions = RunSessions::new();
        let mut spec = sh_spec("cat >/dev/null");
        spec.prefer_pty = false;
        let (id, console) = start(&sessions, spec);
        assert_eq!(
            sessions
                .input(&id, &vec![b'x'; MAX_RUN_INPUT_BYTES + 1])
                .unwrap_err(),
            IpcError::too_large(MAX_RUN_INPUT_BYTES)
        );
        // 1 MiB a second: sixteen calls of 64 KiB, then no more.
        for call in 0..16 {
            sessions
                .input(&id, &vec![b'x'; MAX_RUN_INPUT_BYTES])
                .unwrap_or_else(|error| panic!("call {call}: {error:?}"));
        }
        assert_eq!(
            sessions.input(&id, &vec![b'x'; MAX_RUN_INPUT_BYTES]).unwrap_err(),
            IpcError::RateLimited
        );
        sessions.stop(&id).unwrap();
        console.wait_for_exit();

        // 200 calls a second: a burst of calls is cut off at that (or by the
        // queue of calls waiting for the program).
        let mut spec = sh_spec("cat >/dev/null");
        spec.prefer_pty = false;
        let (id, console) = start(&sessions, spec);
        let began = Instant::now();
        let mut accepted = 0u32;
        let mut refused = 0u32;
        for _ in 0..1000 {
            match sessions.input(&id, b"k") {
                Ok(()) => accepted += 1,
                Err(IpcError::RateLimited) => refused += 1,
                Err(other) => panic!("{other:?}"),
            }
        }
        // 200 calls a second is one every 5 ms.
        let earned = u32::try_from(began.elapsed().as_millis() / 5 + 1).unwrap_or(u32::MAX);
        assert!(refused > 0);
        assert!(accepted >= 64, "{accepted} calls accepted");
        assert!(
            accepted <= 200 + earned,
            "{accepted} calls accepted, {earned} earned"
        );
        sessions.stop(&id).unwrap();
        console.wait_for_exit();
    }

    #[test]
    fn unknown_and_finished_runs_give_typed_errors() {
        let sessions = RunSessions::new();
        let (id, console) = start(&sessions, sh_spec("echo bye; exit 3"));
        let exit = console.wait_for_exit();
        assert_eq!(exit.status, ExitStatus::Exited { code: 3 });
        assert_eq!(exit.message, "Finished with exit code 3");
        assert!(exit.elapsed < TIMEOUT);
        assert_eq!(sessions.input(&id, b"x").unwrap_err(), IpcError::NotRunning);
        assert_eq!(sessions.resize(&id, 80, 24).unwrap_err(), IpcError::NotRunning);
        assert_eq!(sessions.stop(&id).unwrap_err(), IpcError::NotRunning);
        sessions.ack(&id, exit.after_seq).unwrap();
        assert!(!sessions.is_running("project"));

        let unknown = RunId::random().unwrap();
        assert_eq!(sessions.input(&unknown, b"x").unwrap_err(), IpcError::UnknownRun);
        assert_eq!(
            sessions.resize(&unknown, 80, 24).unwrap_err(),
            IpcError::UnknownRun
        );
        assert_eq!(sessions.stop(&unknown).unwrap_err(), IpcError::UnknownRun);
        assert_eq!(sessions.ack(&unknown, 0).unwrap_err(), IpcError::UnknownRun);
    }

    #[test]
    fn the_environment_and_working_folder() {
        let folder = tempfile::tempdir().unwrap();
        let folder_path = folder.path().canonicalize().unwrap();
        let host: Vec<(OsString, OsString)> = [
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/home/ada"),
            ("B2C_FOO", "internal"),
            ("TAURI_ENV_DEBUG", "true"),
            ("WEBKIT_DISABLE_COMPOSITING_MODE", "1"),
            ("APPIMAGE", "/opt/b2c.AppImage"),
            ("TERM", "dumb"),
        ]
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .into();
        for sanitizers in [false, true] {
            let sessions = RunSessions::new();
            let mut spec = sh_spec("env; echo \"cwd=$(pwd -P)\"");
            spec.working_dir = folder_path.clone();
            spec.env = run_environment(
                host.clone(),
                &RunEnvOptions {
                    platform: Platform::Linux,
                    sanitizers,
                    leak_detection: true,
                    toolchain_bin: None,
                },
            );
            let (_, console) = start(&sessions, spec);
            console.wait_for_exit();
            let text = console.text().replace('\r', "");
            let lines: Vec<&str> = text.lines().collect();
            assert!(lines.contains(&"TERM=xterm-256color"), "{text}");
            assert!(lines.contains(&"HOME=/home/ada"));
            assert!(
                lines.contains(&format!("cwd={}", folder_path.display()).as_str()),
                "{text}"
            );
            for internal in [
                "B2C_FOO",
                "TAURI_ENV_DEBUG",
                "WEBKIT_DISABLE_COMPOSITING_MODE",
                "APPIMAGE",
                "B2C_EVENTS",
            ] {
                assert!(
                    !text.contains(&format!("{internal}=")),
                    "{internal} reached the program"
                );
            }
            assert_eq!(
                lines.contains(&"ASAN_OPTIONS=halt_on_error=1:detect_leaks=1"),
                sanitizers
            );
            assert_eq!(
                lines.contains(&"UBSAN_OPTIONS=print_stacktrace=1:halt_on_error=1"),
                sanitizers
            );
        }
    }

    #[test]
    fn sanitizer_reports_become_the_exit_message() {
        let Some(program) = helper(Flavour::Sanitized) else {
            return;
        };
        let sessions = RunSessions::new();
        let mut heap = spec(program, &["heap"]);
        heap.env = host_env(true);
        let (_, console) = start(&sessions, heap);
        let exit = console.wait_for_exit();
        assert_eq!(exit.status, ExitStatus::Exited { code: 1 }, "{}", console.text());
        assert_eq!(exit.message, "Crashed: heap-buffer-overflow (AddressSanitizer)");
        let report = exit.sanitizer.unwrap();
        assert_eq!(report.tool, SanitizerTool::Address);
        assert_eq!(report.kind.as_str(), "heap-buffer-overflow");

        let mut overflow = spec(program, &["overflow"]);
        overflow.env = host_env(true);
        let (_, console) = start(&sessions, overflow);
        let exit = console.wait_for_exit();
        assert_eq!(
            exit.message,
            "Crashed: signed-integer-overflow (UndefinedBehaviorSanitizer)",
            "{}",
            console.text()
        );
        assert_eq!(exit.sanitizer.unwrap().tool, SanitizerTool::Undefined);
    }

    #[test]
    fn one_run_per_project_and_at_most_eight_in_all() {
        let sessions = RunSessions::new();
        let mut consoles = Vec::new();
        for project in 0..MAX_CONCURRENT_RUNS {
            let mut spec = sh_spec("echo up; exec sleep 600");
            spec.project_key = format!("p{project}");
            let (_, console) = start(&sessions, spec);
            console.wait_for_text("up");
            consoles.push(console);
        }
        let mut ninth = sh_spec("exec sleep 600");
        ninth.project_key = String::from("p-extra");
        let console = Arc::new(Console::default());
        let output: Arc<dyn ByteSink> = console.clone();
        let events: Arc<dyn EventSink<RunEvent>> = console.clone();
        assert_eq!(
            sessions.start(ninth, output, events).unwrap_err(),
            IpcError::TooManySessions
        );
        assert!(console.lock().log.is_empty(), "a refused run sends nothing");

        // Starting p0 again replaces its run, which is stopped.
        let mut again = sh_spec("echo again; exec sleep 600");
        again.project_key = String::from("p0");
        let (_, replacement) = start(&sessions, again);
        assert_eq!(consoles[0].wait_for_exit().status, ExitStatus::Stopped);
        replacement.wait_for_text("again");
        assert!(sessions.is_running("p0"));

        sessions.stop_project("p1");
        assert_eq!(consoles[1].wait_for_exit().status, ExitStatus::Stopped);
        assert!(!sessions.is_running("p1"));

        let began = Instant::now();
        sessions.stop_all();
        assert!(began.elapsed() < Duration::from_secs(4));
        for project in 0..MAX_CONCURRENT_RUNS {
            assert!(!sessions.is_running(&format!("p{project}")));
        }
        for console in consoles.iter().skip(2).chain([&replacement]) {
            assert_eq!(console.wait_for_exit().status, ExitStatus::Stopped);
        }
    }

    #[test]
    fn dropping_the_sessions_kills_the_programs() {
        let sessions = RunSessions::new();
        let (_, console) = start(&sessions, sh_spec("echo \"pid $$\"; exec sleep 600"));
        console.wait_for_text("pid ");
        let pid = number_after(&console.text(), "pid ");
        drop(sessions);
        let exit = console.wait_for_exit();
        assert_eq!(exit.status, ExitStatus::Stopped);
        wait_gone(pid);
    }

    #[test]
    fn a_closed_output_channel_does_not_stop_the_program() {
        /// An output channel whose window has gone.
        struct Gone;
        impl ByteSink for Gone {
            fn send(&self, _: Vec<u8>) -> bool {
                false
            }
        }
        let sessions = RunSessions::new();
        let events = Arc::new(Console::default());
        let events_sink: Arc<dyn EventSink<RunEvent>> = events.clone();
        sessions
            .start(sh_spec("seq 1 1000; echo done"), Arc::new(Gone), events_sink)
            .unwrap();
        let exit = events.wait_until("the exit event", |state| {
            state
                .log
                .iter()
                .any(|entry| matches!(entry, Entry::Event(RunEvent::Exit { .. })))
        });
        match exit.log.last() {
            Some(Entry::Event(RunEvent::Exit {
                status, after_seq, ..
            })) => {
                assert_eq!(*status, ExitStatus::Exited { code: 0 });
                assert!(*after_seq >= 1);
            }
            other => panic!("{other:?}"),
        }
    }
}
