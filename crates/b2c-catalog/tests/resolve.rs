//! Regression tests for the resolve stage (spec §6.3): one or more per rule,
//! plus default filling and the example projects.

// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_pass_by_value,
    reason = "test helpers fail the test by panicking and take json! values"
)]

mod common;

use std::collections::BTreeMap;

use b2c_catalog::{Catalog, core_catalog, resolve};
use b2c_ir::{BlockId, DiagSource, Location, ModuleId, Part, Severity};
use b2c_model::{ExprInput, FieldValue, Input, Token};
use common::{codes, messages, project};
use serde_json::{Value, json};

fn print(id: &str, inputs: Value) -> Value {
    json!({"id": id, "type": "io.print", "v": 1, "extra": {"itemCount": 1}, "inputs": inputs})
}

fn location(block: &str, part: Part) -> Location {
    Location {
        module: Some(ModuleId::new("mod_main").unwrap()),
        block: Some(BlockId::new(block).unwrap()),
        part,
    }
}

#[test]
fn every_example_resolves_without_problems() {
    let dir = common::repo_root().join("examples");
    let mut count = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "b2c") {
            let document = b2c_model::load(&std::fs::read(&path).unwrap()).unwrap();
            let (completed, diagnostics) = resolve(&document, core_catalog());
            assert_eq!(diagnostics, [], "{}", path.display());
            // Resolving is idempotent.
            let (again, more) = resolve(&completed, core_catalog());
            assert_eq!(again, completed);
            assert_eq!(more, []);
            count += 1;
        }
    }
    assert!(count >= 15);
}

#[test]
fn diagnostics_come_from_the_catalog_stage() {
    let value = project(json!([{"id": "b1", "type": "nope.nope", "v": 1}]), json!([]));
    let (_, diagnostics) = common::resolve(&value);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].source, DiagSource::Catalog);
    assert_eq!(diagnostics[0].severity, Severity::Error);
}

// --- Defaults ---------------------------------------------------------------

#[test]
fn defaults_are_filled_in() {
    let value = project(json!([{"id": "b1", "type": "control.if", "v": 1}]), json!([]));
    let (document, diagnostics) = common::resolve(&value);
    assert_eq!(diagnostics, []);
    let block = &document.modules[0].workspace.blocks[0].statements["BODY"][0];
    assert_eq!(block.extra["elseIfCount"], json!(0));
    assert_eq!(block.extra["hasElse"], json!(false));
    assert_eq!(
        block.inputs["COND0"],
        Input::Expr(ExprInput {
            expr: vec![Token::Kw("true".into())],
            draft: false
        })
    );
    assert_eq!(block.inputs.len(), 1);
    // Statement inputs are not filled: absent means empty.
    assert!(block.statements.is_empty());

    let value = project(json!([print("b1", json!({}))]), json!([]));
    let (document, _) = common::resolve(&value);
    let block = &document.modules[0].workspace.blocks[0].statements["BODY"][0];
    assert_eq!(block.fields["SEP"], FieldValue::Text("none".into()));
    assert_eq!(block.fields["NEWLINE"], FieldValue::Bool(true));
    assert_eq!(block.fields["STREAM"], FieldValue::Text("out".into()));
    assert_eq!(
        block.inputs["ITEM0"],
        Input::Expr(ExprInput {
            expr: vec![Token::Str("Hello, world!".into())],
            draft: false
        })
    );
}

#[test]
fn present_values_are_kept_and_optional_inputs_stay_empty() {
    let value = project(
        json!([
            {"id": "b1", "type": "io.print", "v": 1, "extra": {"itemCount": 2},
             "fields": {"SEP": "comma"}, "inputs": {"ITEM1": {"expr": [{"num": "7"}]}}},
            {"id": "b2", "type": "func.return", "v": 1},
            {"id": "b3", "type": "var.declare", "v": 1, "fields": {"NAME": {"sym": "s1", "name": "a"}}}
        ]),
        json!([]),
    );
    let (document, diagnostics) = common::resolve(&value);
    assert_eq!(diagnostics, []);
    let body = &document.modules[0].workspace.blocks[0].statements["BODY"];
    assert_eq!(body[0].fields["SEP"], FieldValue::Text("comma".into()));
    assert_eq!(
        body[0].inputs["ITEM1"],
        Input::Expr(ExprInput {
            expr: vec![Token::Num("7".into())],
            draft: false
        })
    );
    assert!(body[0].inputs.contains_key("ITEM0"));
    assert!(body[1].inputs.is_empty(), "return VALUE is optional");
    assert!(!body[2].inputs.contains_key("VALUE"), "declare VALUE is optional");
    assert_eq!(body[2].fields["TYPE"], FieldValue::Text("int".into()));
    assert_eq!(body[2].fields["CONST"], FieldValue::Bool(false));
}

// --- E0600–E0603: catalog and versions --------------------------------------

#[test]
fn e0600_empty_catalog() {
    let empty = Catalog {
        version: "1.0.0".into(),
        blocks: BTreeMap::new(),
    };
    let document = common::load(&project(json!([]), json!([])));
    let (same, diagnostics) = resolve(&document, &empty);
    assert_eq!(same, document);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code.0, "B2C-E0600");
}

#[test]
fn e0601_unknown_types_keep_their_children_checked() {
    let value = project(
        json!([{"id": "b1", "type": "sfml.window", "v": 1, "fields": {"X": 1.to_string()},
                "statements": {"BODY": [{"id": "b2", "type": "io.print", "v": 1, "extra": {"itemCount": 40}}]}}]),
        json!([]),
    );
    let (document, diagnostics) = common::resolve(&value);
    let found: Vec<&str> = diagnostics.iter().map(|d| d.code.0.as_str()).collect();
    assert_eq!(found, ["B2C-E0601", "B2C-E0613"]);
    assert_eq!(
        diagnostics[0].message,
        "This block has the type \"sfml.window\", which this version of Blocks2Cpp does not know. Missing pack: \"sfml\". Install that library pack, or update Blocks2Cpp if the block comes from a newer version."
    );
    assert_eq!(diagnostics[0].primary, location("b1", Part::Whole));
    // The unknown block is preserved unchanged.
    let block = &document.modules[0].workspace.blocks[0].statements["BODY"][0];
    assert_eq!(block.fields["X"], FieldValue::Text("1".into()));

    // A type that is not even shaped like a block ID names no pack.
    let value = project(json!([{"id": "b1", "type": "Not A Type<b>", "v": 1}]), json!([]));
    assert_eq!(codes(&value), ["B2C-E0601"]);
    assert_eq!(
        messages(&value)[0],
        "This block has the type \"Not A Type<b>\", which is not a valid block type. Block types look like \"io.print\"."
    );
}

#[test]
fn e0602_and_e0603_versions() {
    let value = project(
        json!([{"id": "b1", "type": "control.break", "v": 2}, {"id": "b2", "type": "control.break", "v": 0}]),
        json!([]),
    );
    assert_eq!(codes(&value), ["B2C-E0602", "B2C-E0603"]);
    assert!(messages(&value)[0].starts_with("This block was made with a newer version of Blocks2Cpp"));
}

// --- E0604: shapes and places -----------------------------------------------

#[test]
fn e0604_blocks_in_the_wrong_place() {
    let value = project(
        json!([
            {"id": "b1", "type": "math.number", "v": 1, "fields": {"VALUE": "1"}},
            {"id": "b2", "type": "program.main", "v": 1},
            {"id": "b3", "type": "var.set", "v": 1, "fields": {"VAR": {"ref": "s"}},
             "inputs": {"VALUE": {"block": {"id": "b4", "type": "control.break", "v": 1}}}}
        ]),
        json!([
            {"id": "b5", "type": "io.print", "v": 1, "extra": {"itemCount": 1}},
            {"id": "b6", "type": "logic.boolean", "v": 1}
        ]),
    );
    let (_, diagnostics) = common::resolve(&value);
    let found: Vec<(&str, &str)> = diagnostics
        .iter()
        .map(|d| (d.code.0.as_str(), d.primary.block.as_ref().unwrap().as_str()))
        .collect();
    assert_eq!(
        found,
        [
            ("B2C-E0604", "b1"),
            ("B2C-E0604", "b2"),
            ("B2C-E0604", "b4"),
            ("B2C-E0604", "b5"),
            ("B2C-E0604", "b6"),
        ]
    );
    let text: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        text,
        [
            "This \"math.number\" block gives a value and cannot be a step on its own. Put it into an input of another block.",
            "A \"program.main\" block must sit directly on the canvas, not inside another block.",
            "This \"control.break\" block is a step, not a value, so it cannot be plugged into an input.",
            "This \"io.print\" block is not inside \"when program starts\" or a function, so it would never run. Move it inside one, or delete it.",
            "This \"logic.boolean\" block gives a value, but it is not plugged into another block. Put it into an input, or delete it.",
        ]
    );
}

// --- E0605–E0607: fields ------------------------------------------------------

#[test]
fn e0605_to_e0607_fields() {
    let value = project(
        json!([
            {"id": "b1", "type": "io.print", "v": 1, "extra": {"itemCount": 1},
             "fields": {"SEP": "\"); system(\"rm -rf ~\"); //", "NEWLINE": "yes", "COLOUR": "red"}},
            {"id": "b2", "type": "var.declare", "v": 1,
             "fields": {"TYPE": "int; system(\"x\"); int", "NAME": {"ref": "s1"}}},
            {"id": "b3", "type": "var.set", "v": 1, "inputs": {"VALUE": {"expr": [{"num": "1"}]}}},
            {"id": "b4", "type": "control.for_range", "v": 1},
            {"id": "b5", "type": "var.get", "v": 1, "fields": {"VAR": "x"}},
        ]),
        json!([]),
    );
    let (_, diagnostics) = common::resolve(&value);
    let found: Vec<(&str, &Location)> = diagnostics
        .iter()
        .map(|d| (d.code.0.as_str(), &d.primary))
        .collect();
    let field = |block: &str, name: &str| location(block, Part::Field { name: name.into() });
    assert_eq!(
        found,
        [
            ("B2C-E0605", &field("b1", "COLOUR")),
            ("B2C-E0607", &field("b1", "SEP")),
            ("B2C-E0607", &field("b1", "NEWLINE")),
            ("B2C-E0607", &field("b2", "TYPE")),
            ("B2C-E0607", &field("b2", "NAME")),
            ("B2C-E0606", &field("b3", "VAR")),
            ("B2C-E0606", &field("b4", "VAR")),
            ("B2C-E0604", &location("b5", Part::Whole)),
            ("B2C-E0607", &field("b5", "VAR")),
        ]
    );
    assert_eq!(
        diagnostics[1].message,
        "The field SEP is \"\\\"); system(\\\"rm -rf ~\\\"); //\", but it must be one of \"none\", \"space\" or \"comma\"."
    );
    assert_eq!(
        diagnostics[3].message,
        "The field TYPE is \"int; system(\\\"x\\\"); int\", but it must be one of the types \"int\", \"double\", \"bool\", \"char\", \"std::string\" or \"auto\"."
    );
    assert_eq!(
        diagnostics[5].message,
        "This block needs to know which variable or function it uses: its field VAR is missing."
    );
    assert_eq!(
        diagnostics[6].message,
        "This block needs a name: its field VAR is missing."
    );
}

#[test]
fn e0607_number_and_text_fields() {
    let value = project(
        json!([{"id": "b1", "type": "var.set", "v": 1, "fields": {"VAR": {"ref": "s"}}, "inputs": {"VALUE": {"block":
        {"id": "b2", "type": "logic.ternary", "v": 1, "inputs": {
            "THEN": {"block": {"id": "b3", "type": "math.number", "v": 1, "fields": {"VALUE": "  "}}},
            "ELSE": {"block": {"id": "b4", "type": "text.literal", "v": 1, "fields": {"VALUE": false}}}
        }}}}}]),
        json!([]),
    );
    assert_eq!(codes(&value), ["B2C-E0607", "B2C-E0607"]);
    // Inputs are visited in name order: ELSE before THEN.
    let found = messages(&value);
    assert_eq!(found[0], "The field VALUE should be text, but it is false.");
    assert_eq!(
        found[1],
        "The field VALUE should be a number written as text, such as \"42\", but it is \"  \"."
    );
}

// --- E0608–E0610: inputs and statements ---------------------------------------

#[test]
fn e0608_and_e0609_inputs() {
    let value = project(
        json!([
            print("b1", json!({"ITEM0": {"expr": []}, "ITEM1": {"expr": []}, "ITEM01": {"expr": []}, "TEXT": {"expr": []}})),
            {"id": "b2", "type": "var.set", "v": 1, "fields": {"VAR": {"ref": "s"}}},
            {"id": "b3", "type": "func.call_stmt", "v": 1, "fields": {"FUNC": {"ref": "s"}}, "extra": {"argCount": 2},
             "inputs": {"ARG0": {"expr": [{"num": "1"}]}}},
        ]),
        json!([]),
    );
    let (_, diagnostics) = common::resolve(&value);
    let found: Vec<(&str, &Location)> = diagnostics
        .iter()
        .map(|d| (d.code.0.as_str(), &d.primary))
        .collect();
    let input = |block: &str, name: &str| location(block, Part::Input { name: name.into() });
    assert_eq!(
        found,
        [
            ("B2C-E0608", &input("b1", "ITEM01")),
            ("B2C-E0608", &input("b1", "ITEM1")),
            ("B2C-E0608", &input("b1", "TEXT")),
            ("B2C-E0609", &input("b2", "VALUE")),
            ("B2C-E0609", &input("b3", "ARG1")),
        ]
    );
    assert_eq!(
        diagnostics[1].message,
        "This block has no input \"ITEM1\": it has 1 ITEM input(s), set by \"extra.itemCount\"."
    );
    assert_eq!(
        diagnostics[3].message,
        "This block needs a value in its input VALUE."
    );
}

#[test]
fn e0610_statements() {
    let break_block = |id: &str| json!({"id": id, "type": "control.break", "v": 1});
    let value = project(
        json!([{"id": "b1", "type": "control.if", "v": 1, "extra": {"elseIfCount": 1, "hasElse": false},
                "statements": {"DO0": [], "DO1": [break_block("b2")], "DO2": [], "ELSE": [break_block("b3")], "BODY": []}}]),
        json!([]),
    );
    let (_, diagnostics) = common::resolve(&value);
    let found: Vec<&str> = diagnostics.iter().map(|d| d.code.0.as_str()).collect();
    assert_eq!(found, ["B2C-E0610", "B2C-E0610", "B2C-E0610"]);
    let text: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        text,
        [
            "This block has no part \"BODY\" for other blocks. Remove it or check its spelling.",
            "This block has no part \"DO2\": it has 2 DO part(s), set by \"extra.elseIfCount\".",
            "This block has the part \"ELSE\", but \"extra.hasElse\" is false, so it has no such part. Set \"extra.hasElse\" to true or remove the part.",
        ]
    );
    // With hasElse, ELSE is fine.
    let value = project(
        json!([{"id": "b1", "type": "control.if", "v": 1, "extra": {"hasElse": true},
                "statements": {"ELSE": [break_block("b2")]}}]),
        json!([]),
    );
    assert_eq!(codes(&value), Vec::<String>::new());
}

// --- E0611–E0614: extras and parameters ---------------------------------------

#[test]
fn e0611_to_e0613_extras() {
    let value = project(
        json!([
            {"id": "b1", "type": "io.print", "v": 1, "extra": {"itemCount": 0, "colour": 1}},
            {"id": "b2", "type": "io.print", "v": 1, "extra": {"itemCount": 33}},
            {"id": "b3", "type": "io.print", "v": 1, "extra": {"itemCount": "2"}},
            {"id": "b4", "type": "control.if", "v": 1, "extra": {"elseIfCount": -1, "hasElse": "yes"}},
            {"id": "b5", "type": "control.forever", "v": 1, "extra": {"anything": true}},
        ]),
        json!([{"id": "b6", "type": "func.define", "v": 1, "fields": {"NAME": {"sym": "s_f", "name": "f"}}}]),
    );
    let (_, diagnostics) = common::resolve(&value);
    let found: Vec<(&str, &str)> = diagnostics
        .iter()
        .map(|d| (d.code.0.as_str(), d.primary.block.as_ref().unwrap().as_str()))
        .collect();
    assert_eq!(
        found,
        [
            ("B2C-E0611", "b1"),
            ("B2C-E0613", "b1"),
            ("B2C-E0613", "b2"),
            ("B2C-E0613", "b3"),
            ("B2C-E0613", "b4"),
            ("B2C-E0613", "b4"),
            ("B2C-E0611", "b5"),
            ("B2C-E0612", "b6"),
        ]
    );
    let text: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        text[7],
        "This block is missing its setting \"params\" in \"extra\"."
    );
    assert_eq!(
        text[1],
        "\"extra.itemCount\" of this block is 0, but it must be a whole number from 1 to 32."
    );
    assert_eq!(
        text[3],
        "\"extra.itemCount\" of this block is the text \"2\", but it must be a whole number from 1 to 32."
    );
}

#[test]
fn e0613_and_e0614_parameters() {
    let row = |sym: &str, ty: &str, mode: &str| json!({"sym": sym, "name": "p", "type": ty, "mode": mode});
    let rows: Vec<Value> = (0..17).map(|i| row(&format!("p{i}"), "int", "copy")).collect();
    let value = project(
        json!([]),
        json!([
            {"id": "b1", "type": "func.define", "v": 1, "fields": {"NAME": {"sym": "s1", "name": "f"}},
             "extra": {"params": rows}},
            {"id": "b2", "type": "func.define", "v": 1, "fields": {"NAME": {"sym": "s2", "name": "g"}},
             "extra": {"params": [
                row("q1", "float", "copy"),
                row("q2", "int", "rvalue"),
                {"sym": "bad id", "name": 3, "type": "int", "mode": "copy", "default": 1},
                "x",
             ]}},
        ]),
    );
    let (_, diagnostics) = common::resolve(&value);
    let text: Vec<(&str, &str)> = diagnostics
        .iter()
        .map(|d| (d.code.0.as_str(), d.message.as_str()))
        .collect();
    assert_eq!(
        text,
        [
            (
                "B2C-E0613",
                "This block has 17 parameters, but at most 16 are allowed."
            ),
            (
                "B2C-E0614",
                "Parameter 1 of this block has the type \"float\", but parameters can only be \"int\", \"double\", \"bool\", \"char\" or \"std::string\"."
            ),
            (
                "B2C-E0614",
                "Parameter 2 of this block has the mode \"rvalue\", but the mode must be \"copy\", \"editable\" or \"read_only\"."
            ),
            (
                "B2C-E0614",
                "Parameter 3 of this block has an unknown key \"default\"."
            ),
            (
                "B2C-E0614",
                "Parameter 3 of this block needs a valid symbol ID in \"sym\"."
            ),
            ("B2C-E0614", "Parameter 3 of this block needs a name in \"name\"."),
            (
                "B2C-E0614",
                "Parameter 4 of this block should be an object with \"sym\", \"name\", \"type\" and \"mode\", but it is the text \"x\"."
            ),
        ]
    );
}

// --- E0620: broken catalog definitions ----------------------------------------

#[test]
fn e0620_broken_definitions_are_reported_once() {
    let mut catalog = core_catalog().clone();
    let print = catalog.blocks.get_mut("io.print").unwrap();
    print.input[0].repeat.as_mut().unwrap().count = "missing".into();
    let value = project(
        json!([print_block_value("b1"), print_block_value("b2")]),
        json!([]),
    );
    let (_, diagnostics) = resolve(&common::load(&value), &catalog);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code.0, "B2C-E0620");
    assert!(diagnostics[0].message.contains("\"io.print\" is broken"));
}

fn print_block_value(id: &str) -> Value {
    print(id, json!({}))
}

#[test]
fn resolving_with_a_catalog_built_by_hand_never_panics() {
    // Strip every definition down in turn; resolve must report, not panic.
    let examples: Vec<_> = std::fs::read_dir(common::repo_root().join("examples"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "b2c"))
        .map(|p| b2c_model::load(&std::fs::read(p).unwrap()).unwrap())
        .collect();
    for id in core_catalog().blocks.keys() {
        let mut catalog = core_catalog().clone();
        let def = catalog.blocks.get_mut(id).unwrap();
        def.extra.clear();
        def.field.clear();
        for document in &examples {
            let _ = resolve(document, &catalog);
        }
    }
}
