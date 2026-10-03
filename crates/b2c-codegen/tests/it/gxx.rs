//! Compiling and running generated C++ with the system g++.
//!
//! Tests that need g++ print a note and skip when it is not on `PATH`, unless
//! the environment variable `B2C_REQUIRE_GXX` is set (CI sets it), in which
//! case a missing g++ fails the test.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use b2c_ir::source_map::{FileKind, GeneratedProject};
use tempfile::TempDir;

/// Warning flags every generated file must compile cleanly with.
pub(crate) const WARNINGS: &[&str] = &[
    "-Wall",
    "-Wextra",
    "-Wpedantic",
    "-Werror",
    "-Wshadow",
    "-Wconversion",
    "-Wbidi-chars=any",
];

/// How long a test program may run.
const RUN_TIMEOUT: Duration = Duration::from_secs(20);

/// A command for an external program. Production code spawns processes only
/// through b2c-process; tests drive g++ and the built programs directly.
#[allow(clippy::disallowed_methods)]
fn command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    Command::new(program)
}

/// Whether g++ is available.
pub(crate) fn available() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| {
        let found = command("g++")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !found {
            assert!(
                std::env::var_os("B2C_REQUIRE_GXX").is_none(),
                "B2C_REQUIRE_GXX is set but g++ was not found on PATH"
            );
            eprintln!("note: g++ was not found on PATH; skipping the tests that compile generated C++");
        }
        found
    })
}

/// Writes the generated files into a fresh temporary directory.
pub(crate) fn write_files(project: &GeneratedProject) -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    for file in &project.files {
        std::fs::write(dir.path().join(&file.path), &file.contents).unwrap();
    }
    dir
}

/// Runs g++ with the given arguments and panics with its messages on failure.
fn gxx(args: &[&std::ffi::OsStr], what: &str, source: &str) {
    let output = command("g++").args(args).output().unwrap();
    assert!(
        output.status.success(),
        "g++ rejected {what}:\n{}\n--- source ---\n{source}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Checks that every source file of a project compiles without warnings.
pub(crate) fn syntax_check(project: &GeneratedProject, standard: &str) {
    if !available() {
        return;
    }
    let dir = write_files(project);
    let std_flag = format!("-std={standard}");
    for file in project.files.iter().filter(|f| f.kind == FileKind::Source) {
        let path = dir.path().join(&file.path);
        let mut args: Vec<&std::ffi::OsStr> = vec![std_flag.as_ref()];
        args.extend(WARNINGS.iter().map(std::ffi::OsStr::new));
        args.extend([
            "-fsyntax-only".as_ref(),
            "-I".as_ref(),
            dir.path().as_os_str(),
            path.as_os_str(),
        ]);
        gxx(&args, &file.path, &file.contents);
    }
}

/// A built test program.
pub(crate) struct Executable {
    _dir: TempDir,
    path: PathBuf,
}

/// The result of running a program.
pub(crate) struct Run {
    pub(crate) status: ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

impl Run {
    pub(crate) fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub(crate) fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// Compiles and links a project's source files with `extra` flags added.
/// Returns `None` when g++ is not available.
pub(crate) fn build(project: &GeneratedProject, extra: &[&str]) -> Option<Executable> {
    if !available() {
        return None;
    }
    let dir = write_files(project);
    let path = dir.path().join("program");
    let sources: Vec<PathBuf> = project
        .files
        .iter()
        .filter(|f| f.kind == FileKind::Source)
        .map(|f| dir.path().join(&f.path))
        .collect();
    let mut args: Vec<&std::ffi::OsStr> = vec!["-std=c++20".as_ref(), "-O0".as_ref()];
    args.extend(WARNINGS.iter().chain(extra).map(std::ffi::OsStr::new));
    args.extend(["-I".as_ref(), dir.path().as_os_str()]);
    args.extend(sources.iter().map(|p| p.as_os_str()));
    args.extend(["-o".as_ref(), path.as_os_str()]);
    let all: String = project.files.iter().map(|f| f.contents.as_str()).collect();
    gxx(&args, "the test program", &all);
    Some(Executable { _dir: dir, path })
}

/// Reads a pipe to the end on a background thread.
fn drain(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        bytes
    })
}

/// Waits for a child, killing it after [`RUN_TIMEOUT`].
fn wait(child: &mut Child) -> ExitStatus {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if start.elapsed() > RUN_TIMEOUT {
            let _ = child.kill();
            panic!("the test program did not finish within {RUN_TIMEOUT:?}");
        }
        thread::sleep(Duration::from_millis(5));
    }
}

/// Runs a built program with the given standard input.
pub(crate) fn run(executable: &Executable, stdin: &[u8]) -> Run {
    run_path(&executable.path, stdin)
}

fn run_path(path: &Path, stdin: &[u8]) -> Run {
    let mut child = command(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let bytes = stdin.to_vec();
    // The program may exit without reading everything; ignore broken pipes.
    let writer = thread::spawn(move || {
        let _ = input.write_all(&bytes);
    });
    let stdout = drain(child.stdout.take().unwrap());
    let stderr = drain(child.stderr.take().unwrap());
    let status = wait(&mut child);
    writer.join().unwrap();
    Run {
        status,
        stdout: undo_text_mode(stdout.join().unwrap()),
        stderr: undo_text_mode(stderr.join().unwrap()),
    }
}

/// On Windows, a program's standard streams are in text mode, so every `\n`
/// it writes arrives as `\r\n`. Text mode only ever inserts a `\r` before a
/// `\n`, so replacing each `\r\n` with `\n` gives back exactly the bytes
/// the program wrote (a written `\r\n` arrives as `\r\r\n` and comes back
/// as `\r\n`). Elsewhere the bytes are returned unchanged.
fn undo_text_mode(bytes: Vec<u8>) -> Vec<u8> {
    if !cfg!(windows) {
        return bytes;
    }
    restore_newlines(&bytes)
}

/// Replaces every `\r\n` with `\n`, scanning left to right.
fn restore_newlines(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut rest = bytes;
    while let Some((&first, tail)) = rest.split_first() {
        if first == b'\r' && tail.first() == Some(&b'\n') {
            out.push(b'\n');
            rest = &tail[1..];
        } else {
            out.push(first);
            rest = tail;
        }
    }
    out
}

#[test]
fn text_mode_is_undone_exactly() {
    // What a program writes, and what arrives through a Windows text-mode stream.
    for written in [&b"a\nb"[..], b"\r\n", b"x\r", b"\r\r\n\n", b"", b"no newline"] {
        let mut arrived = Vec::new();
        for &byte in written {
            if byte == b'\n' {
                arrived.push(b'\r');
            }
            arrived.push(byte);
        }
        assert_eq!(restore_newlines(&arrived), written);
    }
}
