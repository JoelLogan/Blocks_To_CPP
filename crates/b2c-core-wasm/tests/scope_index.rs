//! The scope index (`scope_index()`, docs/plans/m3-contracts/worker.md §4):
//! for every example, every file of the security corpus and the generated
//! 1,000-block documents, a lookup over the index alone, written here as the
//! editor's `ScopeIndex` does it, gives exactly the scope query's answers:
//! `symbols_in_scope` at every block with no input, each of its inputs and a
//! name it lacks, `scope_after` after every block and `parked_scope` for
//! every module.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests fail by panicking"
)]

mod common;

#[allow(dead_code, unreachable_pub, reason = "the benchmark's generator, shared")]
#[path = "../benches/pipeline/document.rs"]
mod document;

use std::collections::{BTreeSet, HashMap};

use b2c_ir::{BlockId, ModuleId, SymbolInfo};
use common::{examples, native_analysis, parse, repo_root};
use document::{Shape, Units, generate};
use serde_json::Value;

/// The index's lookup: what the editor's `ScopeIndex` does with it.
struct Lookup {
    index: Value,
    /// The update's symbols.
    symbols: Vec<Value>,
    blocks: HashMap<String, Value>,
    members: HashMap<i64, Vec<i64>>,
}

impl Lookup {
    fn new(index: Value, update: &Value) -> Self {
        let blocks = index["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| (entry[0].as_str().unwrap().to_owned(), entry.clone()))
            .collect();
        let members = index["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                let list: Vec<i64> = entry
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_i64().unwrap())
                    .collect();
                (list[0], list[1..].to_vec())
            })
            .collect();
        Self {
            index,
            symbols: update["symbols"].as_array().unwrap().clone(),
            blocks,
            members,
        }
    }

    fn numbers(&self, key: &str) -> Vec<i64> {
        self.index[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_i64().unwrap())
            .collect()
    }

    /// The symbols visible at `(node, module)`, sorted as the scope query sorts.
    fn visible(&self, node: i64, module: i64) -> Vec<Value> {
        let trail = self.numbers("trail");
        let mut names = BTreeSet::new();
        let mut found = Vec::new();
        let mut at = node;
        while at >= 0 {
            let i = usize::try_from(at).unwrap();
            let Some(step) = trail.get(i * 3..i * 3 + 3) else {
                break;
            };
            if names.insert(step[1]) && step[2] >= 0 {
                found.push(step[2]);
            }
            at = if step[0] < at { step[0] } else { -1 };
        }
        if module >= 0
            && let Some(functions) = self.index["functions"].get(usize::try_from(module).unwrap())
        {
            let list: Vec<i64> = functions
                .as_array()
                .unwrap()
                .iter()
                .map(|n| n.as_i64().unwrap())
                .collect();
            for pair in list.chunks(2) {
                if !names.contains(&pair[0]) && pair[1] >= 0 {
                    found.push(pair[1]);
                }
            }
        }
        let mut out = BTreeSet::new();
        for symbol in found {
            out.insert(symbol);
            out.extend(self.members.get(&symbol).into_iter().flatten().copied());
        }
        // Symbol indexes follow the update's order, which is the answer's.
        out.into_iter()
            .map(|symbol| self.symbols[usize::try_from(symbol).unwrap()].clone())
            .collect()
    }

    fn at(&self, block: &str, input: Option<&str>) -> Option<Vec<Value>> {
        let entry = self.blocks.get(block)?.as_array()?;
        let mut position = (entry[1].as_i64()?, entry[2].as_i64()?);
        if let Some(name) = input {
            for cell in entry[3..].chunks(3) {
                if cell[0].as_str() == Some(name) {
                    position = (cell[1].as_i64()?, cell[2].as_i64()?);
                }
            }
        }
        if position.1 < 0 {
            return None;
        }
        Some(self.visible(position.0, position.1))
    }

    fn after(&self, block: &str) -> Option<Vec<Value>> {
        let mut visible = self.at(block, None)?;
        let created = self.index["created"]
            .as_array()
            .unwrap()
            .iter()
            .find(|pair| pair[0].as_str() == Some(block))
            .map(|pair| self.symbols[usize::try_from(pair[1].as_i64().unwrap()).unwrap()].clone());
        if let Some(own) = created {
            visible.retain(|info| {
                info["name"] != own["name"] || matches!(info["kind"].as_str(), Some("field" | "enumerator"))
            });
            visible.push(own);
            visible.sort_by(|a, b| {
                (a["name"].as_str().unwrap(), a["id"].as_str().unwrap())
                    .cmp(&(b["name"].as_str().unwrap(), b["id"].as_str().unwrap()))
            });
        }
        Some(visible)
    }

    fn parked(&self, module: &str) -> Vec<Value> {
        let modules = self.index["modules"].as_array().unwrap();
        match modules.iter().position(|m| m.as_str() == Some(module)) {
            Some(index) => self.visible(self.numbers("parked")[index], i64::try_from(index).unwrap()),
            None => Vec::new(),
        }
    }
}

/// Every block ID of a document, with the names of its inputs and statement lists.
fn blocks_of(document: &Value) -> Vec<(String, Vec<String>)> {
    let mut found = Vec::new();
    let mut pending: Vec<&Value> = Vec::new();
    for module in document["modules"].as_array().into_iter().flatten() {
        pending.extend(module["workspace"]["blocks"].as_array().into_iter().flatten());
    }
    while let Some(block) = pending.pop() {
        let Some(id) = block["id"].as_str() else {
            continue;
        };
        let mut inputs = Vec::new();
        for key in ["inputs", "statements"] {
            if let Some(map) = block[key].as_object() {
                for (name, value) in map {
                    inputs.push(name.clone());
                    match value {
                        Value::Array(list) => pending.extend(list),
                        Value::Object(_) => pending.extend(value.get("block")),
                        _ => {}
                    }
                }
            }
        }
        pending.extend(block["stack"].as_array().into_iter().flatten());
        found.push((id.to_owned(), inputs));
    }
    found
}

fn json(infos: &[SymbolInfo]) -> Vec<Value> {
    infos
        .iter()
        .map(|info| serde_json::to_value(info).unwrap())
        .collect()
}

/// Checks the index of `text` against the scope query; returns how many
/// questions were asked.
fn check(what: &str, text: &str) -> usize {
    let update = parse(&b2c_core_wasm::update(text, r#"{"indentWidth":4}"#));
    let output = b2c_core_wasm::scope_index();
    if update["contentHash"].is_null() {
        return 0;
    }
    assert!(!output.contains('\n'), "{what}: not compact");
    let index: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(index["v"], 1, "{what}");
    assert_eq!(index["contentHash"], update["contentHash"], "{what}");
    let trail = index["trail"].as_array().unwrap();
    assert_eq!(trail.len() % 3, 0, "{what}");
    let names = index["names"].as_array().unwrap().len();
    let symbols = update["symbols"].as_array().unwrap().len();
    for (node, step) in trail.chunks(3).enumerate() {
        let parent = step[0].as_i64().unwrap();
        assert!(
            parent < i64::try_from(node).unwrap() && parent >= -1,
            "{what}: parent of {node}"
        );
        assert!(
            usize::try_from(step[1].as_i64().unwrap()).unwrap() < names,
            "{what}"
        );
        assert!(
            step[2].as_i64().unwrap() < i64::try_from(symbols).unwrap(),
            "{what}"
        );
    }
    let lookup = Lookup::new(index, &update);
    let analysis = native_analysis(text);
    let document: Value = serde_json::from_str(text).unwrap();
    let mut asked = 0;
    for (block, inputs) in blocks_of(&document) {
        let id = BlockId::new(&block).unwrap();
        for input in std::iter::once(None)
            .chain(inputs.iter().map(|name| Some(name.as_str())))
            .chain([Some("NOPE")])
        {
            let expected = analysis.scope_at(&id, input).map(|found| json(&found));
            assert_eq!(lookup.at(&block, input), expected, "{what}: {block} {input:?}");
            asked += 1;
        }
        let expected = analysis.scope_after(&id).map(|found| json(&found));
        assert_eq!(lookup.after(&block), expected, "{what}: after {block}");
    }
    for module in document["modules"].as_array().into_iter().flatten() {
        let id = module["id"].as_str().unwrap();
        let expected = json(&analysis.parked_scope(&ModuleId::new(id).unwrap()));
        assert_eq!(lookup.parked(id), expected, "{what}: parked {id}");
    }
    assert!(lookup.parked("nope").is_empty());
    asked
}

#[test]
fn null_before_an_update_and_kept_after_one_that_does_not_load() {
    // Each test runs on a thread of its own, so its session starts empty.
    assert_eq!(b2c_core_wasm::scope_index(), "null");
    let text = common::example("guessing_game");
    let _ = b2c_core_wasm::update(&text, r#"{"indentWidth":4}"#);
    let kept = b2c_core_wasm::scope_index();
    assert_ne!(kept, "null");
    let _ = b2c_core_wasm::update("{not a project", r#"{"indentWidth":4}"#);
    assert_eq!(b2c_core_wasm::scope_index(), kept);
}

#[test]
fn every_example_answers_as_the_scope_query() {
    let mut asked = 0;
    for example in examples() {
        asked += check(&example.name, &example.text);
    }
    assert!(asked > 500, "{asked}");
}

#[test]
fn the_security_corpus_answers_as_the_scope_query() {
    let mut paths: Vec<_> = std::fs::read_dir(repo_root().join("tests/security/projects"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "b2c"))
        .collect();
    paths.sort();
    assert!(paths.len() > 50);
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        // A file that does not load has no index; one that is not JSON at
        // all is the loader's business, not the index's.
        if b2c_model::load(text.as_bytes()).is_err() {
            continue;
        }
        check(&path.display().to_string(), &text);
    }
}

#[test]
fn the_generated_documents_answer_as_the_scope_query() {
    for units in [Units::Plain, Units::Collections, Units::Language] {
        let shape = Shape {
            blocks: 1_000,
            drag_handle: false,
            units,
        };
        let document = generate(shape).unwrap();
        let asked = check(&format!("{units:?}"), &document.to_string());
        assert!(asked >= 1_000, "{units:?}: {asked}");
    }
}

#[test]
fn the_names_are_listed_once() {
    let _ = b2c_core_wasm::update(&common::example("guessing_game"), r#"{"indentWidth":4}"#);
    let index: Value = serde_json::from_str(&b2c_core_wasm::scope_index()).unwrap();
    let names: Vec<&str> = index["names"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_str().unwrap())
        .collect();
    let unique: BTreeSet<&str> = names.iter().copied().collect();
    assert_eq!(unique.len(), names.len());
}
