//! Conformance of the facade over the example projects: the preview of an
//! error-free project is exactly what a build generates (ADR-0003: no drift
//! between preview and build), and loading and canonical saving round-trip
//! every file byte for byte (05 §5.2).
//!
//! `tests/golden/<name>/main.cpp` is written by the build's golden tests
//! (`crates/b2c-build/tests/golden.rs`, through `b2c_build::run_frontend`),
//! so comparing with it checks the facade's re-composition of the pipeline
//! against the build's.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests fail by panicking"
)]

mod common;

use common::{codes, examples, file, has_errors, native_generation, parse, preview, repo_root};
use serde_json::Value;

#[test]
fn version_reports_the_crate_versions() {
    let version = b2c_core_wasm::version();
    assert_eq!(
        version,
        format!(
            r#"{{"app":"{}","catalog":"{}","formatVersion":1,"sourceMapVersion":1}}"#,
            env!("CARGO_PKG_VERSION"),
            b2c_catalog::CATALOG_VERSION
        )
    );
}

#[test]
fn preview_main_cpp_equals_the_golden_file() {
    let mut checked = 0;
    for example in examples() {
        let golden = repo_root()
            .join("tests/golden")
            .join(&example.name)
            .join("main.cpp");
        let Ok(expected) = std::fs::read_to_string(&golden) else {
            continue;
        };
        let result = preview(&example.text, 4);
        assert_eq!(result["stage"], "generate", "{}", example.name);
        assert!(
            !has_errors(&result["diagnostics"]),
            "{}: {}",
            example.name,
            result["diagnostics"]
        );
        assert_eq!(result["buildable"], true, "{}", example.name);
        assert_eq!(result["placeholders"], 0, "{}", example.name);
        let generated = file(&result, "main.cpp").unwrap_or_else(|| panic!("{}: no main.cpp", example.name));
        assert!(
            generated == expected,
            "{}: main.cpp differs from {}",
            example.name,
            golden.display()
        );
        checked += 1;
    }
    assert!(checked >= 15, "only {checked} examples have golden files");
}

#[test]
fn preview_files_and_source_map_equal_the_native_pipeline() {
    for example in examples() {
        for indent_width in [2, 4] {
            let result = preview(&example.text, indent_width);
            let native = native_generation(&example.text, indent_width);
            assert_eq!(
                result["files"],
                serde_json::to_value(&native.project.files).unwrap(),
                "{} at indent {indent_width}",
                example.name
            );
            assert_eq!(
                result["sourceMap"],
                serde_json::to_value(&native.project.source_map).unwrap(),
                "{} at indent {indent_width}",
                example.name
            );
            assert_eq!(result["placeholders"], native.placeholders, "{}", example.name);
        }
    }
}

#[test]
fn preview_reports_the_content_hash_and_empty_scope_data() {
    for example in examples() {
        let result = preview(&example.text, 4);
        let canonical = parse(&b2c_core_wasm::canonical(&example.text));
        assert_eq!(result["contentHash"], canonical["hash"], "{}", example.name);
        assert_eq!(result["blockTypes"], Value::Object(serde_json::Map::new()));
        assert_eq!(result["symbols"], Value::Array(Vec::new()));
    }
}

#[test]
fn canonical_of_load_is_the_file_byte_for_byte() {
    for example in examples() {
        let loaded = parse(&b2c_core_wasm::load(example.text.as_bytes()));
        assert_eq!(loaded["ok"], true, "{}", example.name);
        assert_eq!(loaded["diagnostics"], Value::Array(Vec::new()));
        // What the editor does: keep the loaded document, send it back as JSON.
        let document = serde_json::to_string(&loaded["document"]).unwrap();
        let canonical = parse(&b2c_core_wasm::canonical(&document));
        assert_eq!(canonical["ok"], true, "{}", example.name);
        assert!(
            canonical["text"] == example.text.as_str(),
            "{}: not byte-identical",
            example.name
        );
        let hash = canonical["hash"].as_str().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(
            hash.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        let direct = parse(&b2c_core_wasm::canonical(&example.text));
        assert_eq!(direct, canonical, "{}", example.name);
    }
}

#[test]
fn load_returns_the_document_in_canonical_key_order() {
    for example in examples() {
        let output = b2c_core_wasm::load(example.text.as_bytes());
        // The canonical file text without its formatting is exactly the
        // document part of the output.
        let compact: String = {
            let canonical = b2c_model::to_canonical_json(&b2c_model::load(example.text.as_bytes()).unwrap());
            let value: Value = serde_json::from_str(&canonical).unwrap();
            assert_eq!(
                value,
                serde_json::from_str::<Value>(&example.text).unwrap(),
                "{}",
                example.name
            );
            minify(&canonical)
        };
        assert_eq!(
            output,
            format!(r#"{{"ok":true,"document":{compact},"diagnostics":[]}}"#),
            "{}",
            example.name
        );
    }
}

/// Removes formatting whitespace from JSON text (outside strings).
fn minify(text: &str) -> String {
    let mut out = String::new();
    let (mut in_string, mut escaped) = (false, false);
    for c in text.chars() {
        if in_string {
            out.push(c);
            match (escaped, c) {
                (true, _) => escaped = false,
                (false, '\\') => escaped = true,
                (false, '"') => in_string = false,
                _ => {}
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if !c.is_ascii_whitespace() {
            out.push(c);
        }
    }
    out
}

/// The example with the top-level blocks of every module in another order.
fn shuffled(text: &str, rotate: usize) -> String {
    let mut value: Value = serde_json::from_str(text).unwrap();
    for module in value["modules"].as_array_mut().unwrap() {
        let blocks = module["workspace"]["blocks"].as_array_mut().unwrap();
        blocks.reverse();
        if !blocks.is_empty() {
            let by = rotate % blocks.len();
            blocks.rotate_left(by);
        }
    }
    serde_json::to_string(&value).unwrap()
}

#[test]
fn top_level_block_order_does_not_change_the_preview() {
    let mut reordered = 0;
    for example in examples() {
        let expected = b2c_core_wasm::preview(&example.text, r#"{"indentWidth":4}"#);
        for rotate in 0..3 {
            let text = shuffled(&example.text, rotate);
            let blocks_moved = text != example.text;
            let output = b2c_core_wasm::preview(&text, r#"{"indentWidth":4}"#);
            assert!(
                output == expected,
                "{} (rotation {rotate}): the preview changed",
                example.name
            );
            if blocks_moved {
                reordered += 1;
            }
        }
    }
    assert!(reordered > 0);
    // At least one example has several top-level blocks, so the order
    // really changed somewhere.
    assert!(examples().iter().any(|e| {
        let value: Value = serde_json::from_str(&e.text).unwrap();
        value["modules"][0]["workspace"]["blocks"]
            .as_array()
            .unwrap()
            .len()
            > 1
    }));
}

/// `hello_world` printing an expression that references a symbol that does
/// not exist.
fn dangling_reference() -> String {
    let text = std::fs::read_to_string(repo_root().join("examples/hello_world.b2c")).unwrap();
    let mut value: Value = serde_json::from_str(&text).unwrap();
    let print = &mut value["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"][0];
    print["inputs"]["ITEM0"] = serde_json::json!({ "expr": [{ "ref": "sym_gone" }] });
    serde_json::to_string_pretty(&value).unwrap()
}

#[test]
fn a_dangling_reference_still_previews_best_effort_cpp() {
    let result = preview(&dangling_reference(), 4);
    assert_eq!(result["stage"], "analyze");
    let found = codes(&result["diagnostics"]);
    assert!(found.contains(&String::from("B2C-E0201")), "{found:?}");
    assert!(!found.contains(&String::from("B2C-E0701")), "{found:?}");
    let main = file(&result, "main.cpp").unwrap();
    assert!(main.contains("/* error */"), "{main}");
    assert_eq!(result["buildable"], false);
    assert!(result["placeholders"].as_u64().unwrap() > 0);
    assert!(result["sourceMap"].is_object());
    assert!(result["contentHash"].is_string());
}

#[test]
fn a_catalog_error_still_previews_best_effort_cpp() {
    let text = std::fs::read_to_string(repo_root().join("examples/guessing_game.b2c")).unwrap();
    let mut value: Value = serde_json::from_str(&text).unwrap();
    // An unknown block type inside main: the catalog rejects it, and the rest
    // of the program is still generated.
    let body = value["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"]
        .as_array_mut()
        .unwrap();
    body.push(serde_json::json!({ "id": "b_unknown", "type": "pack.missing_block", "v": 1 }));
    let result = preview(&serde_json::to_string(&value).unwrap(), 4);
    assert_eq!(result["stage"], "resolve");
    let found = codes(&result["diagnostics"]);
    assert!(found.contains(&String::from("B2C-E0601")), "{found:?}");
    assert!(!found.contains(&String::from("B2C-E0701")), "{found:?}");
    assert_eq!(result["buildable"], false);
    let main = file(&result, "main.cpp").unwrap();
    assert!(main.contains("int main()"), "{main}");
}

#[test]
fn a_load_failure_gives_no_files() {
    let result = preview("{\"format\": \"blocks2cpp/project\"", 4);
    assert_eq!(result["stage"], "load");
    assert_eq!(result["files"], Value::Array(Vec::new()));
    assert_eq!(result["sourceMap"], Value::Null);
    assert_eq!(result["contentHash"], Value::Null);
    assert_eq!(result["buildable"], false);
    assert_eq!(result["placeholders"], 0);
    assert_eq!(codes(&result["diagnostics"]), ["B2C-E0103"]);
}

#[test]
fn invalid_preview_options_give_an_error_envelope() {
    let text = std::fs::read_to_string(repo_root().join("examples/hello_world.b2c")).unwrap();
    for options in [
        "",
        "{}",
        r#"{"indentWidth":3}"#,
        r#"{"indentWidth":4,"tabs":true}"#,
    ] {
        let result = parse(&b2c_core_wasm::preview(&text, options));
        assert_eq!(result["error"]["kind"], "invalidOptions", "{options:?}");
        assert!(
            result["error"]["message"]
                .as_str()
                .unwrap()
                .starts_with("the preview options")
        );
        assert_eq!(result.as_object().unwrap().len(), 1);
    }
}
