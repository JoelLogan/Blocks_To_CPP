//! Copy and paste through the exports (05 §5.12): `clipboard_make` builds a
//! validated payload and cuts the blocks' C++, and `paste_prepare` validates
//! a payload like a project file, gives its blocks fresh IDs and binds their
//! outside references again at the target (06 §6.14.11).
//!
//! The tests insert the prepared blocks into the document the way the
//! editor does and preview the result, so "re-bound" and "unresolved" are
//! checked against the analyser itself.

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

use b2c_core_wasm::{PreviewOptions, Session};
use b2c_model::IdSource as _;
use common::{codes, example, examples, file, is_unsafe_to_show, names, parse, preview, repo_root};
use proptest::prelude::*;
use proptest::sample::Index;
use serde_json::{Value, json};

// ---------------------------------------------------------------- helpers

fn make(document: &str, ids: &[&str]) -> Value {
    parse(&b2c_core_wasm::clipboard_make(
        document,
        &serde_json::to_string(ids).unwrap(),
    ))
}

/// `clipboard_make` that must succeed: (payload, text).
fn copy(document: &str, ids: &[&str]) -> (String, Option<String>) {
    let made = make(document, ids);
    assert_eq!(made["ok"], true, "{made}");
    assert_eq!(made["diagnostics"], json!([]));
    (
        made["payload"].as_str().unwrap().to_owned(),
        made.get("text").map(|t| t.as_str().unwrap().to_owned()),
    )
}

fn seed(n: u64) -> String {
    format!("{n:064x}")
}

fn target(module: &str, block: Option<&str>, input: Option<&str>) -> String {
    json!({"module": module, "block": block, "input": input}).to_string()
}

fn paste(payload: &str, document: &str, target: &str, seed_hex: &str) -> Value {
    parse(&b2c_core_wasm::paste_prepare(payload, document, target, seed_hex))
}

/// `paste_prepare` that must succeed.
fn paste_ok(payload: &str, document: &str, target: &str, n: u64) -> Value {
    let pasted = paste(payload, document, target, &seed(n));
    assert_eq!(pasted["ok"], true, "{pasted}");
    pasted
}

/// The block node with this ID anywhere in a JSON tree, mutably.
fn find_mut<'a>(value: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    let is_it = value.get("id").and_then(Value::as_str) == Some(id) && value.get("type").is_some();
    if is_it {
        return Some(value);
    }
    match value {
        Value::Object(map) => map.values_mut().find_map(|v| find_mut(v, id)),
        Value::Array(items) => items.iter_mut().find_map(|v| find_mut(v, id)),
        _ => None,
    }
}

/// The statement list that holds the block with this ID, and its index.
fn list_holding<'a>(value: &'a mut Value, id: &str) -> Option<(&'a mut Vec<Value>, usize)> {
    match value {
        Value::Array(items) => {
            if let Some(at) = items
                .iter()
                .position(|item| item.get("id").and_then(Value::as_str) == Some(id))
            {
                return Some((items, at));
            }
            items.iter_mut().find_map(|v| list_holding(v, id))
        }
        Value::Object(map) => map.values_mut().find_map(|v| list_holding(v, id)),
        _ => None,
    }
}

/// Inserts pasted blocks where `target` says, as the editor would.
fn insert(
    document: &str,
    module: usize,
    target_block: Option<&str>,
    input: Option<&str>,
    blocks: &Value,
) -> String {
    let mut value: Value = serde_json::from_str(document).unwrap();
    let blocks = blocks.as_array().unwrap().clone();
    match (target_block, input) {
        (None, _) => {
            let canvas = value["modules"][module]["workspace"]["blocks"]
                .as_array_mut()
                .unwrap();
            for (i, mut block) in blocks.into_iter().enumerate() {
                block["x"] = json!(1000 + i);
                block["y"] = json!(1000);
                canvas.push(block);
            }
        }
        (Some(block), Some(list)) => {
            let node = find_mut(&mut value, block).unwrap();
            let items = node["statements"][list].as_array_mut().unwrap();
            for (i, block) in blocks.into_iter().enumerate() {
                items.insert(i, block);
            }
        }
        (Some(block), None) => {
            let (items, at) = list_holding(&mut value, block).unwrap();
            for (i, block) in blocks.into_iter().enumerate() {
                items.insert(at + 1 + i, block);
            }
        }
    }
    serde_json::to_string_pretty(&value).unwrap()
}

/// Every block ID and declared symbol ID in a JSON tree.
fn used_ids(value: &Value) -> BTreeSet<String> {
    let document = b2c_model::load(value.to_string().as_bytes());
    match document {
        Ok(document) => b2c_model::used_ids(&document),
        Err(error) => panic!("not a project: {:?}", error.diagnostics),
    }
}

fn block_ids(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let (Some(Value::String(id)), Some(_)) = (map.get("id"), map.get("type")) {
                out.push(id.clone());
            }
            map.values().for_each(|v| block_ids(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| block_ids(v, out)),
        _ => {}
    }
}

/// `hello_world` with `create int guess = 0` and `create int secret = 5`
/// (other symbol IDs than the guessing game's) before its print block.
fn other_document() -> String {
    let mut value: Value = serde_json::from_str(&example("hello_world")).unwrap();
    let body = value["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"]
        .as_array_mut()
        .unwrap();
    for (at, (id, sym, name, number)) in [("c1", "t_guess", "guess", "0"), ("c2", "t_secret", "secret", "5")]
        .into_iter()
        .enumerate()
    {
        body.insert(
            at,
            json!({"id": id, "type": "var.declare", "v": 1,
                   "fields": {"CONST": false, "NAME": {"sym": sym, "name": name}, "TYPE": "int"},
                   "inputs": {"VALUE": {"expr": [{"num": number}]}}}),
        );
    }
    serde_json::to_string_pretty(&value).unwrap()
}

fn errors(result: &Value) -> Vec<String> {
    result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["severity"] == "error")
        .map(|d| {
            format!(
                "{} {}",
                d["code"].as_str().unwrap(),
                d["message"].as_str().unwrap()
            )
        })
        .collect()
}

// ------------------------------------------------------------------ copy

#[test]
fn a_copy_holds_the_blocks_their_outside_references_and_their_code() {
    let game = example("guessing_game");
    preview(&game, 4);
    let (payload, text) = copy(&game, &["b009"]);
    let clipboard = b2c_model::load_clipboard(payload.as_bytes()).unwrap();
    assert_eq!(
        b2c_model::to_canonical_clipboard_json(&clipboard),
        payload,
        "canonical"
    );
    assert_eq!(clipboard.catalog, b2c_catalog::CATALOG_VERSION);
    assert_eq!(clipboard.blocks.len(), 1);
    assert_eq!(clipboard.blocks[0].id.as_str(), "b009");
    let refs: Vec<(String, String)> = clipboard
        .refs
        .iter()
        .map(|(sym, r)| (sym.to_string(), format!("{} {:?}", r.name, r.kind)))
        .collect();
    assert_eq!(
        refs,
        [
            (String::from("s_guess"), String::from("guess Variable")),
            (String::from("s_secret"), String::from("secret Variable")),
        ]
    );
    // The `if` with its branches, as whole lines without the indentation.
    let text = text.unwrap();
    assert!(text.starts_with("if (guess < secret) {\n"), "{text}");
    assert!(text.ends_with("}\n"), "{text}");
    assert!(text.contains("\n    std::cout << \"Too low!\""), "{text}");
    assert!(!text.starts_with(' '));
    let main = file(&preview(&game, 4), "main.cpp").unwrap().to_owned();
    for line in text.lines() {
        assert!(main.contains(line.trim()), "{line}");
    }
}

#[test]
fn a_copied_value_block_is_its_expression_and_a_function_its_definition() {
    let game = example("guessing_game");
    let (_, text) = copy(&game, &["b001"]);
    let text = text.unwrap();
    assert_eq!(text.lines().count(), 1, "{text}");
    assert!(text.contains("100"), "{text}");
    assert!(
        file(&preview(&game, 4), "main.cpp")
            .unwrap()
            .contains(text.trim_end())
    );

    let factorial = example("factorial");
    let (payload, text) = copy(&factorial, &["b004"]);
    let text = text.unwrap();
    // The definition with its comment, not the forward declaration.
    assert!(text.starts_with("// n! = "), "{text}");
    assert!(text.contains("\nint factorial(int n) {\n"), "{text}");
    assert!(!text.contains("int factorial(int n);"), "{text}");
    assert!(text.ends_with("}\n"));
    // A whole function declares everything it uses.
    assert!(
        b2c_model::load_clipboard(payload.as_bytes())
            .unwrap()
            .refs
            .is_empty()
    );
    // Several blocks: in the given order.
    let (_, both) = copy(&factorial, &["b005", "b004"]);
    let both = both.unwrap();
    assert!(both.starts_with("factorial(i)\n// n! = "), "{both}");
}

#[test]
fn function_references_are_recorded_with_the_global_qualifier() {
    let factorial = example("factorial");
    let (payload, _) = copy(&factorial, &["b006"]);
    let clipboard = b2c_model::load_clipboard(payload.as_bytes()).unwrap();
    let refs: Vec<String> = clipboard
        .refs
        .values()
        .map(|r| format!("{} {:?}", r.name, r.kind))
        .collect();
    assert_eq!(refs, ["::factorial Function", "i LoopVariable"]);
    let (payload, _) = copy(&factorial, &["b003"]);
    let clipboard = b2c_model::load_clipboard(payload.as_bytes()).unwrap();
    let refs: Vec<String> = clipboard
        .refs
        .values()
        .map(|r| format!("{} {:?}", r.name, r.kind))
        .collect();
    assert_eq!(refs, ["::factorial Function", "n Parameter"]);
}

#[test]
fn nested_selections_are_copied_once_in_the_given_order() {
    let game = example("guessing_game");
    let (payload, _) = copy(&game, &["b006", "b010", "b004"]);
    let clipboard = b2c_model::load_clipboard(payload.as_bytes()).unwrap();
    let ids: Vec<&str> = clipboard.blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["b010", "b004"], "b006 is inside b010");
    // A copy of main holds everything, and has no position.
    let (payload, _) = copy(&game, &["b011"]);
    assert!(!payload.contains("\"x\""));
    let clipboard = b2c_model::load_clipboard(payload.as_bytes()).unwrap();
    assert!(clipboard.refs.is_empty());
    let mut ids = Vec::new();
    block_ids(&serde_json::from_str(&payload).unwrap(), &mut ids);
    assert_eq!(ids.len(), 11);
    // An empty selection is an empty payload.
    let (payload, text) = copy(&game, &[]);
    assert!(
        b2c_model::load_clipboard(payload.as_bytes())
            .unwrap()
            .blocks
            .is_empty()
    );
    assert_eq!(text, None);
}

#[test]
fn a_loose_stack_is_copied_with_its_head_and_has_no_code() {
    let mut value: Value = serde_json::from_str(&example("guessing_game")).unwrap();
    value["modules"][0]["workspace"]["blocks"].as_array_mut().unwrap().push(json!(
        {"id": "loose", "type": "io.print", "v": 1, "x": 500, "y": 40,
         "extra": {"itemCount": 1}, "fields": {"NEWLINE": true, "SEP": "none", "STREAM": "out"},
         "inputs": {"ITEM0": {"expr": [{"ref": "s_guess"}]}},
         "stack": [{"id": "below", "type": "io.print", "v": 1,
                    "extra": {"itemCount": 1}, "fields": {"NEWLINE": true, "SEP": "none", "STREAM": "out"},
                    "inputs": {"ITEM0": {"expr": [{"str": "x"}]}}}]}
    ));
    let document = value.to_string();
    let (payload, text) = copy(&document, &["loose"]);
    assert_eq!(text, None, "loose blocks are not part of the program");
    let clipboard = b2c_model::load_clipboard(payload.as_bytes()).unwrap();
    assert_eq!(clipboard.blocks[0].stack.len(), 1);
    assert_eq!(clipboard.refs.len(), 1);
    // A stacked block alone, and with its head (copied once).
    let (payload, _) = copy(&document, &["below"]);
    assert!(
        b2c_model::load_clipboard(payload.as_bytes()).unwrap().blocks[0]
            .stack
            .is_empty()
    );
    let (payload, _) = copy(&document, &["loose", "below"]);
    assert_eq!(
        b2c_model::load_clipboard(payload.as_bytes())
            .unwrap()
            .blocks
            .len(),
        1
    );
}

#[test]
fn a_reference_whose_declaration_is_gone_gets_no_entry() {
    let mut value: Value = serde_json::from_str(&example("hello_world")).unwrap();
    value["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"][0]["inputs"]["ITEM0"] =
        json!({"expr": [{"ref": "sym_gone"}]});
    let (payload, _) = copy(&value.to_string(), &["b001"]);
    assert!(
        b2c_model::load_clipboard(payload.as_bytes())
            .unwrap()
            .refs
            .is_empty()
    );
}

#[test]
fn the_code_comes_from_the_last_preview_or_is_generated() {
    let game = example("guessing_game");
    let mut session = Session::new();
    let ids = [b2c_ir::BlockId::new("b009").unwrap()];
    // No preview yet: generated at the default indent width.
    let four = session.clipboard_make(&game, &ids).unwrap().text.unwrap();
    assert!(four.contains("\n    std::cout"), "{four}");
    // After a preview at width 2, from that preview.
    session.preview(
        &game,
        &PreviewOptions {
            indent_width: b2c_core_wasm::IndentWidth::Two,
        },
    );
    let two = session.clipboard_make(&game, &ids).unwrap().text.unwrap();
    assert!(two.contains("\n  std::cout"), "{two}");
    assert!(!two.contains("\n    std::cout"), "{two}");
    // Another document: generated, at the last preview's width.
    let mut value: Value = serde_json::from_str(&game).unwrap();
    let print = find_mut(&mut value, "b006").unwrap();
    print["inputs"]["ITEM0"] = json!({"expr": [{"str": "Higher!"}]});
    let other = session
        .clipboard_make(&value.to_string(), &ids)
        .unwrap()
        .text
        .unwrap();
    assert!(other.contains("\n  std::cout << \"Higher!\""), "{other}");
    // Layout-only changes keep the content hash: the kept analysis is used.
    let mut moved: Value = serde_json::from_str(&game).unwrap();
    moved["modules"][0]["workspace"]["blocks"][0]["x"] = json!(400);
    assert_eq!(
        session
            .clipboard_make(&moved.to_string(), &ids)
            .unwrap()
            .text
            .unwrap(),
        two
    );
}

#[test]
fn disabled_blocks_are_copied_without_code() {
    let mut value: Value = serde_json::from_str(&example("guessing_game")).unwrap();
    find_mut(&mut value, "b004").unwrap()["disabled"] = json!(true);
    let (payload, text) = copy(&value.to_string(), &["b004"]);
    assert_eq!(text, None);
    assert!(payload.contains("\"disabled\": true"));
}

#[test]
fn copy_refuses_bad_arguments_and_reports_bad_documents() {
    let game = example("guessing_game");
    for ids in [
        "",
        "{}",
        "[1]",
        r#"["b-1"]"#,
        r#"["b005", "b005"]"#,
        r#"["nope"]"#,
    ] {
        let result = parse(&b2c_core_wasm::clipboard_make(&game, ids));
        assert_eq!(result["error"]["kind"], "invalidArguments", "{ids}");
        assert_eq!(result.as_object().unwrap().len(), 1);
    }
    let unknown = parse(&b2c_core_wasm::clipboard_make(&game, r#"["nope"]"#));
    assert!(
        unknown["error"]["message"]
            .as_str()
            .unwrap()
            .contains("nope is not in the document")
    );
    let broken = make("{", &["b005"]);
    assert_eq!(broken["ok"], false);
    assert_eq!(codes(&broken["diagnostics"]), ["B2C-E0103"]);
    assert!(broken.get("payload").is_none());
}

// ----------------------------------------------------------------- paste

#[test]
fn a_paste_next_to_the_original_keeps_its_references() {
    let game = example("guessing_game");
    preview(&game, 4);
    let (payload, _) = copy(&game, &["b009"]);
    let pasted = paste_ok(&payload, &game, &target("mod_main", Some("b005"), None), 1);
    assert_eq!(pasted["unresolved"], json!([]));
    assert_eq!(pasted["diagnostics"], json!([]));
    let blocks = &pasted["blocks"];
    assert_eq!(blocks.as_array().unwrap().len(), 1);
    let mut ids = Vec::new();
    block_ids(blocks, &mut ids);
    assert_eq!(ids.len(), 4);
    let before = used_ids(&serde_json::from_str(&game).unwrap());
    for id in &ids {
        assert!(id.starts_with("blk_") && id.len() == 21, "{id}");
        assert!(!before.contains(id), "{id}");
    }
    assert!(blocks[0].get("x").is_none());
    let text = blocks.to_string();
    assert!(text.contains("s_guess") && text.contains("s_secret"));

    let after = insert(&game, 0, Some("b005"), None, blocks);
    let result = preview(&after, 4);
    assert_eq!(result["stage"], "generate", "{:?}", errors(&result));
    assert_eq!(result["buildable"], true);
    assert_eq!(file(&result, "main.cpp").unwrap().matches("Too low!").count(), 2);
}

#[test]
fn a_paste_into_another_document_binds_references_by_name() {
    let game = example("guessing_game");
    let other = other_document();
    let (payload, _) = copy(&game, &["b009"]);
    // After `create secret`, both names are visible (other symbol IDs).
    let pasted = paste_ok(&payload, &other, &target("mod_main", Some("c2"), None), 2);
    assert_eq!(pasted["unresolved"], json!([]));
    let text = pasted["blocks"].to_string();
    assert!(text.contains("t_guess") && text.contains("t_secret"), "{text}");
    assert!(!text.contains("s_guess") && !text.contains("s_secret"), "{text}");
    let result = preview(&insert(&other, 0, Some("c2"), None, &pasted["blocks"]), 4);
    assert_eq!(result["buildable"], true, "{:?}", errors(&result));

    // At the start of main's list nothing is declared yet: both stay
    // unresolved, each with an E0201 naming the original.
    let pasted = paste_ok(
        &payload,
        &other,
        &target("mod_main", Some("b002"), Some("BODY")),
        3,
    );
    assert_eq!(
        pasted["unresolved"],
        json!([{"sym": "s_guess", "name": "guess"}, {"sym": "s_secret", "name": "secret"}])
    );
    let diagnostics = pasted["diagnostics"].as_array().unwrap();
    assert_eq!(codes(&pasted["diagnostics"]), ["B2C-E0201", "B2C-E0201"]);
    let new_if = pasted["blocks"][0]["id"].as_str().unwrap();
    for (diagnostic, name) in diagnostics.iter().zip(["guess", "secret"]) {
        assert_eq!(diagnostic["severity"], "error");
        assert_eq!(diagnostic["source"], "analyser");
        assert!(
            diagnostic["message"]
                .as_str()
                .unwrap()
                .contains(&format!("`{name}`"))
        );
        assert_eq!(diagnostic["primary"]["module"], "mod_main");
        assert_eq!(diagnostic["primary"]["block"], new_if);
    }
    assert_eq!(
        diagnostics[0]["primary"]["part"],
        json!({"kind": "tokens", "input": "COND0", "start": 0, "end": 1})
    );
    assert_eq!(
        diagnostics[1]["primary"]["part"],
        json!({"kind": "tokens", "input": "COND0", "start": 2, "end": 3})
    );
    // The analyser agrees once the blocks are in place.
    let result = preview(
        &insert(&other, 0, Some("b002"), Some("BODY"), &pasted["blocks"]),
        4,
    );
    assert!(codes(&result["diagnostics"]).contains(&String::from("B2C-E0201")));
    assert_eq!(result["buildable"], false);
}

#[test]
fn the_target_decides_what_is_visible() {
    let game = example("guessing_game");
    let other = other_document();
    let (payload, _) = copy(&game, &["b005"]); // ask (guess)
    let unresolved = |target: &str| paste_ok(&payload, &other, target, 4)["unresolved"].clone();
    // Directly after `create guess`: its own variable is visible.
    assert_eq!(unresolved(&target("mod_main", Some("c1"), None)), json!([]));
    // Inside its starting value it is not.
    assert_eq!(
        unresolved(&target("mod_main", Some("c1"), Some("VALUE"))),
        json!([{"sym": "s_guess", "name": "guess"}])
    );
    // At the block: only what is declared before it.
    assert_eq!(
        unresolved(&target("mod_main", Some("c1"), Some("NOT_A_LIST"))),
        json!([{"sym": "s_guess", "name": "guess"}])
    );
    // On the canvas only functions are visible.
    assert_eq!(
        unresolved(&target("mod_main", None, None)),
        json!([{"sym": "s_guess", "name": "guess"}])
    );
}

#[test]
fn a_paste_after_a_disabled_statement_sees_its_position() {
    // 05 §5.12: scope is evaluated at the target. A disabled statement is
    // not analysed, but the place after it is in a list that is.
    let game = example("guessing_game");
    let (payload, _) = copy(&game, &["b009"]); // if guess < secret …
    let mut value: Value = serde_json::from_str(&game).unwrap();
    find_mut(&mut value, "b004").unwrap()["disabled"] = json!(true);
    let disabled = value.to_string();
    let after = target("mod_main", Some("b004"), None);

    // In the same document: the references keep their symbols, with no
    // diagnostic, and the analyser agrees once the blocks are in place.
    preview(&disabled, 4);
    let pasted = paste_ok(&payload, &disabled, &after, 21);
    assert_eq!(pasted["unresolved"], json!([]));
    assert_eq!(pasted["diagnostics"], json!([]));
    let result = preview(&insert(&disabled, 0, Some("b004"), None, &pasted["blocks"]), 4);
    assert_eq!(result["buildable"], true, "{:?}", errors(&result));
    // Into one of its value inputs, the same.
    let inside = paste_ok(
        &payload,
        &disabled,
        &target("mod_main", Some("b004"), Some("ITEM0")),
        22,
    );
    assert_eq!(inside["unresolved"], json!([]));
    // Its own dropdowns are filled: what is declared before it.
    preview(&disabled, 4);
    let here: Value = serde_json::from_str(&b2c_core_wasm::symbols_in_scope("b004", None)).unwrap();
    assert_eq!(names(&here), ["guess", "secret"]);

    // Into another document whose symbols have other IDs: bound by name.
    let renamed = disabled
        .replace("s_guess", "t_guess")
        .replace("s_secret", "t_secret");
    let pasted = paste_ok(&payload, &renamed, &after, 23);
    assert_eq!(pasted["unresolved"], json!([]));
    let text = pasted["blocks"].to_string();
    assert!(text.contains("t_guess") && text.contains("t_secret"), "{text}");
    assert!(!text.contains("s_guess") && !text.contains("s_secret"), "{text}");

    // A block inside a disabled block is still not reached: its place sees
    // what the canvas sees (no functions here).
    let mut value: Value = serde_json::from_str(&renamed).unwrap();
    find_mut(&mut value, "b010").unwrap()["disabled"] = json!(true);
    let pasted = paste_ok(
        &payload,
        &value.to_string(),
        &target("mod_main", Some("b005"), None),
        24,
    );
    assert_eq!(
        pasted["unresolved"],
        json!([{"sym": "s_guess", "name": "guess"}, {"sym": "s_secret", "name": "secret"}])
    );
}

#[test]
fn a_renamed_original_is_kept_where_it_is_visible() {
    // 05 §5.12, 06 §6.14.11: when no visible symbol has the recorded name, a
    // reference whose own symbol is visible there with the same kind keeps
    // it, even though it was renamed since the copy.
    let game = example("guessing_game");
    let (payload, _) = copy(&game, &["b009"]);
    let tries = game.replace("\"name\": \"guess\"", "\"name\": \"tries\"");
    assert_ne!(tries, game);
    let at = target("mod_main", Some("b005"), None);
    let pasted = paste_ok(&payload, &tries, &at, 25);
    assert_eq!(pasted["unresolved"], json!([]));
    assert_eq!(pasted["diagnostics"], json!([]));
    assert!(pasted["blocks"].to_string().contains("s_guess"));
    let result = preview(&insert(&tries, 0, Some("b005"), None, &pasted["blocks"]), 4);
    assert_eq!(result["buildable"], true, "{:?}", errors(&result));
    assert!(
        file(&result, "main.cpp")
            .unwrap()
            .matches("tries < secret")
            .count()
            >= 2
    );

    // Projects made from the same example share symbol IDs: a paste binds
    // the same way, silently, to whatever that symbol is called there.
    let other = other_document()
        .replace("t_guess", "s_guess")
        .replace("\"name\": \"guess\"", "\"name\": \"attempt\"");
    let pasted = paste_ok(&payload, &other, &target("mod_main", Some("c2"), None), 26);
    assert_eq!(pasted["unresolved"], json!([]));
    assert_eq!(pasted["diagnostics"], json!([]));
    let text = pasted["blocks"].to_string();
    assert!(text.contains("s_guess") && text.contains("t_secret"), "{text}");

    // Another visible symbol with the recorded name wins over the original.
    let mut value: Value = serde_json::from_str(&tries).unwrap();
    let (items, at_b004) = list_holding(&mut value, "b004").unwrap();
    items.insert(
        at_b004,
        json!({"id": "c9", "type": "var.declare", "v": 1,
               "fields": {"CONST": false, "NAME": {"sym": "t_guess", "name": "guess"}, "TYPE": "int"},
               "inputs": {"VALUE": {"expr": [{"num": "1"}]}}}),
    );
    let both = value.to_string();
    let pasted = paste_ok(&payload, &both, &at, 27);
    assert_eq!(pasted["unresolved"], json!([]));
    let text = pasted["blocks"].to_string();
    assert!(text.contains("t_guess") && !text.contains("s_guess"), "{text}");
}

#[test]
fn a_pasted_stack_is_unstacked_inside_a_statement_list() {
    // A copied loose stack is one block with `stack` (ADR-0011). Inside a
    // statement list (or after a block), the stacked blocks follow their
    // head, since a nested block cannot have a `stack` (B2C-E0139).
    let mut value: Value = serde_json::from_str(&example("guessing_game")).unwrap();
    let print = |id: &str, text: &str| {
        json!({"id": id, "type": "io.print", "v": 1, "extra": {"itemCount": 1},
               "fields": {"NEWLINE": true, "SEP": "none", "STREAM": "out"},
               "inputs": {"ITEM0": {"expr": [{"str": text}]}}})
    };
    let mut head = print("L1", "one");
    head["x"] = json!(500);
    head["y"] = json!(40);
    head["stack"] = json!([print("L2", "two"), print("L3", "three")]);
    value["modules"][0]["workspace"]["blocks"]
        .as_array_mut()
        .unwrap()
        .push(head);
    let document = value.to_string();
    let (payload, _) = copy(&document, &["L1"]);
    // The loose stack itself is an error (B2C-E0604); the pastes add none.
    let before = errors(&preview(&document, 4));
    assert_eq!(before.len(), 1, "{before:?}");

    for (block, input) in [(Some("b011"), Some("BODY")), (Some("b004"), None)] {
        let pasted = paste_ok(&payload, &document, &target("mod_main", block, input), 28);
        let blocks = pasted["blocks"].as_array().unwrap();
        assert_eq!(blocks.len(), 3, "{pasted}");
        assert!(blocks.iter().all(|b| b.get("stack").is_none()), "{pasted}");
        let texts: Vec<&str> = blocks
            .iter()
            .map(|b| b["inputs"]["ITEM0"]["expr"][0]["str"].as_str().unwrap())
            .collect();
        assert_eq!(texts, ["one", "two", "three"]);
        let after = insert(&document, 0, block, input, &pasted["blocks"]);
        let loaded = parse(&b2c_core_wasm::load(after.as_bytes()));
        assert_eq!(loaded["ok"], true, "{}", loaded["diagnostics"]);
        let result = preview(&after, 4);
        assert_eq!(errors(&result), before);
        let main = file(&result, "main.cpp").unwrap();
        let (one, two, three) = (
            main.find("\"one\"").unwrap(),
            main.find("\"two\"").unwrap(),
            main.find("\"three\"").unwrap(),
        );
        assert!(one < two && two < three, "{main}");
    }

    // On the canvas the stack stays one block with `stack`.
    let pasted = paste_ok(&payload, &document, &target("mod_main", None, None), 29);
    let blocks = pasted["blocks"].as_array().unwrap();
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["stack"].as_array().unwrap().len(), 2);
    let after = insert(&document, 0, None, None, &pasted["blocks"]);
    assert_eq!(parse(&b2c_core_wasm::load(after.as_bytes()))["ok"], true);
}

#[test]
fn function_calls_bind_to_the_target_modules_function() {
    let factorial = example("factorial");
    let (payload, _) = copy(&factorial, &["b006"]); // print i, factorial(i)
    // Into a loop with a counter `i` and a function `factorial` of other IDs.
    let mut value: Value = serde_json::from_str(&factorial).unwrap();
    let text = value
        .to_string()
        .replace("s_factorial", "t_fact")
        .replace("s_i", "t_i");
    value = serde_json::from_str(&text).unwrap();
    let document = value.to_string();
    let pasted = paste_ok(
        &payload,
        &document,
        &target("mod_main", Some("b007"), Some("BODY")),
        5,
    );
    assert_eq!(pasted["unresolved"], json!([]));
    let blocks = pasted["blocks"].to_string();
    assert!(blocks.contains("t_fact") && blocks.contains("t_i"), "{blocks}");
    // Outside the loop the counter is not visible; the function still is.
    let pasted = paste_ok(&payload, &document, &target("mod_main", Some("b007"), None), 6);
    assert_eq!(pasted["unresolved"], json!([{"sym": "s_i", "name": "i"}]));
    assert!(pasted["blocks"].to_string().contains("t_fact"));
    // In a document without the function, it is unresolved by its name.
    let pasted = paste_ok(
        &payload,
        &example("hello_world"),
        &target("mod_main", None, None),
        7,
    );
    let names: Vec<&str> = pasted["unresolved"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["::factorial", "i"]);
    assert!(
        pasted["diagnostics"][0]["message"]
            .as_str()
            .unwrap()
            .contains("function `::factorial`")
    );
}

#[test]
fn a_pasted_function_gets_fresh_symbols_for_everything_it_declares() {
    let factorial = example("factorial");
    let (payload, _) = copy(&factorial, &["b004"]);
    let pasted = paste_ok(&payload, &factorial, &target("mod_main", None, None), 8);
    assert_eq!(pasted["unresolved"], json!([]));
    let function = &pasted["blocks"][0];
    let name = function["fields"]["NAME"]["sym"].as_str().unwrap();
    let param = function["extra"]["params"][0]["sym"].as_str().unwrap();
    assert!(name.starts_with("sym_") && param.starts_with("sym_"));
    let text = function.to_string();
    // The recursive call and the parameter uses follow the new IDs.
    assert!(
        !text.contains("s_factorial") && !text.contains("\"s_n\""),
        "{text}"
    );
    assert!(text.matches(name).count() >= 2);
    assert!(text.matches(param).count() >= 3);
    // The document with both functions loads (the analyser reports the
    // duplicate name, not duplicate IDs).
    let after = insert(&factorial, 0, None, None, &pasted["blocks"]);
    assert_eq!(parse(&b2c_core_wasm::load(after.as_bytes()))["ok"], true);
}

#[test]
fn fresh_ids_never_collide_with_the_document() {
    // The first IDs the seed would give are already in the document.
    let mut source = b2c_model::SeededIds::new([7; 32]);
    let taken: Vec<String> = (0..3).map(|_| source.next_id(b2c_model::IdKind::Block)).collect();
    let mut value: Value = serde_json::from_str(&example("guessing_game")).unwrap();
    for (i, id) in taken.iter().enumerate() {
        value["modules"][0]["workspace"]["blocks"]
            .as_array_mut()
            .unwrap()
            .push(json!(
                {"id": id, "type": "io.print", "v": 1, "x": 0, "y": 100 * i,
                 "extra": {"itemCount": 1}, "fields": {"NEWLINE": true, "SEP": "none", "STREAM": "out"},
                 "inputs": {"ITEM0": {"expr": [{"str": "x"}]}}}
            ));
    }
    let document = value.to_string();
    let (payload, _) = copy(&document, &["b009"]);
    let pasted = paste(
        &payload,
        &document,
        &target("mod_main", None, None),
        &"07".repeat(32),
    );
    assert_eq!(pasted["ok"], true);
    let mut ids = Vec::new();
    block_ids(&pasted["blocks"], &mut ids);
    assert_eq!(ids.len(), 4);
    for id in &ids {
        assert!(!taken.contains(id), "{id} was taken");
    }
}

#[test]
fn repeated_duplicates_keep_the_document_loadable() {
    let mut document = example("guessing_game");
    let session = Session::new();
    let (payload, _) = copy(&document, &["b005"]);
    let target = b2c_core_wasm::args::paste_target(&target("mod_main", Some("b005"), None)).unwrap();
    for n in 0..300_u64 {
        let mut seed = [0u8; 32];
        seed[..8].copy_from_slice(&n.to_be_bytes());
        let pasted = session.paste_prepare(&payload, &document, &target, seed).unwrap();
        assert!(pasted.unresolved.is_empty());
        let blocks = serde_json::to_value(&pasted.blocks).unwrap();
        document = insert(&document, 0, Some("b005"), None, &blocks);
    }
    let loaded = parse(&b2c_core_wasm::load(document.as_bytes()));
    assert_eq!(loaded["ok"], true, "{}", loaded["diagnostics"]);
    let result = preview(&document, 4);
    assert_eq!(result["buildable"], true, "{:?}", errors(&result));
    assert_eq!(
        file(&result, "main.cpp").unwrap().matches("Your guess: ").count(),
        301
    );
}

#[test]
fn the_same_seed_gives_the_same_blocks() {
    let game = example("guessing_game");
    let (payload, _) = copy(&game, &["b010"]);
    let at = target("mod_main", Some("b004"), None);
    assert_eq!(
        paste_ok(&payload, &game, &at, 9),
        paste_ok(&payload, &game, &at, 9)
    );
    assert_ne!(
        paste_ok(&payload, &game, &at, 9),
        paste_ok(&payload, &game, &at, 10)
    );
}

#[test]
fn hand_made_payloads_without_names_stay_as_they_are() {
    let payload = json!({
        "format": "blocks2cpp/clipboard", "formatVersion": 1, "catalog": "1.0.0",
        "blocks": [{"id": "p1", "type": "io.print", "v": 1, "extra": {"itemCount": 2},
                    "fields": {"NEWLINE": true, "SEP": "none", "STREAM": "out"},
                    "inputs": {"ITEM0": {"expr": [{"ref": "s_guess"}]},
                               "ITEM1": {"expr": [{"ref": "s_nobody"}]}}}],
        "refs": {}
    })
    .to_string();
    let game = example("guessing_game");
    let pasted = paste_ok(&payload, &game, &target("mod_main", Some("b005"), None), 11);
    // s_guess is visible there (by ID); s_nobody is not and has no name.
    assert_eq!(pasted["unresolved"], json!([{"sym": "s_nobody", "name": null}]));
    let message = pasted["diagnostics"][0]["message"].as_str().unwrap();
    assert!(
        message.contains("a variable or function that doesn't exist here"),
        "{message}"
    );
    assert_eq!(
        pasted["diagnostics"][0]["primary"]["part"],
        json!({"kind": "tokens", "input": "ITEM1", "start": 0, "end": 1})
    );
}

#[test]
fn paste_refuses_bad_arguments() {
    let game = example("guessing_game");
    let (payload, _) = copy(&game, &["b005"]);
    let good_target = target("mod_main", Some("b005"), None);
    for (target_json, seed_hex, what) in [
        (good_target.as_str(), "", "the seed"),
        (good_target.as_str(), &*"0".repeat(63), "the seed"),
        (good_target.as_str(), &*"z".repeat(64), "the seed"),
        ("", &*seed(1), "the paste target"),
        (
            r#"{"module": "mod_main", "extra": 1}"#,
            &*seed(1),
            "the paste target",
        ),
        (
            r#"{"module": "mod_main", "input": "BODY"}"#,
            &*seed(1),
            "the paste target",
        ),
        (
            &*target("nope", None, None),
            &*seed(1),
            "module nope is not in the document",
        ),
        (
            &*target("mod_main", Some("nope"), None),
            &*seed(1),
            "block nope is not in module mod_main",
        ),
    ] {
        let result = paste(&payload, &game, target_json, seed_hex);
        assert_eq!(
            result["error"]["kind"], "invalidArguments",
            "{target_json} {seed_hex}"
        );
        let message = result["error"]["message"].as_str().unwrap();
        assert!(message.contains(what), "{message}");
    }
    // A document that does not load.
    let result = paste(&payload, "{}", &good_target, &seed(1));
    assert_eq!(result["ok"], false);
    assert_eq!(result["unresolved"], json!([]));
    assert!(
        codes(&result["diagnostics"])
            .iter()
            .all(|c| c.starts_with("B2C-E01"))
    );
}

// ------------------------------------------------------- hostile payloads

fn clipboard_suite() -> PathBuf {
    repo_root().join("tests/security/clipboard")
}

/// The rows of `tests/security/clipboard/README.md`: file and expected
/// loader codes (`None` when accepted).
fn clipboard_cases() -> Vec<(String, Option<BTreeSet<String>>)> {
    let readme = std::fs::read_to_string(clipboard_suite().join("README.md")).unwrap();
    let cases: Vec<(String, Option<BTreeSet<String>>)> = readme
        .lines()
        .filter(|line| line.starts_with("| `"))
        .map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            assert_eq!(cells.len(), 6, "{line}");
            let file = cells[1].trim_matches('`').to_owned();
            let loader = (cells[4] != "accepted").then(|| {
                cells[4]
                    .split('`')
                    .filter(|piece| piece.starts_with("B2C-"))
                    .map(str::to_owned)
                    .collect()
            });
            (file, loader)
        })
        .collect();
    assert!(cases.len() >= 20, "the README table looks truncated");
    cases
}

#[test]
fn the_malicious_clipboard_suite_is_refused_with_the_loader_codes() {
    let game = example("guessing_game");
    let at = target("mod_main", Some("b005"), None);
    for (name, expected) in clipboard_cases() {
        let bytes = std::fs::read(clipboard_suite().join(&name)).unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let output = b2c_core_wasm::paste_prepare(&text, &game, &at, &seed(12));
        if expected.is_none() {
            // An accepted payload may nest deeper than serde_json's default
            // parsing limit once it is wrapped in the result.
            assert!(
                output.starts_with(r#"{"ok":true,"blocks":["#),
                "{name}: {output:.300}"
            );
            continue;
        }
        let result = parse(&output);
        for diagnostic in result["diagnostics"].as_array().unwrap() {
            let message = diagnostic["message"].as_str().unwrap();
            assert!(!message.chars().any(is_unsafe_to_show), "{name}: {message:?}");
        }
        match expected {
            Some(codes_expected) => {
                assert_eq!(result["ok"], false, "{name}");
                assert!(result.get("blocks").is_none(), "{name}");
                let found: BTreeSet<String> = codes(&result["diagnostics"]).into_iter().collect();
                assert_eq!(found, codes_expected, "{name}");
                for diagnostic in result["diagnostics"].as_array().unwrap() {
                    assert_eq!(diagnostic["source"], "loader", "{name}");
                }
            }
            None => assert_eq!(result["ok"], true, "{name}: {}", result["diagnostics"]),
        }
    }
}

#[test]
fn generated_attacks_are_refused() {
    let game = example("guessing_game");
    let at = target("mod_main", Some("b005"), None);
    let (payload, _) = copy(&game, &["b005"]);
    let refused = |text: &str| {
        let result = paste(text, &game, &at, &seed(13));
        assert_eq!(result["ok"], false);
        codes(&result["diagnostics"])
    };
    // Deep nesting, far past the limit: refused before it can exhaust the stack.
    for depth in [129, 100_000] {
        assert_eq!(
            refused(&format!("{}{}", "[".repeat(depth), "]".repeat(depth))),
            ["B2C-E0104"]
        );
    }
    // Oversize.
    let mut big = String::from("{\"format\": \"blocks2cpp/clipboard\", \"pad\": \"");
    big.push_str(&"x".repeat(b2c_model::limits::MAX_FILE_BYTES));
    big.push_str("\"}");
    assert_eq!(refused(&big), ["B2C-E0101"]);
    // Duplicate keys, prototype keys, another format, a whole project.
    let duplicate = payload.replacen(
        "\"catalog\": \"1.0.0\",",
        "\"catalog\": \"1.0.0\", \"catalog\": \"9\",",
        1,
    );
    assert_eq!(refused(&duplicate), ["B2C-E0105"]);
    let proto = payload.replacen(
        "\"refs\": {",
        "\"refs\": {\"__proto__\": {\"name\": \"x\", \"kind\": \"variable\"},",
        1,
    );
    assert!(refused(&proto).contains(&String::from("B2C-E0127")));
    let wrong = payload.replacen("blocks2cpp/clipboard", "blockly/clipboard", 1);
    assert_eq!(refused(&wrong), ["B2C-E0138"]);
    assert_eq!(refused(&game), ["B2C-E0138"]);
    assert_eq!(refused(""), ["B2C-E0103"]);
}

// ------------------------------------------------------------- properties

/// The number of generated cases; `PROPTEST_CASES` overrides it.
fn cases_count() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(32)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases_count()))]

    /// Any selection of blocks of any example copies to a payload that loads,
    /// and pastes anywhere in the same document with fresh, unique IDs; the
    /// document with the pasted blocks on its canvas still loads.
    #[test]
    fn copy_and_paste_anywhere(
        example_index in 0..15_usize,
        picks in prop::collection::vec(any::<Index>(), 0..4),
        at in any::<Index>(),
        list in any::<bool>(),
        seed_value in any::<u64>(),
    ) {
        let all = examples();
        let text = &all[example_index % all.len()].text;
        let value: Value = serde_json::from_str(text).unwrap();
        let mut ids = Vec::new();
        block_ids(&value, &mut ids);
        let mut chosen: Vec<&str> = picks.iter().map(|pick| pick.get(&ids).as_str()).collect();
        chosen.dedup();
        let unique: BTreeSet<&str> = chosen.iter().copied().collect();
        prop_assume!(unique.len() == chosen.len());
        let made = make(text, &chosen);
        prop_assert_eq!(&made["ok"], &json!(true), "{}", made);
        let payload = made["payload"].as_str().unwrap();
        prop_assert!(b2c_model::load_clipboard(payload.as_bytes()).is_ok());

        let module = value["modules"][0]["id"].as_str().unwrap();
        let block = at.get(&ids).as_str();
        let input = if list { Some("BODY") } else { None };
        let pasted = paste(payload, text, &target(module, Some(block), input), &seed(seed_value));
        prop_assert_eq!(&pasted["ok"], &json!(true), "{}", pasted);
        let mut new_ids = Vec::new();
        block_ids(&pasted["blocks"], &mut new_ids);
        let before = used_ids(&value);
        let fresh: BTreeSet<&String> = new_ids.iter().collect();
        prop_assert_eq!(fresh.len(), new_ids.len());
        for id in &new_ids {
            prop_assert!(!before.contains(id));
        }
        prop_assert_eq!(
            pasted["unresolved"].as_array().unwrap().len(),
            pasted["diagnostics"].as_array().unwrap().len()
        );
        let after = insert(text, 0, None, None, &pasted["blocks"]);
        let loaded = parse(&b2c_core_wasm::load(after.as_bytes()));
        prop_assert_eq!(&loaded["ok"], &json!(true), "{}", loaded["diagnostics"]);
    }

    /// Mutated payloads never panic: every result is ok or a list of loader
    /// diagnostics.
    #[test]
    fn mutated_payloads_never_panic(cut in any::<Index>(), byte in any::<u8>(), at in any::<Index>()) {
        let game = example("guessing_game");
        let (payload, _) = copy(&game, &["b010"]);
        let mut bytes = payload.into_bytes();
        let position = at.index(bytes.len());
        bytes[position] = byte;
        bytes.truncate(cut.index(bytes.len() + 1).max(position));
        let text = String::from_utf8_lossy(&bytes);
        let result = paste(&text, &game, &target("mod_main", None, None), &seed(14));
        if result["ok"] == json!(false) {
            prop_assert!(!result["diagnostics"].as_array().unwrap().is_empty());
        }
    }
}
