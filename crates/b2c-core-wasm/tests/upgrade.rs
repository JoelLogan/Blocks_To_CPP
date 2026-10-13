//! Block upgrades in the editor's core (05 §5.7, 03 §3.11.3, ADR-0015):
//! `load` and `paste_prepare` upgrade blocks before the editor sees them, and
//! report the blocks they could not upgrade (`B2C-E0603`) and, for a payload
//! from a newer catalog, the blocks that catalog extended (`B2C-E0602`).
//!
//! The shipped catalog has nothing to upgrade yet, so these tests use blocks
//! that cannot be upgraded (version 0) and a payload from a newer catalog;
//! `b2c-catalog`'s tests cover successful upgrades with test-only tables. A
//! document upgraded with such a table checks that the scope query (06 §6.5)
//! answers for the blocks an upgrade produced.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests fail by panicking"
)]

mod common;

use common::{codes, example, names, parse, preview};
use serde_json::{Value, json};

/// A project whose `main` holds `body`.
fn project(body: &Value) -> String {
    json!({
        "format": "blocks2cpp/project", "formatVersion": 1,
        "generator": {"app": "0.1.0", "catalog": "1.1.0"},
        "project": {"id": "prj_up", "name": "Up", "language": {"standard": "c++20"}},
        "modules": [{"id": "mod_main", "name": "main", "workspace": {"blocks": [
            {"id": "b_main", "type": "program.main", "v": 1, "x": 0, "y": 0,
             "statements": {"BODY": body}}
        ]}}]
    })
    .to_string()
}

/// A clipboard payload of `catalog` with these blocks.
fn payload(catalog: &str, blocks: &Value) -> String {
    json!({"format": "blocks2cpp/clipboard", "formatVersion": 1, "catalog": catalog,
           "blocks": blocks, "refs": {}})
    .to_string()
}

fn paste(payload: &str, document: &str) -> Value {
    let target = json!({"module": "mod_main", "block": "b_main", "input": "BODY"}).to_string();
    parse(&b2c_core_wasm::paste_prepare(
        payload,
        document,
        &target,
        &format!("{:064x}", 7),
    ))
}

#[test]
fn load_reports_blocks_it_cannot_upgrade() {
    let old = json!([{"id": "b_old", "type": "io.print", "v": 0}]);
    let loaded = parse(&b2c_core_wasm::load(project(&old).as_bytes()));
    assert_eq!(loaded["ok"], true, "{loaded}");
    assert_eq!(codes(&loaded["diagnostics"]), ["B2C-E0603"]);
    assert_eq!(loaded["diagnostics"][0]["primary"]["block"], "b_old");
    assert_eq!(loaded["diagnostics"][0]["primary"]["module"], "mod_main");
    // Left as it was.
    assert_eq!(
        loaded["document"]["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"][0]["v"],
        0
    );

    let current = json!([{"id": "b_new", "type": "io.print", "v": 1}]);
    let loaded = parse(&b2c_core_wasm::load(project(&current).as_bytes()));
    assert_eq!(loaded["diagnostics"], json!([]));
}

#[test]
fn paste_upgrades_the_pasted_blocks() {
    let document = project(&json!([]));
    // A block that cannot be upgraded is pasted as it was, and reported
    // under its new ID.
    let old = payload("1.1.0", &json!([{"id": "c1", "type": "io.print", "v": 0}]));
    let pasted = paste(&old, &document);
    assert_eq!(pasted["ok"], true, "{pasted}");
    let block = &pasted["blocks"][0];
    assert_eq!(block["v"], 0);
    assert_ne!(block["id"], "c1");
    assert_eq!(codes(&pasted["diagnostics"]), ["B2C-E0603"]);
    assert_eq!(pasted["diagnostics"][0]["primary"]["block"], block["id"]);

    // From a newer catalog, a field this one does not know is reported as
    // made by it, and the block is kept as it is.
    let extended = json!([{"id": "c1", "type": "io.print", "v": 1, "fields": {"VOLUME": "11"}}]);
    let pasted = paste(&payload("1.2.0", &extended), &document);
    assert_eq!(codes(&pasted["diagnostics"]), ["B2C-E0602"]);
    assert_eq!(pasted["blocks"][0]["fields"]["VOLUME"], "11");
    // From this catalog, it is left to the preview (an unknown field).
    let pasted = paste(&payload("1.1.0", &extended), &document);
    assert_eq!(pasted["diagnostics"], json!([]));
}

/// `test.assign` v1, a retired test type, becomes `var.set` v1 as it is: it
/// has the same field and input.
fn assign_to_set(block: &mut b2c_model::Block) -> Result<(), String> {
    if block.fields.contains_key("VAR") {
        Ok(())
    } else {
        Err(String::from("the block has no VAR field"))
    }
}

/// Retires `test.assign` in favour of `var.set`.
const ASSIGN_RETIRED: b2c_catalog::BlockUpgrades<'static> = b2c_catalog::BlockUpgrades {
    migrations: &[],
    replacements: &[b2c_catalog::BlockReplacement {
        from: "test.assign",
        version: 1,
        to: "var.set",
        to_version: 1,
        apply: assign_to_set,
    }],
    retired: &["test.assign"],
};

/// The scope query answers for a block an upgrade replaced, under the ID it
/// kept: in `main`, what is visible there, and parked on the canvas, what is
/// visible at the end of `main` (ADR-0015, 06 §6.5).
#[test]
fn the_scope_query_answers_for_upgraded_blocks() {
    let mut game: Value = serde_json::from_str(&example("guessing_game")).unwrap();
    let assign = |id: &str| {
        json!({"id": id, "type": "test.assign", "v": 1,
               "fields": {"VAR": {"ref": "s_guess"}},
               "inputs": {"VALUE": {"expr": [{"num": "1"}]}}})
    };
    let blocks = game["modules"][0]["workspace"]["blocks"].as_array_mut().unwrap();
    blocks[0]["statements"]["BODY"]
        .as_array_mut()
        .unwrap()
        .push(assign("b_assigned"));
    let mut parked = assign("b_parked");
    parked["x"] = json!(600);
    parked["y"] = json!(40);
    blocks.push(parked);
    let mut document = b2c_model::load(game.to_string().as_bytes()).unwrap();
    let report =
        b2c_catalog::upgrade_document_with(&mut document, b2c_catalog::core_catalog(), &ASSIGN_RETIRED);
    assert_eq!((report.upgraded, report.diagnostics.len()), (2, 0));
    let text = b2c_model::to_canonical_json(&document);
    let result = preview(&text, 4);
    // The parked block only warns.
    assert_eq!(
        codes(&result["diagnostics"]),
        ["B2C-W0505"],
        "{}",
        result["diagnostics"]
    );
    let scope = |block: &str, input: Option<&str>| -> Value {
        serde_json::from_str(&b2c_core_wasm::symbols_in_scope(block, input.map(str::to_owned))).unwrap()
    };
    assert_eq!(names(&scope("b_assigned", Some("VALUE"))), ["guess", "secret"]);
    assert_eq!(names(&scope("b_assigned", None)), ["guess", "secret"]);
    assert_eq!(names(&scope("b_parked", Some("VALUE"))), ["guess", "secret"]);
}
