//! Fuzzes `parse_sarif`, the reader for the SARIF 2.1 file GCC 13+ writes
//! (docs/spec/07-toolchain-build-run.md §7.5.3), with arbitrary bytes. It
//! must never panic or allocate without bound, and:
//!
//! * parsing is deterministic;
//! * an accepted log obeys the shared oracle (`diag_checks`: bounds,
//!   cleaned text, 1-based lines) and every message comes from the
//!   compiler; a rejection is a SARIF `Malformed` error;
//! * `parse_output` with this file and empty standard error gives exactly
//!   the same messages, or none when the file is rejected (the text
//!   fallback then reads the empty standard error).

#![no_main]

mod diag_checks;

use b2c_toolchain::diagnostics::{MAX_INPUT_BYTES, MessageOrigin, ParseError, parse_output, parse_sarif};
use b2c_toolchain::probe::DiagnosticsFormat;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let parsed = parse_sarif(data);
    assert_eq!(parsed, parse_sarif(data), "parse_sarif is not deterministic");
    match &parsed {
        Ok(output) => {
            diag_checks::check_output(output);
            diag_checks::check_truncation_is_real(output);
            diag_checks::check_origin(output, MessageOrigin::Compiler);
        }
        Err(ParseError::Malformed { format, .. }) => assert_eq!(*format, "SARIF"),
        Err(ParseError::TooLarge) => assert!(data.len() > MAX_INPUT_BYTES),
    }

    let expected = parsed.unwrap_or_default();
    for format in [DiagnosticsFormat::SarifFile, DiagnosticsFormat::AddOutputSarif] {
        assert_eq!(
            parse_output(format, b"", Some(data)),
            expected,
            "parse_output({format:?}) disagrees with parse_sarif"
        );
    }
});
