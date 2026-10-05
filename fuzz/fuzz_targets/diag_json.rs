//! Fuzzes `parse_gcc_json`, the reader for the diagnostics array GCC 10–14
//! prints with `-fdiagnostics-format=json` (docs/spec/07-toolchain-build-run.md
//! §7.5.3), and the standard-error reader `parse_output` uses for that format,
//! where the array is mixed with the driver's and linker's text. Arbitrary
//! bytes must never panic, overflow the stack (GCC nests notes) or allocate
//! without bound, and:
//!
//! * both readers are deterministic and their results obey the shared oracle
//!   (`diag_checks`: bounds, cleaned text, 1-based lines);
//! * an accepted array gives only compiler messages, and a rejection is a
//!   JSON `Malformed` error;
//! * standard error that is exactly one array (with surrounding whitespace)
//!   reads the same through `parse_output` as through `parse_gcc_json`.

#![no_main]

mod diag_checks;

use b2c_toolchain::diagnostics::{MAX_INPUT_BYTES, MessageOrigin, ParseError, parse_gcc_json, parse_output};
use b2c_toolchain::probe::DiagnosticsFormat;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let stderr = parse_output(DiagnosticsFormat::Json, data, None);
    assert_eq!(
        stderr,
        parse_output(DiagnosticsFormat::Json, data, None),
        "parse_output(Json) is not deterministic"
    );
    diag_checks::check_output(&stderr);
    diag_checks::check_truncation_is_real(&stderr);

    let parsed = parse_gcc_json(data);
    assert_eq!(
        parsed,
        parse_gcc_json(data),
        "parse_gcc_json is not deterministic"
    );
    match parsed {
        Ok(output) => {
            diag_checks::check_output(&output);
            diag_checks::check_truncation_is_real(&output);
            diag_checks::check_origin(&output, MessageOrigin::Compiler);
            assert_eq!(stderr, output, "parse_output(Json) disagrees with parse_gcc_json");
        }
        Err(ParseError::Malformed { format, .. }) => assert_eq!(format, "JSON"),
        Err(ParseError::TooLarge) => assert!(data.len() > MAX_INPUT_BYTES),
    }
});
