//! Fuzzes `parse_output`, which combines everything one compiler run
//! produced (docs/spec/07-toolchain-build-run.md §7.5.3): standard error and,
//! for the SARIF formats, the SARIF file.
//!
//! Input: one mode byte, then standard error. Mode bits 0–1 pick the
//! diagnostics format (0 `AddOutputSarif`, 1 `SarifFile`, 2 `Json`,
//! 3 `Plain`). With mode bit 2 set, a SARIF file is passed too: the bytes
//! after the first `\0sarif\0` separator (empty when there is none), and
//! standard error ends before the separator. Without it, there is no SARIF
//! file and the whole rest is standard error.
//!
//! It must never panic or allocate without bound, the result must be
//! deterministic and obey the shared oracle (`diag_checks`), and it must be
//! exactly what `parse_output` documents:
//!
//! * SARIF formats with a SARIF file that parses: the file's messages, then
//!   only the driver's and linker's messages from the text on standard
//!   error, up to `MAX_MESSAGES`, truncated when either part was or when
//!   messages did not fit;
//! * SARIF formats without a usable file, and the plain format: standard
//!   error read as text;
//! * the JSON format: standard error read as GCC JSON mixed with text, with
//!   any SARIF file ignored.

#![no_main]

mod diag_checks;

use b2c_toolchain::diagnostics::{
    CompilerMessage, MAX_MESSAGES, MessageOrigin, parse_output, parse_sarif, parse_text,
};
use b2c_toolchain::probe::DiagnosticsFormat;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let format = match mode & 0b11 {
        0 => DiagnosticsFormat::AddOutputSarif,
        1 => DiagnosticsFormat::SarifFile,
        2 => DiagnosticsFormat::Json,
        _ => DiagnosticsFormat::Plain,
    };
    let (stderr, sarif) = diag_checks::split_output_input(rest, mode & 0b100 != 0);

    let output = parse_output(format, stderr, sarif);
    assert_eq!(
        output,
        parse_output(format, stderr, sarif),
        "parse_output is not deterministic"
    );
    diag_checks::check_output(&output);

    let text = parse_text(&String::from_utf8_lossy(stderr));
    match format {
        DiagnosticsFormat::AddOutputSarif | DiagnosticsFormat::SarifFile => match sarif.map(parse_sarif) {
            Some(Ok(from_sarif)) => {
                let (head, tail) = output
                    .messages
                    .split_at_checked(from_sarif.messages.len())
                    .expect("every SARIF message is kept");
                assert_eq!(head, from_sarif.messages, "the SARIF messages come first");
                let not_compiler: Vec<&CompilerMessage> = text
                    .messages
                    .iter()
                    .filter(|message| message.origin != MessageOrigin::Compiler)
                    .collect();
                let room = MAX_MESSAGES - head.len();
                let expected_tail: Vec<CompilerMessage> = not_compiler
                    .iter()
                    .take(room)
                    .map(|&message| message.clone())
                    .collect();
                assert_eq!(
                    tail, expected_tail,
                    "only the driver's and linker's text messages follow, in order"
                );
                assert_eq!(
                    output.truncated,
                    from_sarif.truncated || text.truncated || not_compiler.len() > room,
                    "the truncated flag"
                );
            }
            Some(Err(_)) | None => assert_eq!(output, text, "the text fallback"),
        },
        DiagnosticsFormat::Json => {
            assert_eq!(
                output,
                parse_output(DiagnosticsFormat::Json, stderr, None),
                "the JSON format ignores the SARIF file"
            );
            diag_checks::check_truncation_is_real(&output);
        }
        DiagnosticsFormat::Plain => assert_eq!(output, text, "plain text"),
    }
});
