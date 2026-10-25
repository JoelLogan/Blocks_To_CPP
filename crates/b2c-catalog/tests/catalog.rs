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

use b2c_catalog::{BlockDef, CATALOG_VERSION, ExtraKind, FieldDefault, FieldKind, Shape, core_catalog};
use common::{block_ids_on_disk, project, resolve};
use serde_json::{Map, Value, json};

#[test]
fn the_core_catalog_loads_every_block_on_disk() {
    let catalog = core_catalog();
    assert_eq!(catalog.version, CATALOG_VERSION);
    let embedded: BTreeSet<String> = catalog.blocks.keys().cloned().collect();
    // A TOML file added to catalog/core/ but not embedded fails here.
    assert_eq!(embedded, block_ids_on_disk());
    // M2's 31 blocks at least; W2's packages add theirs to their own files.
    assert!(embedded.len() >= 31, "{}", embedded.len());
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
/// checker and the generated block definitions rely on it. From catalog
/// 1.3.0 it is a type pattern, and `std::string` is written that way.
#[test]
fn value_blocks_declare_their_output_type() {
    let expected: BTreeMap<&str, &str> = [
        ("coll.contains", "bool"),
        ("coll.is_empty", "bool"),
        ("coll.item", "elem(C)"),
        ("coll.length", "int"),
        ("coll.list_of", "C"),
        ("coll.next", "elem(C)"),
        ("func.call", "symbol"),
        ("logic.boolean", "bool"),
        ("logic.not", "bool"),
        ("logic.operation", "bool"),
        ("logic.ternary", "any"),
        ("math.arithmetic", "number"),
        ("math.compare", "bool"),
        ("math.convert", "field:TO"),
        ("math.number", "number"),
        ("math.random_int", "int"),
        ("text.char", "char"),
        ("text.join", "std::string"),
        ("text.literal", "std::string"),
        ("var.get", "symbol"),
    ]
    .into_iter()
    .collect();
    let actual: BTreeMap<&str, String> = core_catalog()
        .blocks
        .values()
        .filter_map(|def| Some((def.id.as_str(), def.output.as_ref()?.to_string())))
        .collect();
    assert_eq!(
        actual,
        expected
            .into_iter()
            .map(|(id, output)| (id, output.to_owned()))
            .collect()
    );
    for def in core_catalog().blocks.values() {
        assert_eq!(def.output.is_some(), def.shape.is_value(), "{}", def.id);
        if def.shape == Shape::Predicate {
            assert_eq!(
                def.output.as_ref().map(ToString::to_string).as_deref(),
                Some("bool"),
                "{}",
                def.id
            );
        }
    }
}

/// A minimal valid instance of a block: required fields and extras set,
/// value inputs left to their defaults, required ones given an expression.
/// A row extra gets one row, and a count its default or minimum; a
/// repeated field gets a part for each that count gives.
fn instance(def: &BlockDef, index: usize) -> Value {
    let mut extra = Map::new();
    let mut counts = std::collections::BTreeMap::new();
    for def_extra in &def.extra {
        let name = def_extra.name.clone();
        match (def_extra.kind, &def_extra.default) {
            (ExtraKind::Params, _) => {
                let row = json!({"sym": format!("p{index}"), "name": "p", "type": def_extra.types[0], "mode": "copy"});
                extra.insert(name.clone(), json!([row]));
                counts.insert(name, 1);
            }
            (ExtraKind::Fields, _) => {
                let row = json!({"sym": format!("f{index}"), "name": "f", "type": def_extra.types[0]});
                extra.insert(name.clone(), json!([row]));
                counts.insert(name, 1);
            }
            (ExtraKind::Values, _) => {
                extra.insert(name.clone(), json!([{"sym": format!("v{index}"), "name": "V"}]));
                counts.insert(name, 1);
            }
            (ExtraKind::Count, None) => {
                extra.insert(name.clone(), json!(def_extra.min));
                counts.insert(name, def_extra.min);
            }
            (ExtraKind::Count, Some(FieldDefault::Text(text))) => {
                counts.insert(name, text.parse().unwrap_or(0));
            }
            (ExtraKind::Flag, None) => {
                extra.insert(name, json!(true));
            }
            (_, Some(FieldDefault::Text(_) | FieldDefault::Bool(_))) => {}
        }
    }
    let mut fields = Map::new();
    for field in &def.field {
        // A type field that lists only the module's types may be left
        // unchosen, as a name or a reference may.
        if field.default.is_some() || (field.kind == FieldKind::Type && field.types.is_empty()) {
            continue;
        }
        let names: Vec<String> = match &field.repeat {
            None => vec![field.name.clone()],
            Some(repeat) => {
                let count = counts.get(&repeat.count).copied().unwrap_or(0) + repeat.plus;
                (0..count).map(|i| format!("{}{i}", field.name)).collect()
            }
        };
        for name in names {
            let value = match field.kind {
                FieldKind::Dropdown => json!(field.options[0][1]),
                FieldKind::Checkbox => json!(true),
                FieldKind::Text => json!("a"),
                FieldKind::Number => json!("1"),
                FieldKind::Type => json!(field.types[0]),
                FieldKind::SymbolDecl => json!({"sym": format!("s{index}_{name}"), "name": "x"}),
                FieldKind::SymbolRef => json!({"ref": "s_any"}),
                FieldKind::Header => json!("<vector>"),
            };
            fields.insert(name, value);
        }
    }
    let mut inputs = Map::new();
    for input in &def.input {
        if !input.default.is_empty() || input.optional {
            continue;
        }
        let names: Vec<String> = match &input.repeat {
            None => vec![input.name.clone()],
            // A repeated part without a default (*list of*'s items) needs a
            // value too.
            Some(repeat) => {
                let count = counts.get(&repeat.count).copied().unwrap_or(0) + repeat.plus;
                (0..count).map(|i| format!("{}{i}", input.name)).collect()
            }
        };
        for name in names {
            inputs.insert(name, json!({"expr": [{"num": "1"}]}));
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
