//! Fuzzes `parse_text`, the hand-written reader for GCC's plain-text
//! diagnostics and the linker's messages (docs/spec/07-toolchain-build-run.md
//! §7.5.3), on arbitrary bytes read as UTF-8 with invalid sequences
//! replaced, as the build reads standard error. It must never panic or
//! allocate without bound, and:
//!
//! * parsing is deterministic and the result obeys the shared oracle
//!   (`diag_checks`: bounds, cleaned text, 1-based lines, symbols only on
//!   linker messages), with `truncated` set only when a list is full;
//! * `parse_output` reads standard error exactly like this for the plain
//!   format, and for the SARIF formats when the SARIF file is missing or
//!   malformed (its documented fallback).

#![no_main]

mod diag_checks;

use b2c_toolchain::diagnostics::{parse_output, parse_text};
use b2c_toolchain::probe::DiagnosticsFormat;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let parsed = parse_text(&text);
    assert_eq!(parsed, parse_text(&text), "parse_text is not deterministic");
    diag_checks::check_output(&parsed);
    diag_checks::check_truncation_is_real(&parsed);

    // One call per fallback path; `diag_output` covers the other combinations.
    assert_eq!(
        parse_output(DiagnosticsFormat::Plain, data, None),
        parsed,
        "parse_output(Plain) disagrees with parse_text"
    );
    assert_eq!(
        parse_output(DiagnosticsFormat::SarifFile, data, None),
        parsed,
        "parse_output(SarifFile) without a SARIF file disagrees with parse_text"
    );
    assert_eq!(
        parse_output(DiagnosticsFormat::AddOutputSarif, data, Some(b"not SARIF")),
        parsed,
        "parse_output(AddOutputSarif) with a malformed SARIF file disagrees with parse_text"
    );
});
