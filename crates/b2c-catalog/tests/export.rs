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
//! The JSON is `{catalogVersion, blocks, toolbox}`: the block definitions
//! sorted by ID and the toolbox, both in their Rust (serde) shapes, pretty
//! printed with a final newline.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod common;

use b2c_catalog::{BlockDef, Toolbox, core_catalog, toolbox};
use serde::Serialize;

/// The variable that makes the test write the file instead of comparing.
const UPDATE: &str = "B2C_UPDATE_CATALOG_JSON";

/// The exported file, relative to the repository root.
const PATH: &str = "packages/catalog-gen/catalog.json";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Export<'a> {
    catalog_version: &'a str,
    blocks: Vec<&'a BlockDef>,
    toolbox: &'a Toolbox,
}

/// The JSON text of the export.
fn export() -> String {
    let catalog = core_catalog();
    let export = Export {
        catalog_version: &catalog.version,
        // A BTreeMap iterates in ID order.
        blocks: catalog.blocks.values().collect(),
        toolbox: toolbox(),
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
    assert_eq!(core_catalog().blocks.len(), 32);

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
    assert_eq!(keys, ["blocks", "catalogVersion", "toolbox"]);
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
    assert_eq!(ids.len(), 32);
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
}
