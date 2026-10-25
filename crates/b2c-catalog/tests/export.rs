//! Exports the validated catalog and toolbox as
//! `packages/catalog-gen/catalog.json`, the only input of the build-time
//! generator `packages/catalog-gen` (spec §3.11.1). The generator never reads
//! the TOML files, so all validation stays here, in Rust.
//!
//! The file is committed. This test fails when it is stale; to regenerate it
//! and everything generated from it, run from the repository root:
//!
//! ```sh
//! B2C_UPDATE_CATALOG_JSON=1 cargo test -p b2c-catalog --test export
//! pnpm --filter @blocks2cpp/catalog-gen run generate
//! ```
//!
//! The JSON is `{catalogVersion, blocks, toolbox, types}`: the block
//! definitions sorted by ID and the toolbox, both in their Rust (serde)
//! shapes, and the types of milestone M3 (`docs/plans/m3-contracts/types.md`
//! §1.1), pretty printed with a final newline. Each block also carries what
//! its signature adds (§4.5): `order`, `placeVar`, `members` and
//! `optionHeaders`, each left out while empty.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod common;

use std::collections::BTreeMap;

use b2c_catalog::{BlockDef, Toolbox, core_catalog, toolbox};
use b2c_ir::types::NoFacts;
use b2c_ir::{BlockSig, Header, LibType, PartName, Type, TypeVar};
use serde::Serialize;
use serde_json::{Value, json};

/// The variable that makes the test write the file instead of comparing.
const UPDATE: &str = "B2C_UPDATE_CATALOG_JSON";

/// The exported file, relative to the repository root.
const PATH: &str = "packages/catalog-gen/catalog.json";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Export<'a> {
    catalog_version: &'a str,
    blocks: Vec<ExportBlock<'a>>,
    toolbox: &'a Toolbox,
    types: Vec<TypeEntry>,
}

/// A block definition in its serde shape, with what its signature adds.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ExportBlock<'a> {
    #[serde(flatten)]
    def: &'a BlockDef,
    /// The parts that have a pattern, in the order they bind in:
    /// `{"field": "TYPE"}` or `{"input": "VALUE"}`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    order: Vec<Value>,
    /// The output's variable when the block takes its type from its place.
    #[serde(skip_serializing_if = "Option::is_none")]
    place_var: Option<&'a str>,
    /// The `members` fields and the variable each lists the members of.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    members: BTreeMap<&'a str, &'a str>,
    /// The headers of dropdown options, by field, then option.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    option_headers: BTreeMap<&'a str, BTreeMap<&'a str, Vec<&'static str>>>,
}

impl<'a> ExportBlock<'a> {
    fn new(def: &'a BlockDef, sig: &'a BlockSig) -> Self {
        ExportBlock {
            def,
            order: sig
                .order
                .iter()
                .map(|part| match part {
                    PartName::Field(name) => json!({ "field": name }),
                    PartName::Input(name) => json!({ "input": name }),
                })
                .collect(),
            place_var: sig.place_var.as_ref().map(TypeVar::as_str),
            members: sig
                .members
                .iter()
                .map(|(field, var)| (field.as_str(), var.as_str()))
                .collect(),
            option_headers: sig
                .option_headers
                .iter()
                .map(|(field, options)| {
                    let options = options
                        .iter()
                        .map(|(option, headers)| {
                            (option.as_str(), headers.iter().map(|h| h.as_str()).collect())
                        })
                        .collect();
                    (field.as_str(), options)
                })
                .collect(),
        }
    }
}

/// One entry of the `types` section (types.md §1.1).
#[derive(Serialize)]
struct TypeEntry {
    /// Its JSON name: `int`, `std::string`, `std::vector`.
    name: &'static str,
    /// How many template arguments it takes.
    arity: usize,
    /// Its friendly label, with `%1` and `%2` for the arguments' labels.
    label: String,
    /// The standard header that declares it.
    header: Option<&'static str>,
}

/// The fundamental types, `std::string` and the library types, in that order.
fn types() -> Vec<TypeEntry> {
    let names = [
        (Type::Void, "void"),
        (Type::Bool, "bool"),
        (Type::Char, "char"),
        (Type::Int, "int"),
        (Type::Double, "double"),
        (Type::String, "std::string"),
    ];
    let primitives = names.into_iter().map(|(ty, name)| TypeEntry {
        name,
        arity: 0,
        label: ty.label(&NoFacts).to_string(),
        header: ty.headers().next().map(Header::as_str),
    });
    let library = LibType::ALL.into_iter().map(|ty| TypeEntry {
        name: ty.cpp_name(),
        arity: ty.arity(),
        label: ty.label().to_owned(),
        header: Some(ty.header().as_str()),
    });
    primitives.chain(library).collect()
}

/// The JSON text of the export.
fn export() -> String {
    let catalog = core_catalog();
    let sigs = catalog.sigs();
    let export = Export {
        catalog_version: &catalog.version,
        // A BTreeMap iterates in ID order. Every block of the core catalog
        // has a signature.
        blocks: catalog
            .blocks
            .iter()
            .map(|(id, def)| ExportBlock::new(def, sigs.get(id).unwrap()))
            .collect(),
        toolbox: toolbox(),
        types: types(),
    };
    let mut text = serde_json::to_string_pretty(&export).unwrap();
    text.push('\n');
    text
}

#[test]
fn the_exported_catalog_is_current() {
    // Only a complete, valid catalog and toolbox are exported: the cached
    // values leave broken parts out, so check them in full first.
    let toolbox_text = std::fs::read_to_string(common::repo_root().join("catalog/toolbox.toml")).unwrap();
    assert_eq!(&Toolbox::load(&toolbox_text, core_catalog()).unwrap(), toolbox());
    assert_eq!(core_catalog().blocks.len(), common::block_ids_on_disk().len());

    let text = export();
    let path = common::repo_root().join(PATH);
    if std::env::var_os(UPDATE).is_some_and(|value| value == "1") {
        std::fs::write(&path, &text).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_default();
    if committed != text {
        let line = committed
            .lines()
            .zip(text.lines())
            .position(|(old, new)| old != new)
            .unwrap_or_else(|| committed.lines().count().min(text.lines().count()));
        panic!(
            "{PATH} is stale (it differs from line {}). Regenerate it with `{UPDATE}=1 cargo test -p b2c-catalog --test export`, then run `pnpm --filter @blocks2cpp/catalog-gen run generate` and commit the results.",
            line + 1
        );
    }
}

#[test]
fn the_export_has_the_documented_shape() {
    let value: serde_json::Value = serde_json::from_str(&export()).unwrap();
    let object = value.as_object().unwrap();
    let keys: Vec<&str> = object.keys().map(String::as_str).collect();
    assert_eq!(keys, ["blocks", "catalogVersion", "toolbox", "types"]);
    assert_eq!(object["catalogVersion"], b2c_catalog::CATALOG_VERSION);
    let ids: Vec<&str> = object["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["id"].as_str().unwrap())
        .collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted);
    assert_eq!(ids.len(), core_catalog().blocks.len());
    // Spot checks of the serde shapes the generator reads.
    let random = object["blocks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["id"] == "math.random_int")
        .unwrap();
    assert_eq!(random["output"], "int");
    assert_eq!(random["input"][0]["default"][0]["num"], "1");
    let categories = object["toolbox"]["category"].as_array().unwrap();
    assert_eq!(categories[0]["id"], "program");
    assert_eq!(categories[1]["dynamic"], "variables");
    assert_eq!(
        categories[1]["entry"][0]["preset"]["inputs"]["VALUE"][0]["num"],
        "0"
    );
    assert_eq!(categories[0]["entry"][0]["preset"], serde_json::Value::Null);
    // The signatures (types.md §4.5): patterns as tagged JSON, the binding
    // order, the class of parameter rows, and types as terms.
    let block = |id: &str| {
        object["blocks"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["id"] == id)
            .unwrap()
            .clone()
    };
    let declare = block("var.declare");
    assert_eq!(declare["typeVars"], json!({"T": "value"}));
    assert_eq!(declare["field"][0]["type"], json!({"kind": "var", "name": "T"}));
    assert_eq!(declare["field"][0]["types"][4], "std::string");
    assert_eq!(
        declare["order"],
        json!([{"field": "TYPE"}, {"field": "NAME"}, {"input": "VALUE"}])
    );
    assert!(declare.get("placeVar").is_none() && declare.get("members").is_none());
    let set = block("var.set");
    assert_eq!(set["input"][0]["assign"], true);
    assert!(declare["input"][0].get("assign").is_none());
    assert_eq!(
        block("func.call")["input"][0]["type"],
        json!({"kind": "param", "field": "FUNC"})
    );
    assert_eq!(
        block("func.return")["input"][0]["type"],
        json!({"kind": "result"})
    );
    assert_eq!(block("func.define")["extra"][0]["typeClass"], "value");
    assert_eq!(block("text.literal")["output"], "std::string");
    assert_eq!(block("text.literal")["headers"], json!([]));
    let types = object["types"].as_array().unwrap();
    assert_eq!(types.len(), 6 + LibType::ALL.len());
    assert_eq!(
        types[5],
        json!({"name": "std::string", "arity": 0, "label": "string", "header": "<string>"})
    );
    assert_eq!(
        types[9],
        json!({"name": "std::map", "arity": 2, "label": "map from %1 to %2", "header": "<map>"})
    );
}
