//! Loose statement stacks (ADR-0011): a top-level block may carry `"stack"`,
//! the statement blocks attached below it on the canvas. They are kept
//! intact through load and save, count toward every limit, and are content.

// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod common;

use b2c_ir::{BlockId, Location, ModuleId, Part};
use b2c_model::limits::MAX_BLOCKS;
use b2c_model::{Document, content_hash, load, security_hash, to_canonical_json};
use common::{base, bytes, codes, failure, main_block};
use serde_json::{Value, json};

fn print(id: &str, text: &str) -> Value {
    json!({
        "id": id,
        "type": "io.print",
        "v": 1,
        "extra": { "itemCount": 1 },
        "fields": { "SEP": "none", "NEWLINE": true, "STREAM": "out" },
        "inputs": { "ITEM0": { "expr": [{ "str": text }] } }
    })
}

/// [`base`] plus a loose `print` at (500, 40) with two more stacked below.
fn with_stack() -> Value {
    let mut document = base();
    let mut head = print("b_loose", "one");
    head["x"] = json!(500);
    head["y"] = json!(40);
    head["stack"] = json!([print("b_two", "two"), print("b_three", "three")]);
    document["modules"][0]["workspace"]["blocks"]
        .as_array_mut()
        .unwrap()
        .push(head);
    document
}

fn loads(value: &Value) -> Document {
    match load(&bytes(value)) {
        Ok(document) => document,
        Err(error) => panic!("{:#?}", error.diagnostics),
    }
}

fn loose_head(document: &Document) -> &b2c_model::Block {
    document.modules[0]
        .workspace
        .blocks
        .iter()
        .find(|b| b.id.as_str() == "b_loose")
        .unwrap()
}

#[test]
fn a_stack_loads_in_order_and_saves_intact() {
    let mut document = loads(&with_stack());
    // Canonical order of top-level blocks (the stack itself is not sorted).
    document.modules[0]
        .workspace
        .blocks
        .sort_by(|a, b| a.id.cmp(&b.id));
    let head = loose_head(&document);
    let stacked: Vec<&str> = head.stack.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(stacked, ["b_two", "b_three"]);
    assert_eq!(head.stack[0].x, None);
    let canonical = to_canonical_json(&document);
    assert_eq!(load(canonical.as_bytes()).unwrap(), document);
    // "stack" is the last key of the head block, after "inputs".
    let at_inputs = canonical.find("\"ITEM0\": {\n").unwrap();
    assert!(canonical[at_inputs..].contains("\"stack\": [\n"));
    // The hand-written writer agrees with serde.
    assert_eq!(canonical, serde_json::to_string_pretty(&document).unwrap() + "\n");
    let through_serde: Document = serde_json::from_str(&canonical).unwrap();
    assert_eq!(through_serde, document);
}

#[test]
fn the_stack_is_content() {
    let document = loads(&with_stack());
    let mut reordered = document.clone();
    let head = reordered.modules[0]
        .workspace
        .blocks
        .iter_mut()
        .find(|b| b.id.as_str() == "b_loose")
        .unwrap();
    head.stack.reverse();
    assert_ne!(content_hash(&reordered), content_hash(&document));
    assert_ne!(to_canonical_json(&reordered), to_canonical_json(&document));
    // Moving the head is layout only.
    let mut moved = document.clone();
    let head = moved.modules[0]
        .workspace
        .blocks
        .iter_mut()
        .find(|b| b.id.as_str() == "b_loose")
        .unwrap();
    head.x = Some(-7);
    assert_eq!(content_hash(&moved), content_hash(&document));
    // Ordinary stacked blocks are not security-relevant.
    assert_eq!(security_hash(&reordered), security_hash(&document));
}

#[test]
fn e0139_stack_on_a_nested_block_or_stack_element() {
    let mut document = base();
    main_block(&mut document)["statements"]["BODY"][0]["stack"] = json!([print("b_x", "x")]);
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code.0, "B2C-E0139");
    assert_eq!(
        diagnostics[0].message,
        "This block is inside another block, so it cannot have a \"stack\": only a block directly on the canvas can have blocks stacked below it. Move the stacked blocks into the statement list they belong to."
    );
    assert_eq!(
        diagnostics[0].primary,
        Location {
            module: Some(ModuleId::new("mod_main").unwrap()),
            block: Some(BlockId::new("b_print").unwrap()),
            part: Part::Whole,
        }
    );

    let mut document = with_stack();
    document["modules"][0]["workspace"]["blocks"][2]["stack"][1]["stack"] = json!([print("b_four", "four")]);
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code.0, "B2C-E0139");
    assert_eq!(
        diagnostics[0].primary.block.as_ref().map(BlockId::as_str),
        Some("b_three")
    );
    assert!(
        diagnostics[0]
            .message
            .starts_with("This block is itself in a \"stack\"")
    );
}

#[test]
fn a_misplaced_stack_is_still_checked() {
    // The elements of a misplaced stack are decoded, so their problems and
    // their IDs are reported too.
    let mut document = base();
    main_block(&mut document)["statements"]["BODY"][0]["stack"] =
        json!([print("b_main", "x"), {"id": "b_y", "type": "io.print", "v": 1, "x": 1, "y": 2}]);
    assert_eq!(codes(&document), ["B2C-E0139", "B2C-E0114", "B2C-E0128"]);
}

#[test]
fn e0112_an_empty_stack() {
    let mut document = with_stack();
    document["modules"][0]["workspace"]["blocks"][2]["stack"] = json!([]);
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code.0, "B2C-E0112");
    assert_eq!(
        diagnostics[0].message,
        "\"stack\" in this block is an empty list. Leave \"stack\" out when no blocks are stacked below the block."
    );
    // Other wrong kinds of value.
    for wrong in [json!({}), json!("b_two"), json!([1])] {
        let mut document = with_stack();
        document["modules"][0]["workspace"]["blocks"][2]["stack"] = wrong;
        assert_eq!(codes(&document), ["B2C-E0112"]);
    }
}

#[test]
fn e0128_a_stacked_block_has_no_position() {
    let mut document = with_stack();
    document["modules"][0]["workspace"]["blocks"][2]["stack"][0]["x"] = json!(10);
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code.0, "B2C-E0128");
    assert_eq!(
        diagnostics[0].message,
        "This block is stacked below another block, so it cannot have a canvas position (\"x\" and \"y\"). Remove them."
    );
}

#[test]
fn stacked_blocks_follow_every_block_rule() {
    // IDs are unique across stacks and the rest of the project.
    let mut document = with_stack();
    document["modules"][0]["workspace"]["blocks"][2]["stack"][0]["id"] = json!("b_print");
    assert_eq!(codes(&document), ["B2C-E0114"]);
    // Symbols declared in a stack are unique project-wide too.
    let mut document = with_stack();
    document["modules"][0]["workspace"]["blocks"][2]["stack"][0] = json!({
        "id": "b_decl", "type": "var.declare", "v": 1,
        "fields": { "TYPE": "int", "NAME": { "sym": "s_x", "name": "x" } }
    });
    assert_eq!(codes(&document), ["B2C-E0115"]);
    // Text rules and unknown keys.
    let mut document = with_stack();
    document["modules"][0]["workspace"]["blocks"][2]["stack"][1]["inputs"]["ITEM0"]["expr"][0]["str"] =
        json!("a\u{202e}b");
    document["modules"][0]["workspace"]["blocks"][2]["stack"][1]["next"] = json!({});
    assert_eq!(codes(&document), ["B2C-E0110", "B2C-E0126"]);
}

#[test]
fn stacked_blocks_count_toward_the_block_limit() {
    // One loose head with MAX_BLOCKS stacked below it, plus the 6 blocks of
    // the base project: over the limit by 6.
    let mut document = base();
    let stack: Vec<Value> = (0..MAX_BLOCKS)
        .map(|i| json!({"id": format!("s{i}"), "type": "control.break", "v": 1}))
        .collect();
    document["modules"][0]["workspace"]["blocks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "head", "type": "control.break", "v": 1, "stack": stack}));
    assert_eq!(codes(&document), ["B2C-E0121"]);
}

#[test]
fn a_stack_adds_one_level_of_nesting_only() {
    // Stacks are arrays, not `next` chains: a long stack is as shallow as a
    // short one.
    let mut document = base();
    let stack: Vec<Value> = (0..5000)
        .map(|i| json!({"id": format!("s{i}"), "type": "control.break", "v": 1}))
        .collect();
    document["modules"][0]["workspace"]["blocks"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "head", "type": "control.break", "v": 1, "stack": stack}));
    let loaded = loads(&document);
    assert_eq!(
        loaded.modules[0]
            .workspace
            .blocks
            .iter()
            .map(|b| b.stack.len())
            .sum::<usize>(),
        5000
    );
}
