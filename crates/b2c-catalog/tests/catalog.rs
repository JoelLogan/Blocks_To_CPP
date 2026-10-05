//! The built-in catalog loads, contains every block of `catalog/core/`, and
//! every block can be used in a project that resolves without problems.

// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_pass_by_value,
    reason = "test helpers fail the test by panicking and take json! values"
)]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use b2c_catalog::{
    BlockDef, CATALOG_VERSION, CatalogFile, ExtraKind, FieldDefault, FieldKind, OutputType, Shape,
    core_catalog,
};
use common::{project, resolve};
use serde_json::{Map, Value, json};

/// Every block ID in the TOML files on disk.
fn ids_on_disk() -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let dir = common::repo_root().join("catalog/core");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        assert_eq!(path.extension().unwrap(), "toml", "{}", path.display());
        let file: CatalogFile = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for block in file.block {
            assert!(ids.insert(block.id.clone()), "{} is defined twice", block.id);
        }
    }
    ids
}

#[test]
fn the_core_catalog_loads_every_block_on_disk() {
    let catalog = core_catalog();
    assert_eq!(catalog.version, CATALOG_VERSION);
    let embedded: BTreeSet<String> = catalog.blocks.keys().cloned().collect();
    // A TOML file added to catalog/core/ but not embedded fails here.
    assert_eq!(embedded, ids_on_disk());
    assert_eq!(embedded.len(), 32);
    assert!(std::ptr::eq(catalog, core_catalog()), "the catalog is cached");
}

#[test]
fn only_hats_and_definitions_are_top_level() {
    for def in core_catalog().blocks.values() {
        let top_level = def.shape.is_top_level();
        assert_eq!(
            top_level,
            matches!(def.shape, Shape::Hat | Shape::Definition),
            "{}",
            def.id
        );
        if def.shape.is_value() {
            assert!(def.statement.is_empty(), "{}", def.id);
        }
    }
}

/// The output type of every value block is pinned: the editor's connection
/// checker and the generated block definitions rely on it.
#[test]
fn value_blocks_declare_their_output_type() {
    use OutputType as O;
    let expected: BTreeMap<&str, OutputType> = [
        ("func.call", O::Symbol),
        ("logic.boolean", O::Bool),
        ("logic.not", O::Bool),
        ("logic.operation", O::Bool),
        ("logic.ternary", O::Any),
        ("math.arithmetic", O::Number),
        ("math.compare", O::Bool),
        ("math.convert", O::Field("TO".into())),
        ("math.number", O::Number),
        ("math.random_int", O::Int),
        ("text.char", O::Char),
        ("text.join", O::StdString),
        ("text.literal", O::StdString),
        ("var.get", O::Symbol),
    ]
    .into_iter()
    .collect();
    let actual: BTreeMap<&str, OutputType> = core_catalog()
        .blocks
        .values()
        .filter_map(|def| Some((def.id.as_str(), def.output.clone()?)))
        .collect();
    assert_eq!(actual, expected);
    for def in core_catalog().blocks.values() {
        assert_eq!(def.output.is_some(), def.shape.is_value(), "{}", def.id);
        if def.shape == Shape::Predicate {
            assert_eq!(def.output, Some(O::Bool), "{}", def.id);
        }
    }
}

/// A minimal valid instance of a block: required fields and extras set,
/// value inputs left to their defaults, required ones given an expression.
fn instance(def: &BlockDef, index: usize) -> Value {
    let mut fields = Map::new();
    for field in &def.field {
        if field.default.is_some() {
            continue;
        }
        let value = match field.kind {
            FieldKind::Dropdown => json!(field.options[0][1]),
            FieldKind::Checkbox => json!(true),
            FieldKind::Text => json!("a"),
            FieldKind::Number => json!("1"),
            FieldKind::Type => json!(field.types[0]),
            FieldKind::SymbolDecl => json!({"sym": format!("s{index}_{}", field.name), "name": "x"}),
            FieldKind::SymbolRef => json!({"ref": "s_any"}),
        };
        fields.insert(field.name.clone(), value);
    }
    let mut extra = Map::new();
    for def_extra in &def.extra {
        match (def_extra.kind, &def_extra.default) {
            (ExtraKind::Params, _) => {
                let row = json!({"sym": format!("p{index}"), "name": "p", "type": def_extra.types[0], "mode": "copy"});
                extra.insert(def_extra.name.clone(), json!([row]));
            }
            (ExtraKind::Count, None) => {
                extra.insert(def_extra.name.clone(), json!(def_extra.min));
            }
            (ExtraKind::Flag, None) => {
                extra.insert(def_extra.name.clone(), json!(true));
            }
            (_, Some(FieldDefault::Text(_) | FieldDefault::Bool(_))) => {}
        }
    }
    let mut inputs = Map::new();
    for input in &def.input {
        if input.repeat.is_none() && input.default.is_empty() && !input.optional {
            inputs.insert(input.name.clone(), json!({"expr": [{"num": "1"}]}));
        }
    }
    json!({
        "id": format!("b{index}"),
        "type": def.id,
        "v": def.version,
        "extra": extra,
        "fields": fields,
        "inputs": inputs,
    })
}

#[test]
fn every_block_resolves_cleanly_in_its_place() {
    let mut body = Vec::new();
    let mut top = Vec::new();
    let mut values = Vec::new();
    for (index, def) in core_catalog().blocks.values().enumerate() {
        let block = instance(def, index);
        match def.shape {
            Shape::Hat if def.id == "program.main" => {}
            Shape::Hat | Shape::Definition => top.push(block),
            Shape::Statement => body.push(block),
            Shape::Reporter | Shape::Predicate => values.push(block),
        }
    }
    let value_count = values.len();
    let inputs: Map<String, Value> = values
        .into_iter()
        .enumerate()
        .map(|(i, block)| (format!("ITEM{i}"), json!({ "block": block })))
        .collect();
    body.push(json!({
        "id": "b_print_all",
        "type": "io.print",
        "v": 1,
        "extra": { "itemCount": value_count },
        "inputs": inputs,
    }));
    let (document, diagnostics) = resolve(&project(Value::Array(body), Value::Array(top)));
    assert_eq!(diagnostics, []);
    // Every block type appears in the completed document.
    let mut seen = BTreeSet::new();
    let mut stack: Vec<&b2c_model::Block> = document.modules[0].workspace.blocks.iter().collect();
    while let Some(block) = stack.pop() {
        seen.insert(block.block_type.clone());
        for input in block.inputs.values() {
            if let b2c_model::Input::Block(nested) = input {
                stack.push(&nested.block);
            }
        }
        stack.extend(block.statements.values().flatten());
    }
    let all: BTreeSet<String> = core_catalog().blocks.keys().cloned().collect();
    assert_eq!(seen, all);
}
