//! Checks of the generated benchmark document ([`document`]), run by
//! `cargo test` (the `bench_document` test target): the benchmarks are only
//! meaningful when the document is a valid, error-free program of exactly
//! the requested size, and when the TypeScript generator of the webview
//! benchmarks produces the same document.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests fail by panicking"
)]

#[path = "document.rs"]
mod document;

use b2c_core_wasm::{PreviewOptions, Stage, preview_document};
use b2c_ir::Severity;
use serde_json::Value;

use crate::document::{
    GenerateError, HANDLE_BLOCKS, MAX_BLOCKS, Shape, UNIT_BLOCKS, count_blocks, generate, parity_digest,
};

/// The parity digests the TypeScript generator must also give
/// (`apps/desktop/e2e/bench/document.test.ts`). When the shape of the
/// document changes on purpose, change both generators and both copies of
/// these digests.
const DIGEST_1000: &str = "a1816065efc298a5ffb8647caeb57f9e32b9b40bfe724938c82572b18ffae64f";
const DIGEST_5000_HANDLE: &str = "05900f8a729be4e3c46e1d16ec6d3ad7359b430a9e9df019241fe96afa7fb70d";

fn shape(blocks: usize, drag_handle: bool) -> Shape {
    Shape { blocks, drag_handle }
}

/// The deepest JSON nesting of a value (an object or array is one level).
fn depth(value: &Value) -> usize {
    match value {
        Value::Object(map) => 1 + map.values().map(depth).max().unwrap_or(0),
        Value::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

#[test]
fn every_shape_has_exactly_the_requested_blocks() {
    let sizes = [1, 2, 8, 9, 10, 16, 17, 100, 1_000, 1_001, 5_000];
    for blocks in sizes {
        let generated = generate(shape(blocks, false)).unwrap();
        assert_eq!(count_blocks(&generated), blocks, "{blocks} blocks");
    }
    for blocks in [3, 4, 11, 1_000, 5_000] {
        let generated = generate(shape(blocks, true)).unwrap();
        assert_eq!(
            count_blocks(&generated),
            blocks,
            "{blocks} blocks with the handle"
        );
    }
}

#[test]
fn a_unit_is_eight_blocks_and_the_rest_are_fillers() {
    let generated = generate(shape(1 + 2 * UNIT_BLOCKS + 3, false)).unwrap();
    let body = generated
        .pointer("/modules/0/workspace/blocks/0/statements/BODY")
        .unwrap();
    let types: Vec<&str> = body
        .as_array()
        .unwrap()
        .iter()
        .map(|block| block["type"].as_str().unwrap())
        .collect();
    assert_eq!(
        types,
        [
            "var.declare",
            "control.for_range",
            "io.print",
            "var.declare",
            "control.for_range",
            "io.print",
            "io.print",
            "io.print",
            "io.print",
        ]
    );
    assert_eq!(body[8]["inputs"]["ITEM0"]["expr"][0]["str"], "filler 2");
}

#[test]
fn block_ids_follow_document_order() {
    let generated = generate(shape(1 + UNIT_BLOCKS, false)).unwrap();
    let main = &generated["modules"][0]["workspace"]["blocks"][0];
    assert_eq!(main["id"], "b0");
    let body = &main["statements"]["BODY"];
    assert_eq!(body[0]["id"], "b1");
    assert_eq!(body[1]["id"], "b2");
    let branch = &body[1]["statements"]["BODY"][0];
    assert_eq!(branch["id"], "b3");
    assert_eq!(branch["statements"]["DO0"][0]["id"], "b4");
    assert_eq!(branch["statements"]["ELSE"][0]["id"], "b5");
    assert_eq!(body[2]["id"], "b6");
    let sum = &body[2]["inputs"]["ITEM1"]["block"];
    assert_eq!(sum["id"], "b7");
    assert_eq!(sum["inputs"]["A"]["block"]["id"], "b8");
}

#[test]
fn the_drag_handle_is_a_separate_definition() {
    let generated = generate(shape(1_000, true)).unwrap();
    let top = generated["modules"][0]["workspace"]["blocks"].as_array().unwrap();
    assert_eq!(top.len(), 2);
    assert_eq!(top[1]["type"], "func.define");
    assert_eq!(top[1]["id"], format!("b{}", 1_000 - HANDLE_BLOCKS));
    assert_eq!(top[1]["x"], 800);
    assert_eq!(top[1]["statements"]["BODY"][0]["id"], format!("b{}", 1_000 - 1));
}

#[test]
fn shapes_outside_the_limits_are_refused() {
    assert_eq!(
        generate(shape(0, false)),
        Err(GenerateError::TooFew { blocks: 0, min: 1 })
    );
    assert_eq!(
        generate(shape(2, true)),
        Err(GenerateError::TooFew { blocks: 2, min: 3 })
    );
    assert_eq!(
        generate(shape(MAX_BLOCKS + 1, false)),
        Err(GenerateError::TooMany {
            blocks: MAX_BLOCKS + 1
        })
    );
    assert!(generate(shape(MAX_BLOCKS, false)).is_ok());
}

#[test]
fn the_benchmark_documents_load_and_preview_without_errors() {
    for (blocks, drag_handle) in [(1_000, false), (5_000, true)] {
        let generated = generate(shape(blocks, drag_handle)).unwrap();
        // Far from the 128 levels a project may nest (05 §5.6).
        assert!(depth(&generated) < 32, "{}", depth(&generated));
        let loaded = b2c_model::load(generated.to_string().as_bytes()).unwrap();
        let text = b2c_model::to_canonical_json(&loaded);
        let preview = preview_document(&text, &PreviewOptions::default());
        let errors: Vec<&str> = preview
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .map(|diagnostic| diagnostic.code.0.as_str())
            .collect();
        assert!(errors.is_empty(), "{blocks} blocks: {errors:?}");
        assert_eq!(preview.stage, Stage::Generate);
        assert!(preview.buildable, "{blocks} blocks");
        assert_eq!(preview.placeholders, 0);
        let cpp = &preview.files.first().unwrap().contents;
        assert!(cpp.contains("value_0 += i_0;"), "{cpp:.2000}");
        assert!(cpp.contains("value_0 -= 1;"), "{cpp:.2000}");
        if drag_handle {
            assert!(cpp.contains("void dragMe()"), "{cpp:.2000}");
        }
    }
}

#[test]
fn the_1000_block_document_has_no_diagnostics_at_all() {
    let generated = generate(shape(1_000, false)).unwrap();
    let preview = preview_document(&generated.to_string(), &PreviewOptions::default());
    assert!(preview.diagnostics.is_empty(), "{:?}", preview.diagnostics);
}

#[test]
fn the_typescript_generator_gives_the_same_documents() {
    assert_eq!(
        parity_digest(&generate(shape(1_000, false)).unwrap()),
        DIGEST_1000
    );
    assert_eq!(
        parity_digest(&generate(shape(5_000, true)).unwrap()),
        DIGEST_5000_HANDLE
    );
}

#[test]
fn the_parity_digest_ignores_key_order_but_not_content() {
    let a: Value = serde_json::from_str(r#"{"b": [1, {"y": true, "x": "s"}], "a": null}"#).unwrap();
    let b: Value = serde_json::from_str(r#"{"a": null, "b": [1, {"x": "s", "y": true}]}"#).unwrap();
    let c: Value = serde_json::from_str(r#"{"a": null, "b": [{"x": "s", "y": true}, 1]}"#).unwrap();
    assert_eq!(parity_digest(&a), parity_digest(&b));
    assert_ne!(parity_digest(&a), parity_digest(&c));
    // `printf '%s' '{"a":null,"b":[1,{"x":"s","y":true}]}' | sha256sum`
    assert_eq!(
        parity_digest(&a),
        "704112698874bfb52b267e9cec3b2285641e983134d36cdf5f86e283eb7c737e"
    );
}
