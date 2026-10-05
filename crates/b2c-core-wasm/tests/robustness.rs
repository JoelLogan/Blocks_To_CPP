//! Every export on hostile and broken input: the malicious-project suite
//! (spec 08 §8.12), random bytes, truncated and mutated projects, and deep
//! nesting. Each call must return well-formed compact JSON with the problems
//! as diagnostics; nothing may panic (in the editor a panic is a trap).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests fail by panicking"
)]

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;

use common::{codes, examples, is_unsafe_to_show, parse, preview, repo_root};
use proptest::prelude::*;
use proptest::sample::Index;
use serde_json::{Value, json};

/// A row of `tests/security/projects/README.md`.
struct Case {
    file: String,
    /// `None` when the loader accepts the file.
    loader: Option<BTreeSet<String>>,
    /// The codes resolve reports (empty for `clean`), for accepted files.
    resolve: BTreeSet<String>,
}

fn suite_dir() -> PathBuf {
    repo_root().join("tests/security/projects")
}

fn codes_in(cell: &str) -> BTreeSet<String> {
    cell.split('`')
        .filter(|piece| piece.starts_with("B2C-"))
        .map(str::to_owned)
        .collect()
}

/// The README table (the same parsing as `b2c-model`'s and `b2c-catalog`'s
/// security tests).
fn cases() -> Vec<Case> {
    let readme = std::fs::read_to_string(suite_dir().join("README.md")).unwrap();
    let cases: Vec<Case> = readme
        .lines()
        .filter(|line| line.starts_with("| `"))
        .map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            assert_eq!(cells.len(), 8, "a table row needs six cells: {line}");
            Case {
                file: cells[1].trim_matches('`').to_owned(),
                loader: (cells[4] != "accepted").then(|| codes_in(cells[4])),
                resolve: codes_in(cells[5]),
            }
        })
        .collect();
    assert!(cases.len() >= 60, "the README table looks truncated");
    cases
}

fn assert_text_is_safe(file: &str, what: &str, text: &str) {
    if let Some(c) = text.chars().find(|&c| is_unsafe_to_show(c)) {
        panic!("{file}: {what} shows the unsafe character U+{:04X}", u32::from(c));
    }
}

#[test]
fn the_malicious_project_suite_through_every_export() {
    for case in cases() {
        let bytes = std::fs::read(suite_dir().join(&case.file)).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        let loaded = parse(&b2c_core_wasm::load(&bytes));
        let result = preview(&text, 4);
        let canonical = parse(&b2c_core_wasm::canonical(&text));

        for diagnostic in result["diagnostics"].as_array().unwrap() {
            assert_text_is_safe(&case.file, "a message", diagnostic["message"].as_str().unwrap());
        }
        for generated in result["files"].as_array().unwrap() {
            assert_text_is_safe(
                &case.file,
                "the generated C++",
                generated["contents"].as_str().unwrap(),
            );
        }

        if let Some(expected) = &case.loader {
            assert_eq!(loaded["ok"], false, "{}", case.file);
            assert!(loaded.get("document").is_none(), "{}", case.file);
            let found: BTreeSet<String> = codes(&loaded["diagnostics"]).into_iter().collect();
            assert_eq!(&found, expected, "{}", case.file);
            // The string exports see the same text only when the file is
            // UTF-8 (a JavaScript string cannot hold invalid UTF-8).
            if std::str::from_utf8(&bytes).is_ok() {
                assert_eq!(result["stage"], "load", "{}", case.file);
                assert_eq!(result["files"], json!([]), "{}", case.file);
                assert_eq!(result["sourceMap"], Value::Null, "{}", case.file);
                assert_eq!(result["buildable"], false, "{}", case.file);
                assert_eq!(result["diagnostics"], loaded["diagnostics"], "{}", case.file);
                assert_eq!(canonical["ok"], false, "{}", case.file);
                assert_eq!(canonical["diagnostics"], loaded["diagnostics"], "{}", case.file);
            }
        } else {
            assert_eq!(loaded["ok"], true, "{}", case.file);
            // The editor's round trip keeps the document.
            let document = serde_json::to_string(&loaded["document"]).unwrap();
            let saved = parse(&b2c_core_wasm::canonical(&document));
            assert_eq!(saved["ok"], true, "{}", case.file);
            assert_eq!(saved["text"], canonical["text"], "{}", case.file);
            let reloaded = parse(&b2c_core_wasm::load(saved["text"].as_str().unwrap().as_bytes()));
            assert_eq!(reloaded["document"], loaded["document"], "{}", case.file);

            // Best-effort C++ even when the catalog or the analyser objects.
            assert!(!result["files"].as_array().unwrap().is_empty(), "{}", case.file);
            let found: BTreeSet<String> = codes(&result["diagnostics"]).into_iter().collect();
            assert!(found.is_superset(&case.resolve), "{}: {found:?}", case.file);
            let stage = result["stage"].as_str().unwrap();
            if case.resolve.is_empty() {
                assert!(matches!(stage, "analyze" | "generate"), "{}: {stage}", case.file);
            } else {
                assert_eq!(stage, "resolve", "{}", case.file);
                assert_eq!(result["buildable"], false, "{}", case.file);
            }
        }
    }
}

#[test]
fn oversized_input_is_refused_before_parsing() {
    let mut text = String::from("{\"format\": \"blocks2cpp/project\", \"pad\": \"");
    text.push_str(&"x".repeat(b2c_model::limits::MAX_FILE_BYTES));
    text.push_str("\"}");
    for diagnostics in [
        parse(&b2c_core_wasm::load(text.as_bytes()))["diagnostics"].clone(),
        parse(&b2c_core_wasm::canonical(&text))["diagnostics"].clone(),
        preview(&text, 2)["diagnostics"].clone(),
    ] {
        assert_eq!(codes(&diagnostics), ["B2C-E0101"]);
    }
}

#[test]
fn deep_nesting_is_refused_without_exhausting_the_stack() {
    for depth in [129, 1_000, 100_000] {
        for (open, close) in [("[", "]"), ("{\"a\":", "}")] {
            let text = format!("{}{}", open.repeat(depth), close.repeat(depth));
            let loaded = parse(&b2c_core_wasm::load(text.as_bytes()));
            assert_eq!(loaded["ok"], false);
            assert_eq!(codes(&loaded["diagnostics"]), ["B2C-E0104"], "depth {depth}");
            assert_eq!(parse(&b2c_core_wasm::canonical(&text))["ok"], false);
            assert_eq!(preview(&text, 4)["stage"], "load");
        }
    }
}

/// The number of generated cases; `PROPTEST_CASES` overrides it.
fn cases_count() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64)
}

/// Every export on the same input: each returns a well-formed result, and
/// all three agree on whether the input is a project.
fn check_all_exports(bytes: &[u8]) -> Result<(), TestCaseError> {
    let loaded = parse(&b2c_core_wasm::load(bytes));
    let text = String::from_utf8_lossy(bytes);
    let canonical = parse(&b2c_core_wasm::canonical(&text));
    let result = preview(&text, 2);
    let ok = loaded["ok"].as_bool().unwrap();
    prop_assert_eq!(ok, loaded.get("document").is_some());
    if ok {
        prop_assert_eq!(&canonical["ok"], &json!(true));
        prop_assert_ne!(&result["stage"], &json!("load"));
        prop_assert!(!result["files"].as_array().unwrap().is_empty());
    } else {
        prop_assert!(!loaded["diagnostics"].as_array().unwrap().is_empty());
        if std::str::from_utf8(bytes).is_ok() {
            prop_assert_eq!(&canonical["ok"], &json!(false));
            prop_assert_eq!(&result["stage"], &json!("load"));
            prop_assert_eq!(&result["files"], &json!([]));
        }
    }
    Ok(())
}

/// Where a node sits in a JSON tree.
#[derive(Debug, Clone)]
enum Step {
    Key(String),
    Item(usize),
}

/// The paths of every node in a tree (the root is the empty path).
fn paths(value: &Value) -> Vec<Vec<Step>> {
    let mut found = Vec::new();
    let mut stack = vec![(value, Vec::new())];
    while let Some((node, path)) = stack.pop() {
        match node {
            Value::Object(map) => {
                for (key, child) in map {
                    let mut next = path.clone();
                    next.push(Step::Key(key.clone()));
                    stack.push((child, next));
                }
            }
            Value::Array(items) => {
                for (index, child) in items.iter().enumerate() {
                    let mut next = path.clone();
                    next.push(Step::Item(index));
                    stack.push((child, next));
                }
            }
            _ => {}
        }
        found.push(path);
    }
    found
}

fn node<'a>(value: &'a Value, path: &[Step]) -> &'a Value {
    path.iter().fold(value, |node, step| match step {
        Step::Key(key) => &node[key.as_str()],
        Step::Item(index) => &node[*index],
    })
}

fn node_mut<'a>(value: &'a mut Value, path: &[Step]) -> &'a mut Value {
    path.iter().fold(value, |node, step| match step {
        Step::Key(key) => &mut node[key.as_str()],
        Step::Item(index) => &mut node[*index],
    })
}

/// Replacement values: wrong types, hostile text, other block types, huge
/// numbers and empty containers.
fn replacement() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        Just(json!(1e300)),
        Just(json!(u64::MAX)),
        "[a-z_.]{0,12}".prop_map(Value::String),
        prop::sample::select(vec![
            "program.main",
            "control.if",
            "control.repeat",
            "control.while",
            "var.declare",
            "var.set",
            "io.print",
            "io.ask",
            "math.number",
            "math.random_int",
            "func.define",
            "func.call",
            "logic.compare",
            "text.join",
            "pack.unknown",
        ])
        .prop_map(|t| json!(t)),
        Just(json!("\u{202e}\u{0}\u{1b}[31m")),
        Just(json!({"ref": "sym_gone"})),
        Just(json!({"expr": [{"op": "("}, {"num": "1"}]})),
        Just(json!([])),
        Just(json!({})),
    ];
    leaf.prop_recursive(2, 8, 3, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..3).prop_map(Value::Array),
            prop::collection::btree_map("[a-zA-Z_]{1,8}", inner, 0..3)
                .prop_map(|map| Value::Object(map.into_iter().collect())),
        ]
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases_count()))]

    #[test]
    fn random_bytes(bytes in prop::collection::vec(any::<u8>(), 0..512)) {
        check_all_exports(&bytes)?;
    }

    #[test]
    fn random_json_text(text in "[\\[\\]{}\":,0-9a-z \\\\]{0,256}") {
        check_all_exports(text.as_bytes())?;
    }

    #[test]
    fn truncated_projects(example in 0..15_usize, cut in any::<Index>()) {
        let all = examples();
        let text = &all[example % all.len()].text;
        let trimmed = text.trim_end();
        // A strict prefix of the JSON text is never a complete document.
        let end = cut.index(trimmed.len());
        let prefix = &text.as_bytes()[..end];
        check_all_exports(prefix)?;
        prop_assert_eq!(&parse(&b2c_core_wasm::load(prefix))["ok"], &json!(false));
    }

    #[test]
    fn mutated_projects(
        example in 0..15_usize,
        edits in prop::collection::vec((any::<Index>(), replacement()), 1..4),
    ) {
        let all = examples();
        let mut value: Value = serde_json::from_str(&all[example % all.len()].text).unwrap();
        for (at, new) in edits {
            let found = paths(&value);
            let path = at.get(&found).clone();
            *node_mut(&mut value, &path) = new;
        }
        let text = serde_json::to_string(&value).unwrap();
        check_all_exports(text.as_bytes())?;
        for indent in [2, 4] {
            let result = preview(&text, indent);
            for generated in result["files"].as_array().unwrap() {
                let contents = generated["contents"].as_str().unwrap();
                prop_assert!(contents.ends_with('\n'));
            }
            if result["buildable"] == json!(true) {
                prop_assert!(!common::has_errors(&result["diagnostics"]));
                prop_assert_eq!(&result["placeholders"], &json!(0));
            }
        }
    }

    /// Blocks turned into other catalog blocks, keeping their fields and
    /// inputs: the loader accepts the result, the catalog and the analyser
    /// object, and the preview must still generate code for all of it.
    #[test]
    fn retyped_blocks(
        example in 0..15_usize,
        edits in prop::collection::vec((any::<Index>(), any::<Index>(), any::<bool>()), 1..6),
    ) {
        let all = examples();
        let mut value: Value = serde_json::from_str(&all[example % all.len()].text).unwrap();
        let types: Vec<&String> = b2c_catalog::core_catalog().blocks.keys().collect();
        for (block, new_type, drop_inputs) in edits {
            // Block nodes: objects with an ID, a type and a version.
            let blocks: Vec<Vec<Step>> = paths(&value)
                .into_iter()
                .filter(|path| {
                    let node = node(&value, path);
                    ["id", "type", "v"].iter().all(|key| node.get(key).is_some())
                })
                .collect();
            let path = block.get(&blocks).clone();
            let node = node_mut(&mut value, &path);
            node["type"] = json!(new_type.get(&types).as_str());
            if drop_inputs && let Some(object) = node.as_object_mut() {
                object.remove("inputs");
                object.remove("extra");
            }
        }
        let text = serde_json::to_string(&value).unwrap();
        check_all_exports(text.as_bytes())?;
        let result = preview(&text, 4);
        prop_assert_ne!(&result["stage"], &json!("load"), "{}", result["diagnostics"]);
        prop_assert!(!result["files"].as_array().unwrap().is_empty());
        let found = codes(&result["diagnostics"]);
        if result["stage"] != json!("generate") {
            prop_assert!(!found.contains(&String::from("B2C-E0701")));
        }
    }

    #[test]
    fn preview_options_never_panic(options in "\\PC{0,64}") {
        let output = b2c_core_wasm::preview("{}", &options);
        let result = parse(&output);
        if result.get("error").is_some() {
            prop_assert_eq!(&result["error"]["kind"], &json!("invalidOptions"));
        } else {
            prop_assert_eq!(&result["stage"], &json!("load"));
        }
    }
}
