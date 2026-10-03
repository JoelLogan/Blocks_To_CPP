//! The subcommands.

mod project;

use std::io::Write as _;

pub(crate) use project::{check, fmt, generate, migrate};

/// Writes text to standard output. A closed pipe (`b2c check … | head`) is
/// not an error worth reporting, so write failures are ignored.
fn out(text: &str) {
    let _ = std::io::stdout().lock().write_all(text.as_bytes());
}

/// Writes text to standard error, ignoring write failures.
fn err(text: &str) {
    let _ = std::io::stderr().lock().write_all(text.as_bytes());
}

/// Writes `b2c: <message>` and a newline to standard error.
fn fail(message: &str) {
    err(&format!("b2c: {message}\n"));
}
