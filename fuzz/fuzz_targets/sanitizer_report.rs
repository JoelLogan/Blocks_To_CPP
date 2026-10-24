//! Fuzzes the sanitizer report scanner `b2c_toolchain::sanitizer::Detector`
//! (docs/spec/07-toolchain-build-run.md §7.6.4), which reads a running
//! program's output: bytes the program controls, arriving in chunks of any
//! size. The first input byte chooses how the rest is cut into chunks. It
//! must never panic or allocate without bound, and:
//!
//! * it never keeps more than `MAX_LINE_BYTES` (4 KiB) of line state;
//! * the result does not depend on how the output was cut into chunks;
//! * a reported kind matches `[a-z0-9-]{1,64}` (the format the IPC contract
//!   enforces for `SanitizerKind`), and the summary is
//!   `Crashed: <kind> (<sanitizer name>)`;
//! * the first report wins: once the output ends with a line break, more
//!   output never changes the report.

#![no_main]

use b2c_toolchain::sanitizer::{Detector, MAX_KIND_LEN, MAX_LINE_BYTES};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&seed, output)) = data.split_first() else {
        return;
    };

    let mut whole = Detector::new();
    whole.feed(output);
    assert!(whole.pending_line_len() <= MAX_LINE_BYTES, "line state over 4 KiB");
    let expected = whole.report();

    // The same output in chunks of 1 to 257 bytes, the sizes following the seed.
    let mut chunked = Detector::new();
    let mut rest = output;
    let mut size = usize::from(seed) + 1;
    while !rest.is_empty() {
        let (piece, tail) = rest.split_at(size.min(rest.len()));
        chunked.feed(piece);
        assert!(chunked.pending_line_len() <= MAX_LINE_BYTES, "line state over 4 KiB");
        rest = tail;
        size = (size * 7 + 3) % 257 + 1;
    }
    assert_eq!(chunked.report(), expected, "chunking changed the result");

    let Some(report) = expected else {
        return;
    };
    let kind = report.kind.as_bytes();
    assert!(
        (1..=MAX_KIND_LEN).contains(&kind.len())
            && kind
                .iter()
                .all(|&b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
        "malformed kind {:?}",
        report.kind
    );
    assert_eq!(
        report.summary(),
        format!("Crashed: {} ({})", report.kind, report.tool.name())
    );
    if output.ends_with(b"\n") {
        whole.feed(output);
        assert_eq!(whole.report(), Some(report), "a later report replaced the first");
    }
});
