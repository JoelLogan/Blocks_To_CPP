//! Fuzzes the whole pure pipeline on arbitrary bytes, chained as
//! `b2c_build::run_frontend` does: load, resolve with the core catalog,
//! analyse (including the expression-slot parser) and generate C++
//! (docs/spec/06-compiler-pipeline.md §6.1, 08-security.md §8.4).
//!
//! * No stage panics, overflows the stack or allocates without bound.
//! * Analysis and generation are deterministic: running them again gives the
//!   same program, diagnostics, files and source map.
//! * A program the analyser accepts generates without error placeholders.
//! * Every generated file has a plain generated name (`<stem>.cpp` from
//!   letters, digits, `_` and `-`, not a Windows device name, or
//!   `b2c_support.hpp`), unique ignoring case, with a source-map entry.
//! * Generated text has no raw control character other than newline (and
//!   tab inside a `//` comment), no bidi or invisible character, no line
//!   ending in whitespace, `\` or `??/`, and ends with exactly one newline;
//!   source-map ranges are sorted and lie inside the file.
//!
//! Generation also runs for programs with analysis errors: `b2c_codegen`
//! promises never to panic on any program, and its best-effort output must
//! be just as safe. Only error-free programs are built (as in `run_frontend`),
//! and only for those must there be no error placeholders.

#![no_main]

use std::collections::BTreeSet;

use b2c_codegen::{CodegenOptions, Generation, HelperPlacement};
use b2c_ir::source_map::{FileKind, GeneratedProject};
use b2c_ir::text::is_invisible;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(loaded) = b2c_model::load(data) else {
        return;
    };
    let (document, resolve_diagnostics) = b2c_catalog::resolve(&loaded, b2c_catalog::core_catalog());
    // The analyser's input contract: a document the resolve stage accepted.
    if b2c_ir::has_errors(&resolve_diagnostics) {
        return;
    }
    let analysis = b2c_lang::analyze(&document);
    assert!(
        b2c_lang::analyze(&document) == analysis,
        "analysing the same document twice gave different results"
    );
    let accepted = !b2c_ir::has_errors(&analysis.diagnostics);

    let inline = CodegenOptions {
        project_name: document.project.name.clone(),
        app_version: String::from("0.0.0-fuzz"),
        do_not_edit_banner: true,
        indent_width: 4,
        helper_placement: HelperPlacement::Inline,
    };
    let header = CodegenOptions {
        do_not_edit_banner: false,
        helper_placement: HelperPlacement::Header,
        ..inline.clone()
    };
    for options in [inline, header] {
        let generation = b2c_codegen::generate_with_report(&analysis.program, &options);
        assert!(
            b2c_codegen::generate_with_report(&analysis.program, &options) == generation,
            "generating the same program twice gave different output ({:?})",
            options.helper_placement
        );
        check_generation(&generation, accepted, options.helper_placement);
    }
});

fn check_generation(generation: &Generation, accepted: bool, placement: HelperPlacement) {
    if accepted {
        assert_eq!(
            generation.placeholders, 0,
            "a program without analysis errors needed error placeholders ({placement:?})"
        );
    }
    check_project(&generation.project);
}

fn check_project(project: &GeneratedProject) {
    let maps: Vec<&str> = project
        .source_map
        .files
        .iter()
        .map(|map| map.path.as_str())
        .collect();
    let paths: Vec<&str> = project.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(maps, paths, "the source map does not list the generated files");
    let mut seen = BTreeSet::new();
    for file in &project.files {
        check_file_name(&file.path, file.kind);
        assert!(
            seen.insert(file.path.to_ascii_lowercase()),
            "two generated files are both named {:?} (ignoring case)",
            file.path
        );
        check_text(&file.path, &file.contents);
    }
    for map in &project.source_map.files {
        let file = project
            .files
            .iter()
            .find(|file| file.path == map.path)
            .expect("checked above");
        let lines: Vec<&str> = file.contents.split('\n').collect();
        let mut previous = None;
        for range in &map.ranges {
            let start = (range.start.line, range.start.column);
            let end = (range.end.line, range.end.column);
            assert!(
                start <= end,
                "{}: the source-map range {range:?} ends before it starts",
                map.path
            );
            assert!(
                previous.is_none_or(|previous| previous <= start),
                "{}: the source-map ranges are not sorted by start ({range:?})",
                map.path
            );
            previous = Some(start);
            for (line, column) in [start, end] {
                let text = usize::try_from(line)
                    .ok()
                    .and_then(|line| line.checked_sub(1))
                    .and_then(|index| lines.get(index));
                let inside = text.is_some_and(|text| {
                    usize::try_from(column).is_ok_and(|column| (1..=text.len() + 1).contains(&column))
                });
                assert!(
                    inside,
                    "{}: the source-map range {range:?} is outside the file",
                    map.path
                );
            }
        }
    }
}

/// Windows device names: a file with such a stem is a device, whatever its
/// extension (spec §8.4.1 rule 6).
fn is_device_name(stem: &str) -> bool {
    let lower = stem.to_ascii_lowercase();
    matches!(lower.as_str(), "con" | "prn" | "aux" | "nul")
        || (lower.len() == 4
            && (lower.starts_with("com") || lower.starts_with("lpt"))
            && lower.as_bytes()[3].is_ascii_digit())
}

fn check_file_name(path: &str, kind: FileKind) {
    match kind {
        FileKind::Header => assert_eq!(path, "b2c_support.hpp", "unexpected header name"),
        FileKind::Source => {
            let Some(stem) = path.strip_suffix(".cpp") else {
                panic!("the source file {path:?} does not end in .cpp");
            };
            let plain = !stem.is_empty()
                && stem.len() <= 128
                && stem
                    .bytes()
                    .next()
                    .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
                && stem
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
            assert!(
                plain,
                "the source file name {path:?} is not a plain generated name"
            );
            assert!(
                !is_device_name(stem),
                "the source file {path:?} is named like a Windows device"
            );
            assert!(
                !stem.eq_ignore_ascii_case("b2c_support"),
                "the source file {path:?} would clash with the support header"
            );
        }
    }
}

fn check_text(path: &str, contents: &str) {
    assert!(
        contents.ends_with('\n') && !contents.ends_with("\n\n"),
        "{path} does not end with exactly one newline"
    );
    // Not `lines()`, which would drop a `\r` before each `\n` unseen.
    let body = contents.strip_suffix('\n').unwrap_or(contents);
    for (index, line) in body.split('\n').enumerate() {
        let number = index + 1;
        let comment = line.trim_start_matches(' ').starts_with("//");
        for c in line.chars() {
            let allowed_tab = c == '\t' && comment;
            assert!(
                allowed_tab || !c.is_control(),
                "{path}:{number}: raw control character U+{:04X} in {line:?}",
                u32::from(c)
            );
            assert!(
                !is_invisible(c),
                "{path}:{number}: raw invisible or bidi character U+{:04X} in {line:?}",
                u32::from(c)
            );
        }
        assert!(
            line.trim_end() == line,
            "{path}:{number}: the line ends in whitespace: {line:?}"
        );
        assert!(
            !line.ends_with('\\'),
            "{path}:{number}: the line ends in `\\` and splices the next one: {line:?}"
        );
        assert!(
            !line.ends_with("??/"),
            "{path}:{number}: the line ends in the trigraph `??/`: {line:?}"
        );
    }
}
