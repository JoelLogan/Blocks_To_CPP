//! `analyze_checked`, as the build and the editor's preview analyse a
//! document (06 §6.1): what the block catalog already reported is not
//! reported again by the analyser, so one mistake gives one message.

// Test code: unwrap/expect/panic and printing to stderr (the shared helpers) are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use b2c_ir::diag::Part;
use b2c_model::Document;
use common::*;
use serde_json::{Value, json};

/// The catalog's and the analyser's diagnostics for a document, as the build
/// lists them: `code block part`.
fn checked(document: &Document) -> Vec<String> {
    let report = b2c_catalog::resolve_report(document, b2c_catalog::core_catalog());
    let analysis = analyze_checked(&report.document, &report.diagnostics, &report.unchecked);
    report
        .diagnostics
        .iter()
        .chain(&analysis.diagnostics)
        .map(|d| {
            let block = d.primary.block.as_ref().map_or("", |b| b.as_str());
            match &d.primary.part {
                Part::Field { name } | Part::Input { name } => format!("{} {block} {name}", d.code.0),
                _ => format!("{} {block}", d.code.0),
            }
        })
        .collect()
}

/// A block with this ID.
fn with_id(mut block: Value, id: &str) -> Value {
    block["id"] = json!(id);
    block
}

/// A field or input the catalog rejects is the catalog's error alone: the
/// analyser's `B2C-E0430` about the same field or input is left out.
#[test]
fn a_part_the_catalog_rejects_is_reported_once() {
    let mut bogus = with_id(print(vec![text("hi")]), "p1");
    bogus["fields"] = json!({"SEP": "bogus"});
    let mut float = with_id(declare("int", "y", Some(num("1"))), "d1");
    float["fields"]["TYPE"] = json!("float");
    let mut text_var = with_id(ask("x", None), "a1");
    text_var["fields"]["VAR"] = json!("x");
    let mut no_value = with_id(set("x", num("2")), "s1");
    no_value.as_object_mut().unwrap().remove("inputs");
    let found = checked(&doc(vec![main(vec![
        declare("int", "x", Some(num("0"))),
        bogus,
        float,
        text_var,
        no_value,
    ])]));
    assert_eq!(
        found,
        [
            "B2C-E0607 p1 SEP",
            "B2C-E0607 d1 TYPE",
            "B2C-E0607 a1 VAR",
            "B2C-E0609 s1 VALUE",
        ]
    );
}

/// A function input of the wrong type is a problem of the block's `extra`
/// settings for the catalog (`B2C-E0614`, on the whole block): the analyser
/// does not report the same input again.
#[test]
fn a_function_input_the_catalog_rejects_is_reported_once() {
    let mut function = with_id(func("helper", "void", &[("n", "int", "copy")], vec![]), "f1");
    function["extra"]["params"][0]["type"] = json!("float");
    let found = checked(&doc(vec![main(vec![]), function]));
    assert_eq!(found, ["B2C-E0614 f1"]);
}

/// The analyser still reports what the catalog does not check, also in a
/// block whose field the catalog rejected.
#[test]
fn other_problems_of_the_block_are_still_reported() {
    let mut ask = with_id(ask("ghost", None), "a1");
    ask["fields"]["MODE"] = json!("bogus");
    let found = checked(&doc(vec![main(vec![ask])]));
    assert_eq!(found, ["B2C-E0607 a1 MODE", "B2C-E0201 a1 VAR"]);
}

/// A block the catalog keeps as it is, without a check (made by a newer
/// catalog, `B2C-E0602`, or too old to upgrade, `B2C-E0603`), is that
/// error alone: a choice this catalog does not know, or an input left to a
/// default it did not fill in, is no `B2C-E0430` of its own.
#[test]
fn blocks_the_catalog_keeps_unchecked_are_not_reported_again() {
    let mut pow = arith("add", num("2"), num("3"));
    pow["block"]["id"] = json!("m1");
    pow["block"]["fields"]["OP"] = json!("pow");
    let mut tabs = with_id(print(vec![pow]), "p1");
    tabs["fields"] = json!({"SEP": "tabs"});
    let mut power = with_id(change("x", num("2")), "c1");
    power["fields"]["OP"] = json!("pow");
    power.as_object_mut().unwrap().remove("inputs");
    let mut newer = doc(vec![main(vec![declare("int", "x", Some(num("1"))), tabs, power])]);
    newer.generator.catalog = String::from("99.0.0");
    assert_eq!(checked(&newer), ["B2C-E0602 p1", "B2C-E0602 m1", "B2C-E0602 c1"]);

    let mut old = with_id(print(vec![text("hi")]), "p1");
    old["v"] = json!(0);
    old["fields"] = json!({"SEP": "bogus"});
    assert_eq!(checked(&doc(vec![main(vec![old])])), ["B2C-E0603 p1"]);
}

/// Without catalog findings, `analyze_checked_with` is `analyze_with`.
#[test]
fn without_catalog_findings_nothing_is_left_out() {
    let mut no_var = ask("x", None);
    no_var.as_object_mut().unwrap().remove("fields");
    let document = doc(vec![main(vec![no_var])]);
    let plain = analyze(&document);
    let checked = analyze_checked(&document, &[], &std::collections::BTreeSet::new());
    assert_eq!(plain.diagnostics, checked.diagnostics);
    assert_eq!(plain.program, checked.program);
    assert_eq!(codes(&checked), ["B2C-E0430"]);
}
