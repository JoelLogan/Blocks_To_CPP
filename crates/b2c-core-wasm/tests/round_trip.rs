//! The editor's round trip (05 §5.6): `load()` hands the document to
//! JavaScript, which keeps it as `JSON.parse` values and sends it back with
//! `JSON.stringify` for `canonical()`, `preview()`, saves and builds. Every
//! document that loads must come back as the same canonical text and hash,
//! so a project is never dirty just for being opened and a save never
//! changes a define (and with it the program and the security hash).
//!
//! Natively, JavaScript's number model is emulated: every number becomes an
//! `f64`, and a whole one is written without a fraction. The WebAssembly
//! suite (`packages/b2c-core-wasm/test/wasm.test.ts`) runs the same round
//! trip through real `JSON.parse` and `JSON.stringify`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests fail by panicking"
)]

mod common;

use common::{codes, example, examples, parse, repo_root};
use proptest::prelude::*;
use serde_json::{Number, Value, json};

/// Every number as JavaScript holds it: an `f64`, written without a
/// fraction when it is whole (as `JSON.stringify` does below 10^21).
fn to_javascript_numbers(value: &mut Value) {
    match value {
        Value::Number(n) => {
            let x = n.as_f64().unwrap();
            #[allow(
                clippy::cast_possible_truncation,
                clippy::cast_precision_loss,
                reason = "whole values within the i64 range convert exactly"
            )]
            let whole = (x.fract() == 0.0 && x.abs() < i64::MAX as f64).then_some(x as i64);
            *n = match whole {
                Some(whole) => Number::from(whole),
                None => Number::from_f64(x).unwrap(),
            };
        }
        Value::Array(items) => items.iter_mut().for_each(to_javascript_numbers),
        Value::Object(map) => map.values_mut().for_each(to_javascript_numbers),
        Value::Null | Value::Bool(_) | Value::String(_) => {}
    }
}

/// `JSON.stringify(JSON.parse(text))`, by value.
fn through_javascript(text: &str) -> String {
    let mut value: Value = serde_json::from_str(text).unwrap();
    to_javascript_numbers(&mut value);
    value.to_string()
}

/// `canonical()` of a text that must load: (text, hash).
fn canonical(text: &str) -> (String, String) {
    let result = parse(&b2c_core_wasm::canonical(text));
    assert_eq!(result["ok"], true, "{}", result["diagnostics"]);
    (
        result["text"].as_str().unwrap().to_owned(),
        result["hash"].as_str().unwrap().to_owned(),
    )
}

/// Checks the round trip for a project text; `false` when it does not load.
fn survives_the_editor(name: &str, text: &str) -> bool {
    let loaded = parse(&b2c_core_wasm::load(text.as_bytes()));
    if loaded["ok"] != true {
        return false;
    }
    let original = canonical(text);
    let edited = canonical(&through_javascript(&loaded["document"].to_string()));
    assert!(original == edited, "{name}:\n{}\n---\n{}", original.0, edited.0);
    true
}

#[test]
fn examples_and_the_security_suite_survive_the_editor() {
    for example in examples() {
        assert!(
            survives_the_editor(&example.name, &example.text),
            "{}",
            example.name
        );
    }
    let mut loaded = 0;
    let dir = repo_root().join("tests/security/projects");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "b2c"))
        .collect();
    files.sort();
    for path in files {
        let bytes = std::fs::read(&path).unwrap();
        // Some files of the suite are not UTF-8 on purpose.
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        if survives_the_editor(&path.display().to_string(), &text) {
            loaded += 1;
        }
    }
    assert!(loaded >= 20, "only {loaded} files of the suite load");
}

/// `hello_world` with these defines and `x-ext`.
fn hello(defines: &Value, ext: &Value) -> String {
    let mut value: Value = serde_json::from_str(&example("hello_world")).unwrap();
    value["project"]["build"] = json!({"defines": defines});
    value["x-ext"] = ext.clone();
    value.to_string()
}

#[test]
fn numbers_javascript_would_change_are_refused() {
    // The review's reproducer: before, these loaded and the editor's copy
    // saved a different define and different metadata.
    for (defines, ext) in [
        (
            json!([{"name": "BIG", "value": {"int": 9_007_199_254_740_993_i64}}]),
            json!({}),
        ),
        (json!([]), json!({"c": 12_345_678_901_234_567_890_u64})),
        (json!([]), json!({"c": [1, {"d": 1e300}]})),
    ] {
        let result = parse(&b2c_core_wasm::load(hello(&defines, &ext).as_bytes()));
        assert_eq!(result["ok"], false);
        assert_eq!(codes(&result["diagnostics"]), ["B2C-E0112"]);
    }
}

#[test]
fn free_form_numbers_have_one_spelling() {
    let defines = json!([
        {"name": "TOP", "value": {"int": 9_007_199_254_740_991_i64}},
        {"name": "BOTTOM", "value": {"int": -9_007_199_254_740_991_i64}},
    ]);
    let ext = json!({"a": 1.0, "b": 1.5e-6, "c": 1e-7, "d": -2.5, "e": 2e3, "f": 0.1});
    let text = hello(&defines, &ext);
    assert!(survives_the_editor("constructed", &text));
    let (saved, _) = canonical(&text);
    for line in [
        "\"int\": 9007199254740991",
        "\"int\": -9007199254740991",
        "\"a\": 1,",
        "\"b\": 0.0000015,",
        "\"c\": 1e-7,",
        "\"d\": -2.5,",
        "\"e\": 2000,",
        "\"f\": 0.1",
    ] {
        assert!(saved.contains(line), "{line}\n{saved}");
    }
}

proptest! {
    /// Any number that loads in `x-ext` comes back the same through the
    /// editor.
    #[test]
    fn any_free_form_number_survives_the_editor(bits in any::<u64>(), whole in -1_000_000_i64..1_000_000) {
        let x = f64::from_bits(bits);
        prop_assume!(x.is_finite());
        let text = hello(&json!([]), &json!({"x": x, "whole": whole, "list": [x, -x]}));
        let loads = survives_the_editor("generated", &text);
        // Refused exactly when it is beyond ±(2^53 − 1).
        prop_assert_eq!(loads, x.abs() <= 9_007_199_254_740_991.0, "{:e}", x);
    }
}
