//! Shared helpers for the catalog tests.

#![allow(dead_code, reason = "each test crate uses a different subset")]
#![allow(clippy::needless_pass_by_value, reason = "helpers take json! values")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use b2c_catalog::CatalogFile;
use b2c_ir::Diagnostic;
use b2c_model::Document;
use serde_json::{Value, json};

/// The repository root.
pub(crate) fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every block ID in the TOML files of `catalog/core/` on disk. The tests
/// count the catalog's blocks with it, so a package that adds blocks to its
/// own file (`docs/plans/m3-contracts/w2.md` §3.2) need not change a count
/// here.
pub(crate) fn block_ids_on_disk() -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let dir = repo_root().join("catalog/core");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        assert_eq!(path.extension().unwrap(), "toml", "{}", path.display());
        let file: CatalogFile = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for block in file.block {
            assert!(ids.insert(block.id.clone()), "{} is defined twice", block.id);
        }
    }
    ids
}

/// A project whose `main` holds `body` and whose canvas also holds `extra`
/// top-level blocks.
pub(crate) fn project(body: Value, extra: Value) -> Value {
    let mut blocks = vec![json!({
        "id": "b_main",
        "type": "program.main",
        "v": 1,
        "statements": { "BODY": body }
    })];
    if let Value::Array(more) = extra {
        blocks.extend(more);
    }
    json!({
        "format": "blocks2cpp/project",
        "formatVersion": 1,
        "generator": { "app": "0.1.0", "catalog": "1.0.0" },
        "project": { "id": "prj_test", "name": "Test", "language": { "standard": "c++20" } },
        "modules": [{ "id": "mod_main", "name": "main", "workspace": { "blocks": blocks } }]
    })
}

/// Loads a JSON project (which must be valid for the loader).
pub(crate) fn load(value: &Value) -> Document {
    match b2c_model::load(&serde_json::to_vec(value).unwrap()) {
        Ok(document) => document,
        Err(error) => panic!("the loader rejected the test project: {:#?}", error.diagnostics),
    }
}

/// Resolves a JSON project against the core catalog.
pub(crate) fn resolve(value: &Value) -> (Document, Vec<Diagnostic>) {
    b2c_catalog::resolve(&load(value), b2c_catalog::core_catalog())
}

/// The codes of resolving a JSON project.
pub(crate) fn codes(value: &Value) -> Vec<String> {
    resolve(value).1.into_iter().map(|d| d.code.0).collect()
}

/// The messages of resolving a JSON project.
pub(crate) fn messages(value: &Value) -> Vec<String> {
    resolve(value).1.into_iter().map(|d| d.message).collect()
}
