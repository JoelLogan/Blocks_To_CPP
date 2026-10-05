//! The analyser, and the questions the editor asks of an analysis, never
//! panic, also on documents that the resolve stage rejects: the editor's
//! preview analyses whatever loads (spec §6.1).
//!
//! The inputs are every file of the malicious-project suite
//! (`tests/security/projects/`) that the loader accepts, the examples with a
//! dangling reference added, and the examples with each block in turn
//! changed to every catalog block type (and to an unknown one), stripped of
//! its fields, inputs, statement lists or mutator state, disabled, or moved
//! to the top level. Each document is analysed as loaded and as resolved.

// Test code: unwrap/expect/panic and printing to stderr are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use std::collections::BTreeSet;

use b2c_ir::{BlockId, SymbolId};
use b2c_lang::{Analysis, analyze};
use b2c_model::Document;
use common::*;
use serde_json::{Value, json};

/// Analyses a document and asks every question the editor asks of it,
/// checking that the answers are well-formed.
fn exercise(document: &Document, what: &str) -> Analysis {
    let analysis = analyze(document);
    assert!(
        analyze(document) == analysis,
        "{what}: analysing twice gave different results"
    );
    let symbols = analysis.symbol_infos();
    let known: BTreeSet<&SymbolId> = symbols.iter().map(|s| &s.id).collect();
    assert_eq!(known.len(), symbols.len(), "{what}: a symbol listed twice");
    let blocks = all_blocks(document);
    let ids: BTreeSet<&BlockId> = blocks.iter().map(|(_, _, block)| &block.id).collect();
    for (_, top, block) in &blocks {
        let mut inputs: Vec<Option<&str>> = vec![None, Some("NO_SUCH_INPUT")];
        inputs.extend(block.statements.keys().map(|name| Some(name.as_str())));
        inputs.extend(block.inputs.keys().map(|name| Some(name.as_str())));
        inputs.extend(block.fields.keys().map(|name| Some(name.as_str())));
        for input in inputs {
            let found = analysis.symbols_in_scope(&block.id, input);
            let keys: Vec<(&str, &SymbolId)> = found.iter().map(|s| (s.name.as_str(), &s.id)).collect();
            let mut sorted = keys.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(
                keys, sorted,
                "{what}: {} {input:?} is not sorted or has duplicates",
                block.id
            );
            assert!(
                found.iter().all(|s| known.contains(&s.id)),
                "{what}: {} {input:?} lists a symbol the program does not have",
                block.id
            );
            let item = matches!(block.block_type.as_str(), "program.main" | "func.define");
            assert!(
                !*top || item || found.is_empty(),
                "{what}: the unattached block {} sees symbols",
                block.id
            );
        }
    }
    let types = analysis.block_types();
    assert!(
        types.keys().all(|id| ids.contains(id)),
        "{what}: a typed block that is not in the document"
    );
    analysis
}

/// Analyses the loaded document and the resolved one.
fn exercise_loaded(bytes: &[u8], what: &str) -> Option<Analysis> {
    let loaded = b2c_model::load(bytes).ok()?;
    exercise(&loaded, what);
    let (resolved, _) = b2c_catalog::resolve(&loaded, b2c_catalog::core_catalog());
    Some(exercise(&resolved, what))
}

#[test]
fn the_malicious_projects_that_load() {
    let mut analysed = 0;
    for path in project_files("tests/security/projects") {
        let name = file_name(&path);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        if exercise_loaded(&bytes, &name).is_some() {
            analysed += 1;
        }
    }
    assert!(analysed >= 20, "only {analysed} files of the suite load");
}

/// The examples as JSON.
fn example_values() -> Vec<(String, Value)> {
    project_files("examples")
        .into_iter()
        .map(|path| {
            let name = file_name(&path);
            let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            let value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, value)
        })
        .collect()
}

#[test]
fn examples_with_a_dangling_reference() {
    for (name, mut value) in example_values() {
        let blocks = value["modules"][0]["workspace"]["blocks"]
            .as_array_mut()
            .expect("blocks");
        let main = blocks
            .iter_mut()
            .find(|b| b["type"] == "program.main")
            .unwrap_or_else(|| panic!("{name}: no main"));
        let body = main["statements"]["BODY"].as_array_mut().expect("BODY");
        body.insert(
            0,
            json!({"id": "x_print", "type": "io.print", "v": 1, "extra": {"itemCount": 1},
                   "inputs": {"ITEM0": {"expr": [{"ref": "s_dangling"}, {"op": "+"}, {"num": "1"}]}}}),
        );
        body.push(
            json!({"id": "x_set", "type": "var.set", "v": 1, "fields": {"VAR": {"ref": "s_gone"}},
                         "inputs": {"VALUE": {"block": {"id": "x_get", "type": "var.get", "v": 1,
                                                        "fields": {"VAR": {"ref": "s_dangling"}}}}}}),
        );
        let bytes = serde_json::to_vec(&value).expect("json");
        let analysis = exercise_loaded(&bytes, &name).unwrap_or_else(|| panic!("{name} does not load"));
        let missing = analysis
            .diagnostics
            .iter()
            .filter(|d| d.code.0 == "B2C-E0201")
            .count();
        assert_eq!(missing, 3, "{name}: {}", render_all(&analysis));
    }
}

/// JSON pointers to every block of a project, parents first.
fn block_pointers(value: &Value) -> Vec<String> {
    fn escape(key: &str) -> String {
        key.replace('~', "~0").replace('/', "~1")
    }
    let mut found = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    for (m, module) in value["modules"].as_array().into_iter().flatten().enumerate() {
        for b in (0..module["workspace"]["blocks"].as_array().map_or(0, Vec::len)).rev() {
            stack.push(format!("/modules/{m}/workspace/blocks/{b}"));
        }
    }
    while let Some(pointer) = stack.pop() {
        let block = value.pointer(&pointer).expect("block");
        let mut children = Vec::new();
        for (list, blocks) in block["statements"].as_object().into_iter().flatten() {
            for i in 0..blocks.as_array().map_or(0, Vec::len) {
                children.push(format!("{pointer}/statements/{}/{i}", escape(list)));
            }
        }
        for (input, content) in block["inputs"].as_object().into_iter().flatten() {
            if content.get("block").is_some() {
                children.push(format!("{pointer}/inputs/{}/block", escape(input)));
            }
        }
        found.push(pointer);
        stack.extend(children.into_iter().rev());
    }
    found
}

/// Damages the block at a JSON pointer of a document.
type Mutation = Box<dyn Fn(&mut Value, &str)>;

/// The ways one block of a document is damaged, with labels for messages.
fn mutations() -> Vec<(String, Mutation)> {
    let mut all: Vec<(String, Mutation)> = Vec::new();
    let types: Vec<String> = b2c_catalog::core_catalog()
        .blocks
        .keys()
        .cloned()
        .chain([String::from("nope.unknown")])
        .collect();
    for block_type in types {
        let label = format!("type {block_type}");
        all.push((
            label,
            Box::new(move |doc, pointer| {
                doc.pointer_mut(pointer).expect("block")["type"] = json!(block_type);
            }),
        ));
    }
    for key in ["fields", "inputs", "statements", "extra"] {
        all.push((
            format!("without {key}"),
            Box::new(move |doc, pointer| {
                if let Some(block) = doc.pointer_mut(pointer).and_then(Value::as_object_mut) {
                    block.remove(key);
                }
            }),
        ));
    }
    all.push((
        String::from("disabled"),
        Box::new(|doc, pointer| doc.pointer_mut(pointer).expect("block")["disabled"] = json!(true)),
    ));
    all.push((
        String::from("big counts"),
        Box::new(|doc, pointer| {
            doc.pointer_mut(pointer).expect("block")["extra"] =
                json!({"itemCount": 64, "elseIfCount": 64, "argCount": 64, "hasElse": true});
        }),
    ));
    all.push((
        String::from("moved to the top level"),
        Box::new(|doc, pointer| {
            let Some((parent, last)) = pointer.rsplit_once('/') else {
                return;
            };
            if parent.ends_with("/workspace/blocks") {
                return;
            }
            let block = if last == "block" {
                // A value input: remove the whole input.
                let Some((inputs, input)) = parent.rsplit_once('/') else {
                    return;
                };
                let removed = doc
                    .pointer_mut(inputs)
                    .and_then(Value::as_object_mut)
                    .and_then(|map| map.remove(input));
                removed.map(|mut content| content["block"].take())
            } else {
                let index: usize = last.parse().expect("index");
                doc.pointer_mut(parent)
                    .and_then(Value::as_array_mut)
                    .map(|siblings| siblings.remove(index))
            };
            let module: String = pointer.split('/').take(3).collect::<Vec<_>>().join("/");
            if let Some(mut block) = block {
                block["x"] = json!(500);
                block["y"] = json!(500);
                if let Some(top) = doc
                    .pointer_mut(&format!("{module}/workspace/blocks"))
                    .and_then(Value::as_array_mut)
                {
                    top.push(block);
                }
            }
        }),
    ));
    all
}

#[test]
fn examples_with_a_damaged_block() {
    let mutations = mutations();
    let mut loaded = 0;
    let mut variants = 0;
    for (name, value) in example_values() {
        for pointer in block_pointers(&value) {
            for (label, mutate) in &mutations {
                let mut damaged = value.clone();
                mutate(&mut damaged, &pointer);
                variants += 1;
                let bytes = serde_json::to_vec(&damaged).expect("json");
                if exercise_loaded(&bytes, &format!("{name} {pointer} {label}")).is_some() {
                    loaded += 1;
                }
            }
        }
    }
    // Most variants load; resolve then rejects many of them.
    assert!(loaded * 2 > variants, "only {loaded} of {variants} variants load");
}
