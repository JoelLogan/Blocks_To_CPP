//! The example projects load cleanly and are stored in canonical form.

// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod common;

use std::collections::BTreeMap;

use b2c_model::{Document, content_hash, load, to_canonical_json};
use common::{base, bytes, example_paths};

fn load_ok(raw: &[u8], name: &str) -> Document {
    match load(raw) {
        Ok(document) => document,
        Err(error) => panic!("{name}: {:#?}", error.diagnostics),
    }
}

#[test]
fn every_example_loads_and_is_canonical() {
    for path in example_paths() {
        let raw = std::fs::read(&path).unwrap();
        let name = path.display().to_string();
        let document = load_ok(&raw, &name);
        let canonical = to_canonical_json(&document);
        assert!(
            canonical.as_bytes() == raw.as_slice(),
            "{name} is not in canonical form; run `b2c fmt {name}`"
        );
    }
}

#[test]
fn the_loader_agrees_with_serde() {
    for path in example_paths() {
        let raw = std::fs::read(&path).unwrap();
        let document = load_ok(&raw, &path.display().to_string());
        let through_serde: Document = serde_json::from_slice(&raw).unwrap();
        assert_eq!(document, through_serde, "{}", path.display());
        // The hand-written writer gives exactly what serde_json would.
        assert_eq!(
            to_canonical_json(&document),
            serde_json::to_string_pretty(&document).unwrap() + "\n"
        );
    }
}

#[test]
fn example_hashes_are_stable_and_distinct() {
    let mut seen = BTreeMap::new();
    for path in example_paths() {
        let raw = std::fs::read(&path).unwrap();
        let document = load_ok(&raw, &path.display().to_string());
        let hash = content_hash(&document);
        assert_eq!(hash, content_hash(&document.clone()));
        if let Some(other) = seen.insert(hash, path.clone()) {
            panic!("{} and {} have the same hash", path.display(), other.display());
        }
    }
}

/// The hash is part of build-cache keys and trust records, so its exact
/// value for a fixed document is pinned: changing the canonical form or the
/// layout-only keys must be a deliberate decision.
#[test]
fn content_hash_is_pinned() {
    let document = load(&bytes(&base())).unwrap();
    let hex = content_hash(&document)
        .iter()
        .fold(String::new(), |hex, b| hex + &format!("{b:02x}"));
    // Cross-checked when it was pinned: SHA-256 of the canonical JSON with
    // the layout keys removed, computed independently with Python's `json`
    // and `hashlib`.
    assert_eq!(
        hex,
        "4f106eb4bc5fddae71dfc1e182df47af989a3b42f309d01e6ad8ca860766ad9e"
    );
}
