//! Shared helpers for the loader tests.

#![allow(dead_code, reason = "each test crate uses a different subset")]

use std::path::{Path, PathBuf};

use b2c_ir::Diagnostic;
use serde_json::{Value, json};

/// The repository root.
pub(crate) fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every `examples/*.b2c`, sorted by name.
pub(crate) fn example_paths() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(repo_root().join("examples"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "b2c"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 15, "examples are missing");
    paths
}

/// A small valid project: `main` printing "hi", plus a function.
pub(crate) fn base() -> Value {
    json!({
        "format": "blocks2cpp/project",
        "formatVersion": 1,
        "generator": { "app": "0.1.0", "catalog": "1.0.0" },
        "project": {
            "id": "prj_test",
            "name": "Test",
            "language": { "standard": "c++20" }
        },
        "modules": [{
            "id": "mod_main",
            "name": "main",
            "workspace": {
                "blocks": [
                    {
                        "id": "b_main",
                        "type": "program.main",
                        "v": 1,
                        "x": 0,
                        "y": 0,
                        "statements": {
                            "BODY": [
                                {
                                    "id": "b_print",
                                    "type": "io.print",
                                    "v": 1,
                                    "extra": { "itemCount": 1 },
                                    "fields": { "SEP": "none", "NEWLINE": true, "STREAM": "out" },
                                    "inputs": { "ITEM0": { "expr": [{ "str": "hi" }] } }
                                },
                                {
                                    "id": "b_var",
                                    "type": "var.declare",
                                    "v": 1,
                                    "fields": { "TYPE": "int", "NAME": { "sym": "s_x", "name": "x" } },
                                    "inputs": { "VALUE": { "block": {
                                        "id": "b_num",
                                        "type": "math.number",
                                        "v": 1,
                                        "fields": { "VALUE": "42" }
                                    } } }
                                }
                            ]
                        }
                    },
                    {
                        "id": "b_fn",
                        "type": "func.define",
                        "v": 1,
                        "x": 300,
                        "y": 0,
                        "extra": { "params": [{ "sym": "s_n", "name": "n", "type": "int", "mode": "copy" }] },
                        "fields": { "NAME": { "sym": "s_f", "name": "f" }, "RETURNS": "void" }
                    }
                ]
            }
        }]
    })
}

/// Serialises a JSON value for `load`.
pub(crate) fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec_pretty(value).unwrap()
}

/// Loads bytes that must fail; returns the diagnostics.
pub(crate) fn failure(bytes: &[u8]) -> Vec<Diagnostic> {
    match b2c_model::load(bytes) {
        Ok(_) => panic!("expected the load to fail"),
        Err(error) => {
            assert!(!error.diagnostics.is_empty());
            error.diagnostics
        }
    }
}

/// The codes of a failing load, in order.
pub(crate) fn codes(value: &Value) -> Vec<String> {
    failure(&bytes(value)).into_iter().map(|d| d.code.0).collect()
}

/// The `print` block of [`base`].
pub(crate) fn print_block(document: &mut Value) -> &mut Value {
    &mut document["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"][0]
}

/// The `main` block of [`base`].
pub(crate) fn main_block(document: &mut Value) -> &mut Value {
    &mut document["modules"][0]["workspace"]["blocks"][0]
}
