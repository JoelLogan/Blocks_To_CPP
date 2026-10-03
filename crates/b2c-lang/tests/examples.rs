//! The example projects (`examples/*.b2c`) analyse without errors, and the
//! programs they become generate C++ that g++ accepts.
//!
//! The examples are parsed with plain `serde_json` and completed here the way
//! `b2c_catalog::resolve` completes them: value inputs that are absent get
//! the catalog default (`default = [...]` in `catalog/core/*.toml`).
//!
//! The g++ check runs when g++ is on `PATH`. When it is not, the test prints a
//! note and passes, unless the environment variable `B2C_REQUIRE_GXX` is set
//! (CI sets it), in which case it fails.

// Test code: unwrap/expect/panic and printing to stderr are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use b2c_codegen::{CodegenOptions, generate};
use b2c_ir::diag::Severity;
use b2c_ir::source_map::FileKind;
use b2c_lang::{Analysis, analyze};
use b2c_model::Document;
use serde_json::Value;

/// The repository root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A value input with a catalog default.
#[derive(Debug, PartialEq)]
struct DefaultInput {
    /// Input name (without the number of a repeated input).
    name: String,
    /// For repeated inputs: the `extra` count key, the number added to it and
    /// the count's own default.
    repeat: Option<(String, u64, u64)>,
    /// The default tokens.
    tokens: Value,
}

/// Inputs with defaults, by block type, read from `catalog/core/*.toml`.
fn catalog_defaults() -> BTreeMap<String, Vec<DefaultInput>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(root().join("catalog/core"))
        .expect("catalog folder")
        .map(|entry| entry.expect("catalog entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no catalog files found");
    let mut defaults = BTreeMap::new();
    for path in paths {
        let text = std::fs::read_to_string(&path).expect("catalog file");
        let file: toml::Table = toml::from_str(&text).expect("catalog TOML");
        for block in file["block"].as_array().expect("[[block]]") {
            let id = block["id"].as_str().expect("block id").to_owned();
            let extra_default = |key: &str| -> u64 {
                block
                    .get("extra")
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .find(|e| e["name"].as_str() == Some(key))
                    .and_then(|e| e.get("default"))
                    .and_then(toml::Value::as_str)
                    .and_then(|d| d.parse().ok())
                    .unwrap_or(0)
            };
            let mut inputs = Vec::new();
            for input in block
                .get("input")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(tokens) = input
                    .get("default")
                    .filter(|d| d.as_array().is_some_and(|a| !a.is_empty()))
                else {
                    continue;
                };
                let repeat = input.get("repeat").map(|r| {
                    let key = r["count"].as_str().expect("repeat count").to_owned();
                    let plus = r
                        .get("plus")
                        .and_then(toml::Value::as_integer)
                        .and_then(|p| u64::try_from(p).ok())
                        .unwrap_or(0);
                    let default = extra_default(&key);
                    (key, plus, default)
                });
                inputs.push(DefaultInput {
                    name: input["name"].as_str().expect("input name").to_owned(),
                    repeat,
                    tokens: serde_json::to_value(tokens).expect("tokens as JSON"),
                });
            }
            defaults.insert(id, inputs);
        }
    }
    defaults
}

/// Fills absent inputs of a block (and the blocks inside it) with defaults.
fn fill_defaults(block: &mut Value, defaults: &BTreeMap<String, Vec<DefaultInput>>) {
    let block_type = block["type"].as_str().unwrap_or_default().to_owned();
    for input in defaults.get(&block_type).into_iter().flatten() {
        let names: Vec<String> = match &input.repeat {
            None => vec![input.name.clone()],
            Some((key, plus, default)) => {
                let count = block["extra"][key].as_u64().unwrap_or(*default) + plus;
                (0..count).map(|i| format!("{}{i}", input.name)).collect()
            }
        };
        for name in names {
            let inputs = block
                .as_object_mut()
                .expect("block object")
                .entry("inputs")
                .or_insert_with(|| Value::Object(serde_json::Map::new()));
            let inputs = inputs.as_object_mut().expect("inputs object");
            inputs
                .entry(name)
                .or_insert_with(|| serde_json::json!({ "expr": input.tokens.clone() }));
        }
    }
    if let Some(inputs) = block.get_mut("inputs").and_then(Value::as_object_mut) {
        for input in inputs.values_mut() {
            if let Some(nested) = input.get_mut("block") {
                fill_defaults(nested, defaults);
            }
        }
    }
    if let Some(lists) = block.get_mut("statements").and_then(Value::as_object_mut) {
        for list in lists.values_mut() {
            for nested in list.as_array_mut().into_iter().flatten() {
                fill_defaults(nested, defaults);
            }
        }
    }
}

/// Every example: its file name and the completed document.
fn examples() -> Vec<(String, Document)> {
    let defaults = catalog_defaults();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(root().join("examples"))
        .expect("examples folder")
        .map(|entry| entry.expect("examples entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "b2c"))
        .collect();
    paths.sort();
    assert!(
        paths.len() >= 15,
        "expected the M1 examples, found {}",
        paths.len()
    );
    paths
        .into_iter()
        .map(|path| {
            let name = path
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            let text = std::fs::read_to_string(&path).expect("example file");
            let mut json: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
            for module in json["modules"].as_array_mut().expect("modules") {
                for block in module["workspace"]["blocks"].as_array_mut().into_iter().flatten() {
                    fill_defaults(block, &defaults);
                }
            }
            let document: Document = serde_json::from_value(json).unwrap_or_else(|e| panic!("{name}: {e}"));
            (name, document)
        })
        .collect()
}

/// Renders the diagnostics of an analysis, one per line.
fn render(analysis: &Analysis) -> String {
    analysis
        .diagnostics
        .iter()
        .map(|d| {
            let block = d.primary.block.as_ref().map_or("project", |b| b.as_str());
            format!("{:?} {} at {block}: {}", d.severity, d.code.0, d.message)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn defaults_follow_the_catalog() {
    let defaults = catalog_defaults();
    let print = &defaults["io.print"];
    assert_eq!(print.len(), 1);
    assert_eq!(print[0].name, "ITEM");
    assert_eq!(print[0].repeat, Some((String::from("itemCount"), 0, 1)));
    let mut block = serde_json::json!({"id": "p", "type": "io.print", "v": 1,
        "inputs": {"ITEM0": {"expr": [{"num": "1"}]}}, "extra": {"itemCount": 2}});
    fill_defaults(&mut block, &defaults);
    assert_eq!(
        block["inputs"]["ITEM0"],
        serde_json::json!({"expr": [{"num": "1"}]})
    );
    assert_eq!(
        block["inputs"]["ITEM1"],
        serde_json::json!({"expr": [{"str": "Hello, world!"}]})
    );
    let mut exit = serde_json::json!({"id": "x", "type": "program.exit", "v": 1});
    fill_defaults(&mut exit, &defaults);
    assert_eq!(
        exit["inputs"]["CODE"],
        serde_json::json!({"expr": [{"num": "0"}]})
    );
    // Optional inputs without a default stay absent.
    let mut declare = serde_json::json!({"id": "d", "type": "var.declare", "v": 1});
    fill_defaults(&mut declare, &defaults);
    assert!(declare.get("inputs").is_none_or(|i| i.get("VALUE").is_none()));
    let mut branches =
        serde_json::json!({"id": "i", "type": "control.if", "v": 1, "extra": {"elseIfCount": 1}});
    fill_defaults(&mut branches, &defaults);
    assert_eq!(
        branches["inputs"]["COND1"],
        serde_json::json!({"expr": [{"kw": "true"}]})
    );
    assert!(branches["inputs"].get("COND2").is_none());
}

#[test]
fn examples_analyse_without_errors_or_warnings() {
    let mut problems = Vec::new();
    for (name, document) in examples() {
        let analysis = analyze(&document);
        if !analysis.diagnostics.is_empty() {
            problems.push(format!("{name}:\n{}", render(&analysis)));
        }
        let errors = analysis
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .count();
        assert_eq!(errors, 0, "{name} has errors:\n{}", render(&analysis));
        assert_eq!(
            analysis.program.standard, document.project.language.standard,
            "{name}"
        );
        assert_eq!(analysis.program.modules.len(), document.modules.len(), "{name}");
        // Analysing twice gives the same result.
        assert_eq!(
            analyze(&document),
            analysis,
            "{name}: analysis is not deterministic"
        );
    }
    assert!(
        problems.is_empty(),
        "examples with diagnostics:\n{}",
        problems.join("\n")
    );
}

#[test]
fn examples_generate_cpp_that_compiles() {
    if !common::gxx::available() {
        return;
    }
    for (name, document) in examples() {
        let analysis = analyze(&document);
        let options = CodegenOptions {
            project_name: document.project.name.clone(),
            ..CodegenOptions::default()
        };
        let project = generate(&analysis.program, &options);
        let sources: Vec<_> = project
            .files
            .iter()
            .filter(|file| file.kind == FileKind::Source)
            .collect();
        assert!(!sources.is_empty(), "{name}: no source files");
        for file in sources {
            assert!(
                !file.contents.contains("/* error */"),
                "{name}: {} has error placeholders:\n{}",
                file.path,
                file.contents
            );
            if let Err(messages) = common::gxx::syntax_check(&file.contents, true) {
                panic!(
                    "g++ rejected {name} ({}):\n{messages}\n--- source ---\n{}",
                    file.path, file.contents
                );
            }
        }
    }
}
