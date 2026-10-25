//! The row extras of milestone M3's second wave
//! (`docs/plans/m3-contracts/w2.md` §2.1): a struct's `fields` rows
//! (`{sym, name, type}`) and an enum's `values` rows (`{sym, name}`) declare
//! symbols as a function's `params` rows do. The loader claims their symbol
//! IDs, the `type` of a field row is a type term, and renaming, the
//! references a paste needs and paste remapping treat them as parameters.

// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod common;

use std::collections::BTreeSet;

use b2c_ir::SymbolId;
use b2c_model::{
    Document, SeededIds, load, load_clipboard, outside_refs, remap_ids, required_format_version,
    rewrite_refs, to_canonical_json,
};
use common::{base, bytes, codes};
use serde_json::{Value, json};

fn loads(value: &Value) -> Document {
    match load(&bytes(value)) {
        Ok(document) => document,
        Err(error) => panic!("{:#?}", error.diagnostics),
    }
}

fn sym(id: &str) -> SymbolId {
    SymbolId::new(id).unwrap()
}

/// A struct with two field rows and an enum with two value rows.
fn definitions() -> Vec<Value> {
    vec![
        json!({"id": "b_struct", "type": "type.struct", "v": 1, "x": 0, "y": 400,
            "extra": {"fields": [
                {"sym": "s_name", "name": "name", "type": "std::string"},
                {"sym": "s_where", "name": "where", "type": {"ref": "s_place"}}
            ]},
            "fields": {"NAME": {"sym": "s_student", "name": "Student"}}}),
        json!({"id": "b_enum", "type": "type.enum", "v": 1, "x": 0, "y": 800,
            "extra": {"values": [{"sym": "s_deposit", "name": "Deposit"}, {"sym": "s_quit", "name": "Quit"}]},
            "fields": {"NAME": {"sym": "s_action", "name": "Action"}}}),
    ]
}

/// [`base`] with [`definitions`] on its canvas.
fn with_definitions() -> Value {
    let mut document = base();
    let blocks = document["modules"][0]["workspace"]["blocks"]
        .as_array_mut()
        .unwrap();
    blocks.extend(definitions());
    document
}

#[test]
fn rows_declare_their_symbols() {
    let document = with_definitions();
    let loaded = loads(&document);
    assert_eq!(loaded.modules[0].workspace.blocks.len(), 4);
    // A row symbol that another declaration has, for each row key.
    for (block, key) in [(2, "fields"), (3, "values")] {
        let mut twice = with_definitions();
        twice["modules"][0]["workspace"]["blocks"][block]["extra"][key][1]["sym"] = json!("s_x");
        assert_eq!(codes(&twice), ["B2C-E0115"], "{key}");
        // Names are limited in length, as in parameter rows.
        let mut long = with_definitions();
        long["modules"][0]["workspace"]["blocks"][block]["extra"][key][0]["name"] = json!("n".repeat(65));
        assert_eq!(codes(&long), ["B2C-E0123"], "{key}");
    }
    // Rows of other keys declare nothing.
    let mut other = with_definitions();
    other["modules"][0]["workspace"]["blocks"][3]["extra"]["others"] = json!([{"sym": "s_x", "name": "x"}]);
    loads(&other);
}

#[test]
fn a_template_in_a_field_row_needs_version_2() {
    let document = with_definitions();
    assert_eq!(required_format_version(&loads(&document)), 1);
    let mut template = with_definitions();
    template["modules"][0]["workspace"]["blocks"][2]["extra"]["fields"][0]["type"] =
        json!({"std::vector": {"ref": "s_place"}});
    let loaded = loads(&template);
    assert_eq!(required_format_version(&loaded), 2);
    // A malformed template in a field row is checked like one in a field.
    let mut broken = with_definitions();
    broken["modules"][0]["workspace"]["blocks"][2]["extra"]["fields"][0]["type"] = json!({"std::vector": []});
    assert!(!codes(&broken).is_empty());
    // A value row has no type term: the same object there is free data.
    let mut value = with_definitions();
    value["modules"][0]["workspace"]["blocks"][3]["extra"]["values"][0]["type"] = json!({"std::vector": []});
    assert_eq!(required_format_version(&loads(&value)), 1);
}

#[test]
fn rows_round_trip_through_the_canonical_writer() {
    // The canonical form sorts the canvas by block ID and keeps the rows.
    let loaded = loads(&with_definitions());
    let text = to_canonical_json(&loaded);
    let again = load(text.as_bytes()).unwrap();
    assert_eq!(to_canonical_json(&again), text);
    let find = |id: &str| {
        again.modules[0]
            .workspace
            .blocks
            .iter()
            .find(|block| block.id.as_str() == id)
            .unwrap()
            .extra
            .clone()
    };
    let original = |id: &str| {
        loaded.modules[0]
            .workspace
            .blocks
            .iter()
            .find(|block| block.id.as_str() == id)
            .unwrap()
            .extra
            .clone()
    };
    assert_eq!(find("b_struct"), original("b_struct"));
    assert_eq!(find("b_enum"), original("b_enum"));
}

#[test]
fn pastes_rename_row_symbols_and_their_references() {
    let payload = json!({
        "format": "blocks2cpp/clipboard",
        "formatVersion": 1,
        "catalog": "1.3.0",
        "blocks": definitions(),
        "refs": {},
    });
    let mut blocks = load_clipboard(&serde_json::to_vec(&payload).unwrap())
        .unwrap()
        .blocks;
    // The rows declare their symbols, so only the struct `s_place` comes
    // from outside the pasted blocks.
    assert_eq!(outside_refs(&blocks), BTreeSet::from([sym("s_place")]));
    let mut ids = SeededIds::new([7; 32]);
    let map = remap_ids(&mut blocks, &BTreeSet::new(), &mut ids).unwrap();
    for old in [
        "s_name",
        "s_where",
        "s_deposit",
        "s_quit",
        "s_student",
        "s_action",
    ] {
        let new = map
            .get(&sym(old))
            .unwrap_or_else(|| panic!("{old} is not renamed"));
        assert_ne!(new, &sym(old));
    }
    let row_syms = |blocks: &[b2c_model::Block], block: usize, key: &str| -> Vec<String> {
        blocks[block].extra[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["sym"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        row_syms(&blocks, 0, "fields"),
        [map[&sym("s_name")].as_str(), map[&sym("s_where")].as_str()]
    );
    assert_eq!(
        row_syms(&blocks, 1, "values"),
        [map[&sym("s_deposit")].as_str(), map[&sym("s_quit")].as_str()]
    );
    // Binding the outside struct renames the reference in the field row's
    // type, never a declaration.
    let bound = std::collections::BTreeMap::from([(sym("s_place"), sym("s_home"))]);
    rewrite_refs(&mut blocks, &bound);
    assert_eq!(blocks[0].extra["fields"][1]["type"], json!({"ref": "s_home"}));
    assert_eq!(outside_refs(&blocks), BTreeSet::from([sym("s_home")]));
}
