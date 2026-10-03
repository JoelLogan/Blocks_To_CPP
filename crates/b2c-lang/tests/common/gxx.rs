//! Checking generated C++ with the system g++.
//!
//! When g++ is not on `PATH`, [`available`] prints a note and returns false,
//! so the tests that need it pass without checking anything, unless the
//! environment variable `B2C_REQUIRE_GXX` is set (CI sets it): then it fails.

use std::io::Write as _;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

/// A command for an external program. Production code spawns processes only
/// through b2c-process (spec §8.5); tests drive g++ directly.
#[allow(clippy::disallowed_methods)]
fn command(program: &str) -> Command {
    Command::new(program)
}

/// Whether g++ can be run. Panics when `B2C_REQUIRE_GXX` is set and it can't.
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
            eprintln!("note: g++ was not found on PATH; skipping the checks that generated C++ compiles");
        }
        found
    })
}

/// Checks one C++ source file with `g++ -std=c++20 -Wall -Wextra
/// -fsyntax-only` (plus `-Werror` when `warnings_are_errors`), reading it
/// from standard input. Returns g++'s messages when it rejects the file.
pub(crate) fn syntax_check(source: &str, warnings_are_errors: bool) -> Result<(), String> {
    let mut args = vec!["-std=c++20", "-Wall", "-Wextra"];
    if warnings_are_errors {
        args.push("-Werror");
    }
    args.extend(["-fsyntax-only", "-x", "c++", "-"]);
    let mut child = command("g++")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start g++: {e}"))?;
    let written = child
        .stdin
        .take()
        .ok_or_else(|| String::from("no stdin for g++"))?
        .write_all(source.as_bytes());
    let output = child.wait_with_output().map_err(|e| format!("g++ failed: {e}"))?;
    written.map_err(|e| format!("could not write to g++: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}
