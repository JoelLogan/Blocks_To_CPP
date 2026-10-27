//! Copy and paste of blocks that name structs, enums, fields and enum values
//! (M3 contracts types.md §2.4; w2.md §2.2, package C1): the payload records
//! each by its qualified name and kind, and a paste binds each again at its
//! target: a struct or an enum by name, a member through its owner.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests fail by panicking"
)]
// The project document below is one `json!` literal.
#![recursion_limit = "256"]

mod common;

use common::{codes, parse, update};
use serde_json::{Value, json};

/// A project whose module defines the struct `Student` (`name`, `mark`) and
/// the enum `Action` (`Deposit`, `Quit`), with symbol IDs starting with
/// `prefix`, and whose `main` makes a `Student` `s`, prints its mark and
/// makes an `Action` `a`. `fields` replaces the struct's field names.
fn document(prefix: &str, fields: &[&str]) -> String {
    let sym = |name: &str| format!("{prefix}_{name}");
    let rows: Vec<Value> = fields
        .iter()
        .map(|field| {
            json!({ "sym": sym(&format!("Student_{field}")), "name": field,
                              "type": if *field == "name" { "std::string" } else { "int" } })
        })
        .collect();
    let parts: Vec<(String, Value)> = fields
        .iter()
        .enumerate()
        .map(|(index, field)| {
            let value = if *field == "name" {
                json!({ "expr": [{ "str": "Ann" }] })
            } else {
                json!({ "expr": [{ "num": "74" }] })
            };
            (format!("FIELD{index}"), value)
        })
        .collect();
    let inputs: serde_json::Map<String, Value> = parts.into_iter().collect();
    let value = json!({
        "format": "blocks2cpp/project",
        "formatVersion": 1,
        "generator": { "app": "0.1.0", "catalog": b2c_catalog::CATALOG_VERSION },
        "project": { "id": "prj_types", "name": "Types", "language": { "standard": "c++20" } },
        "modules": [{ "id": "mod_main", "name": "main", "workspace": { "blocks": [
            { "id": "b001", "type": "program.main", "v": 1, "x": 40, "y": 40, "statements": { "BODY": [
                { "id": "b002", "type": "var.declare", "v": 1,
                  "fields": { "TYPE": { "ref": sym("Student") }, "NAME": { "sym": sym("s"), "name": "s" } },
                  "inputs": { "VALUE": { "block": {
                      "id": "b003", "type": "type.new", "v": 1,
                      "fields": { "TYPE": { "ref": sym("Student") } },
                      "extra": { "fieldCount": fields.len() }, "inputs": inputs } } } },
                { "id": "b004", "type": "io.print", "v": 1, "inputs": { "ITEM0": { "block": {
                    "id": "b005", "type": "type.field", "v": 1,
                    "fields": { "MEMBER": { "ref": sym("Student_mark") } },
                    "inputs": { "OBJ": { "expr": [{ "ref": sym("s") }] } } } } } },
                { "id": "b006", "type": "var.declare", "v": 1,
                  "fields": { "TYPE": { "ref": sym("Action") }, "NAME": { "sym": sym("a"), "name": "a" } },
                  "inputs": { "VALUE": { "block": {
                      "id": "b007", "type": "type.enum_value", "v": 1,
                      "fields": { "ENUM": { "ref": sym("Action") }, "VALUE": { "ref": sym("Action_Quit") } } } } } },
            ] } },
            { "id": "b010", "type": "type.struct", "v": 1, "x": 40, "y": 400,
              "fields": { "NAME": { "sym": sym("Student"), "name": "Student" } },
              "extra": { "fields": rows } },
            { "id": "b011", "type": "type.enum", "v": 1, "x": 40, "y": 800,
              "fields": { "NAME": { "sym": sym("Action"), "name": "Action" } },
              "extra": { "values": [
                  { "sym": sym("Action_Deposit"), "name": "Deposit" },
                  { "sym": sym("Action_Quit"), "name": "Quit" },
              ] } },
        ] } }],
    });
    serde_json::to_string_pretty(&value).unwrap()
}

/// The payload of a copy of `ids`, which must succeed.
fn copy(document: &str, ids: &[&str]) -> String {
    let made = parse(&b2c_core_wasm::clipboard_make(
        document,
        &serde_json::to_string(ids).unwrap(),
    ));
    assert_eq!(made["ok"], true, "{made}");
    made["payload"].as_str().unwrap().to_owned()
}

/// The payload's outside references, as `symbol: name kind`.
fn refs(payload: &str) -> Vec<String> {
    let clipboard = b2c_model::load_clipboard(payload.as_bytes()).unwrap();
    clipboard
        .refs
        .iter()
        .map(|(sym, r)| format!("{sym}: {} {:?}", r.name, r.kind))
        .collect()
}

/// A paste after the block `after` of `document`'s `main`, which must
/// succeed.
fn paste_after(payload: &str, document: &str, after: &str) -> Value {
    let target = json!({ "module": "mod_main", "block": after, "input": null }).to_string();
    let pasted = parse(&b2c_core_wasm::paste_prepare(
        payload,
        document,
        &target,
        &format!("{:064x}", 7),
    ));
    assert_eq!(pasted["ok"], true, "{pasted}");
    pasted
}

#[test]
fn the_document_previews_cleanly() {
    let result = update(&document("s", &["name", "mark"]), 4);
    assert_eq!(codes(&result["diagnostics"]), Vec::<String>::new());
    assert_eq!(result["buildable"], true);
}

#[test]
fn structs_enums_and_their_members_are_recorded_by_qualified_name() {
    let source = document("s", &["name", "mark"]);
    assert_eq!(refs(&copy(&source, &["b002"])), ["s_Student: ::Student Struct"]);
    assert_eq!(
        refs(&copy(&source, &["b004"])),
        ["s_Student_mark: ::Student::mark Field", "s_s: s Variable"]
    );
    assert_eq!(
        refs(&copy(&source, &["b006"])),
        [
            "s_Action: ::Action Enum",
            "s_Action_Quit: ::Action::Quit Enumerator"
        ]
    );
}

#[test]
fn a_paste_binds_user_types_by_name_and_members_through_their_owner() {
    let source = document("s", &["name", "mark"]);
    // The same definitions, with other symbol IDs.
    let target = document("t", &["name", "mark"]);
    for (ids, expected) in [
        (["b004"], ["t_Student_mark", "t_s"]),
        (["b006"], ["t_Action", "t_Action_Quit"]),
    ] {
        let pasted = paste_after(&copy(&source, &ids), &target, "b006");
        assert_eq!(pasted["unresolved"], json!([]), "{pasted}");
        let text = pasted["blocks"].to_string();
        for sym in expected {
            assert!(text.contains(&format!("\"{sym}\"")), "{sym}: {text}");
        }
        assert!(!text.contains("\"s_"), "{text}");
    }
    // The struct's type in *create* and *new … with* binds by name too.
    let pasted = paste_after(&copy(&source, &["b002"]), &target, "b006");
    let text = pasted["blocks"].to_string();
    assert!(
        text.contains("\"t_Student\"") && !text.contains("\"s_Student\""),
        "{text}"
    );
}

#[test]
fn a_member_the_target_type_lacks_stays_unresolved() {
    let source = document("s", &["name", "mark"]);
    // `Student` exists at the target, without a field `mark`.
    let target = document("t", &["name", "score"]);
    let pasted = paste_after(&copy(&source, &["b004"]), &target, "b006");
    assert_eq!(
        pasted["unresolved"],
        json!([{ "sym": "s_Student_mark", "name": "::Student::mark" }])
    );
    assert_eq!(codes(&pasted["diagnostics"]), ["B2C-E0201"]);
}
