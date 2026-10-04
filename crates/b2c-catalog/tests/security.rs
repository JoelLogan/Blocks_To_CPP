//! The malicious-project regression suite (spec 08 §8.12), resolve stage:
//! every crafted file in `tests/security/projects/` that the loader accepts
//! resolves with exactly the problem codes listed in the folder's
//! `README.md` (or cleanly), and resolving never panics.
//!
//! The loader side of the same table is checked by
//! `crates/b2c-model/tests/security.rs`.

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

use b2c_catalog::{core_catalog, resolve};
use b2c_ir::{DiagSource, Severity};
use b2c_model::load;

/// What resolving an accepted file must report.
#[derive(Debug, PartialEq, Eq)]
enum Expected {
    /// The loader rejects the file, so resolving is not reached (`—`).
    NotReached,
    /// No problems (`clean`).
    Clean,
    /// Exactly these codes.
    Codes(BTreeSet<String>),
}

fn suite_dir() -> PathBuf {
    common::repo_root().join("tests/security/projects")
}

/// The `B2C-…` codes written in backticks in a table cell.
fn codes_in(cell: &str) -> BTreeSet<String> {
    cell.split('`')
        .filter(|piece| piece.starts_with("B2C-"))
        .map(str::to_owned)
        .collect()
}

/// `(file, accepted by the loader, resolve expectation)` for every row.
fn cases() -> Vec<(String, bool, Expected)> {
    let readme = std::fs::read_to_string(suite_dir().join("README.md")).unwrap();
    let mut cases = Vec::new();
    for line in readme.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        assert_eq!(cells.len(), 8, "a table row needs six cells: {line}");
        let file = cells[1].trim_matches('`').to_owned();
        let accepted = cells[4] == "accepted";
        let expected = match cells[5] {
            "—" => Expected::NotReached,
            "clean" => Expected::Clean,
            cell => {
                let codes = codes_in(cell);
                assert!(
                    !codes.is_empty(),
                    "{file}: the Resolve cell names no codes: {cell:?}"
                );
                Expected::Codes(codes)
            }
        };
        // A file the loader rejects never reaches resolving, and vice versa.
        assert_eq!(accepted, expected != Expected::NotReached, "{file}");
        cases.push((file, accepted, expected));
    }
    assert!(cases.len() >= 60, "the README table looks truncated");
    cases
}

/// Characters a message must never show raw.
fn is_unsafe_to_show(c: char) -> bool {
    c.is_control()
        || b2c_ir::text::is_invisible(c)
        || matches!(c,
            '\u{00AD}' | '\u{061C}' | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}')
}

#[test]
fn accepted_crafted_projects_resolve_as_expected() {
    let mut failures = Vec::new();
    let mut resolved = 0;
    for (file, accepted, expected) in cases() {
        if !accepted {
            continue;
        }
        let bytes = std::fs::read(suite_dir().join(&file)).unwrap();
        let document = load(&bytes).unwrap_or_else(|e| panic!("{file}: {e:?}"));
        let (completed, diagnostics) = resolve(&document, core_catalog());
        resolved += 1;
        let found: BTreeSet<String> = diagnostics.iter().map(|d| d.code.0.clone()).collect();
        let wanted = match expected {
            Expected::Clean => BTreeSet::new(),
            Expected::Codes(codes) => codes,
            Expected::NotReached => unreachable!("checked when reading the table"),
        };
        if found != wanted {
            failures.push(format!("{file}: expected {wanted:?}, found {found:?}"));
        }
        for diagnostic in &diagnostics {
            assert_eq!(diagnostic.source, DiagSource::Catalog, "{file}");
            assert_eq!(diagnostic.severity, Severity::Error, "{file}");
            assert!(
                !diagnostic.message.chars().any(is_unsafe_to_show),
                "{file}: unsafe character in {:?}",
                diagnostic.message
            );
            assert!(diagnostic.message.len() < 1024, "{file}");
        }
        // Resolving the completed copy again changes nothing more.
        let (again, _) = resolve(&completed, core_catalog());
        assert_eq!(again, completed, "{file}");
    }
    assert!(resolved >= 20, "only {resolved} files reached resolving");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
