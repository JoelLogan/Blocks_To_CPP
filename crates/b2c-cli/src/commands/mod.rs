//! The subcommands.

mod build;
mod project;

use std::io::Write as _;
use std::time::Duration;

pub(crate) use build::{build, run, toolchains};
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

/// Parses a duration such as `250ms`, `10s`, `1.5s` or `2m`.
///
/// # Errors
/// Returns a message for clap when the text is not a positive duration of at
/// most 24 hours.
pub(crate) fn parse_duration(text: &str) -> Result<Duration, String> {
    let (number, unit_seconds) = if let Some(n) = text.strip_suffix("ms") {
        (n, 0.001)
    } else if let Some(n) = text.strip_suffix('s') {
        (n, 1.0)
    } else if let Some(n) = text.strip_suffix('m') {
        (n, 60.0)
    } else if let Some(n) = text.strip_suffix('h') {
        (n, 3600.0)
    } else {
        return Err(String::from(
            "expected a number followed by ms, s, m or h (for example 10s)",
        ));
    };
    let value: f64 = number
        .parse()
        .map_err(|_| format!("{number:?} is not a number"))?;
    let seconds = value * unit_seconds;
    if !seconds.is_finite() || !(0.001..=86_400.0).contains(&seconds) {
        return Err(String::from("the timeout must be at least 1ms and at most 24h"));
    }
    Ok(Duration::from_secs_f64(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_parse() {
        assert_eq!(parse_duration("250ms"), Ok(Duration::from_millis(250)));
        assert_eq!(parse_duration("10s"), Ok(Duration::from_secs(10)));
        assert_eq!(parse_duration("1.5s"), Ok(Duration::from_millis(1500)));
        assert_eq!(parse_duration("2m"), Ok(Duration::from_mins(2)));
        assert_eq!(parse_duration("1h"), Ok(Duration::from_hours(1)));
        for bad in [
            "", "10", "s", "-1s", "0s", "NaNs", "infs", "25h", "1e400s", "10 s", "1e-300s", "0.5ms",
        ] {
            assert!(parse_duration(bad).is_err(), "{bad}");
        }
    }
}
