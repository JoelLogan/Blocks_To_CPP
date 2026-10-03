//! The malicious-project regression suite (spec 08 §8.12): every crafted file
//! in `tests/security/projects/` is rejected by the loader with exactly the
//! problem codes listed in that folder's `README.md`, or accepted.
//!
//! The README table is the single list of expectations; this test also makes
//! sure it names every file in the folder and nothing else.

// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;

use b2c_ir::{DiagSource, Diagnostic, Severity};
use b2c_model::{Document, load, to_canonical_json};
use common::repo_root;

/// What the loader must do with a file.
#[derive(Debug, PartialEq, Eq)]
enum Expected {
    /// Load it.
    Accepted,
    /// Reject it with exactly these codes.
    Rejected(BTreeSet<String>),
}

/// One row of the README table.
#[derive(Debug)]
struct Case {
    file: String,
    loader: Expected,
}

fn suite_dir() -> PathBuf {
    repo_root().join("tests/security/projects")
}

/// The `B2C-…` codes written in backticks in a table cell.
fn codes_in(cell: &str) -> BTreeSet<String> {
    cell.split('`')
        .filter(|piece| piece.starts_with("B2C-"))
        .map(str::to_owned)
        .collect()
}

/// The rows of the README table.
fn cases() -> Vec<Case> {
    let readme = std::fs::read_to_string(suite_dir().join("README.md")).unwrap();
    let mut cases = Vec::new();
    for line in readme.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        assert_eq!(cells.len(), 8, "a table row needs six cells: {line}");
        let file = cells[1].trim_matches('`').to_owned();
        let loader = match cells[4] {
            "accepted" => Expected::Accepted,
            cell => {
                let codes = codes_in(cell);
                assert!(
                    !codes.is_empty(),
                    "{file}: the Loader cell names no codes: {cell:?}"
                );
                Expected::Rejected(codes)
            }
        };
        cases.push(Case { file, loader });
    }
    assert!(cases.len() >= 60, "the README table looks truncated");
    cases
}

/// The document with its top-level blocks in canonical (ID) order.
fn sorted(mut document: Document) -> Document {
    for module in &mut document.modules {
        module.workspace.blocks.sort_by(|a, b| a.id.cmp(&b.id));
    }
    document
}

/// Characters a message must never show raw: controls, bidi controls and
/// invisible format characters (hostile text is quoted with escapes).
fn is_unsafe_to_show(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{00AD}' | '\u{061C}' | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}')
}

/// Every message is short and free of characters that could hide or fake
/// text in a terminal or the UI.
fn assert_messages_are_safe(file: &str, diagnostics: &[Diagnostic]) {
    let texts = diagnostics
        .iter()
        .flat_map(|d| std::iter::once(&d.message).chain(d.related.iter().map(|r| &r.message)));
    for text in texts {
        assert!(
            !text.chars().any(is_unsafe_to_show),
            "{file}: a message shows an unsafe character: {text:?}"
        );
        assert!(
            text.len() < 1024,
            "{file}: a message is {} bytes long",
            text.len()
        );
    }
}

#[test]
fn the_table_lists_every_file_exactly_once() {
    let listed: Vec<String> = cases().into_iter().map(|case| case.file).collect();
    let unique: BTreeSet<String> = listed.iter().cloned().collect();
    assert_eq!(unique.len(), listed.len(), "a file is listed twice");
    let on_disk: BTreeSet<String> = std::fs::read_dir(suite_dir())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "README.md")
        .collect();
    assert_eq!(unique, on_disk);
    assert!(
        on_disk
            .iter()
            .all(|name| std::path::Path::new(name).extension().is_some_and(|e| e == "b2c"))
    );
}

#[test]
fn every_crafted_project_has_its_expected_outcome() {
    let mut failures = Vec::new();
    for case in cases() {
        let bytes = std::fs::read(suite_dir().join(&case.file)).unwrap();
        let outcome = load(&bytes);
        match (&case.loader, outcome) {
            (Expected::Accepted, Ok(document)) => {
                // Saving does not turn hostile content into something else
                // (apart from the canonical order of top-level blocks).
                let saved = to_canonical_json(&document);
                let reloaded = load(saved.as_bytes())
                    .unwrap_or_else(|e| panic!("{}: the saved copy does not load: {e:?}", case.file));
                assert_eq!(reloaded, sorted(document), "{}", case.file);
                assert_eq!(to_canonical_json(&reloaded), saved, "{}", case.file);
            }
            (Expected::Accepted, Err(error)) => {
                let found: Vec<String> = error.diagnostics.iter().map(|d| d.code.0.clone()).collect();
                failures.push(format!("{}: expected to load, but got {found:?}", case.file));
            }
            (Expected::Rejected(expected), Ok(_)) => {
                failures.push(format!("{}: expected {expected:?}, but it loaded", case.file));
            }
            (Expected::Rejected(expected), Err(error)) => {
                let diagnostics = error.diagnostics;
                let found: BTreeSet<String> = diagnostics.iter().map(|d| d.code.0.clone()).collect();
                if &found != expected {
                    failures.push(format!("{}: expected {expected:?}, found {found:?}", case.file));
                }
                assert!(
                    diagnostics
                        .iter()
                        .all(|d| d.severity == Severity::Error && d.source == DiagSource::Loader),
                    "{}",
                    case.file
                );
                assert!(diagnostics.len() <= 1001, "{}", case.file);
                assert_messages_are_safe(&case.file, &diagnostics);
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn codes_are_read_from_cells() {
    assert_eq!(
        codes_in("`B2C-E0117`, `B2C-E0119`"),
        BTreeSet::from(["B2C-E0117".to_owned(), "B2C-E0119".to_owned()])
    );
    assert!(codes_in("accepted").is_empty());
    assert!(is_unsafe_to_show('\u{202e}'));
    assert!(is_unsafe_to_show('\u{1b}'));
    assert!(!is_unsafe_to_show('é'));
}
