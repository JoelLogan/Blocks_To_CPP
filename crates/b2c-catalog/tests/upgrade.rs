//! Block upgrades through the public API (spec 05 §5.7, 03 §3.11.3, 08
//! §8.3.1, ADR-0015): a file from a newer catalog, and the trust hash.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod common;

use b2c_catalog::{BlockMigration, BlockUpgrades, Catalog};
use b2c_model::{Block, FieldValue};
use serde_json::{Value, json};

/// A project of `catalog` whose `main` holds `body`.
fn project_of(catalog: &str, body: Value) -> Value {
    let mut project = common::project(body, json!([]));
    project["generator"]["catalog"] = json!(catalog);
    project
}

/// A `change` block with an option a later catalog may add (`OP`, as the
/// merged *change* block of catalog 1.2.0 has).
fn change_with_op() -> Value {
    json!([
        {"id": "b_decl", "type": "var.declare", "v": 1,
         "fields": {"NAME": {"sym": "s_n", "name": "n"}, "TYPE": "int"}},
        {"id": "b_change", "type": "var.change", "v": 1,
         "fields": {"VAR": {"ref": "s_n"}, "OP": "mul"}}
    ])
}

#[test]
fn a_file_from_a_newer_catalog_is_reported_as_made_by_it() {
    let newer = project_of("1.2.0", change_with_op());
    let (completed, diagnostics) = common::resolve(&newer);
    let found: Vec<(&str, Option<&str>)> = diagnostics
        .iter()
        .map(|d| {
            (
                d.code.0.as_str(),
                d.primary.block.as_ref().map(b2c_ir::BlockId::as_str),
            )
        })
        .collect();
    assert_eq!(found, [("B2C-E0602", Some("b_change"))]);
    assert_eq!(
        diagnostics[0].message,
        "This block was made with a newer version of Blocks2Cpp: it has the field \"OP\", which this version does not know. Update Blocks2Cpp to use it; until then the block is kept as it is."
    );
    // The block is kept as it is: its unknown field stays, and no default is
    // filled in.
    let change = &completed.modules[0].workspace.blocks[0].statements["BODY"][1];
    assert_eq!(change.fields["OP"], FieldValue::Text("mul".into()));
    assert!(!change.inputs.contains_key("BY"));

    // A file of this catalog (or an older one) with the same block has an
    // unknown field.
    for catalog in [b2c_catalog::CATALOG_VERSION, "1.0.0"] {
        assert_eq!(
            common::codes(&project_of(catalog, change_with_op())),
            ["B2C-E0605"]
        );
    }
}

#[test]
fn a_newer_catalog_still_reports_what_is_wrong_in_known_names() {
    // A known field with a value that no catalog would write is wrong, not
    // newer.
    let body = json!([{"id": "b1", "type": "io.print", "v": 1, "fields": {"NEWLINE": "yes"}}]);
    assert_eq!(common::codes(&project_of("1.2.0", body)), ["B2C-E0607"]);
}

/// `raw.code` v1 → v2: the checkbox `WRAP` is now `BLOCK` (no text changes).
#[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
fn rename_checkbox(block: &mut Block) -> Result<(), String> {
    if let Some(wrap) = block.fields.remove("WRAP") {
        block.fields.insert(String::from("BLOCK"), wrap);
    }
    Ok(())
}

/// `raw.code` v1 → v2 that also rewrites the code: trust would see new text.
#[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
fn rewrite_code(block: &mut Block) -> Result<(), String> {
    rename_checkbox(block)?;
    block.fields.insert(
        String::from("CODE"),
        FieldValue::Text(String::from("system(\"x\");")),
    );
    Ok(())
}

/// The core catalog with a test-only Raw C++ block type at version 2.
fn catalog_with_raw() -> Catalog {
    let mut catalog = b2c_catalog::core_catalog().clone();
    let mut raw = catalog.blocks["io.print"].clone();
    raw.id = String::from("raw.code");
    raw.version = 2;
    catalog.blocks.insert(raw.id.clone(), raw);
    catalog
}

fn raw_project() -> b2c_model::Document {
    let mut project = project_of(
        "1.0.0",
        json!([{"id": "b_raw", "type": "raw.code", "v": 1,
                "fields": {"CODE": "puts(\"hi\");", "WRAP": true}}]),
    );
    project["project"]["build"] = json!({"libraries": ["m"],
        "defines": [{"name": "N", "value": {"int": 3}}]});
    common::load(&project)
}

#[test]
fn the_trust_hash_is_unchanged_by_an_upgrade() {
    let catalog = catalog_with_raw();
    let old = raw_project();
    let hash = b2c_model::security_hash(&old);
    let summary = b2c_model::security_summary(&old);
    assert_eq!(summary.raw_cpp_blocks.len(), 1);

    let chain = [BlockMigration {
        block: "raw.code",
        from: 1,
        apply: rename_checkbox,
    }];
    let tables = BlockUpgrades {
        migrations: &chain,
        replacements: &[],
        retired: &[],
    };
    let mut upgraded = old.clone();
    let report = b2c_catalog::upgrade_document_with(&mut upgraded, &catalog, &tables);
    assert!(report.changed());
    assert_ne!(upgraded, old);
    assert_eq!(b2c_model::security_hash(&upgraded), hash);
    assert_eq!(b2c_model::security_summary(&upgraded), summary);

    // An upgrade that would change the Raw C++ text is undone, so trust
    // still sees the file's content.
    let chain = [BlockMigration {
        apply: rewrite_code,
        ..chain[0]
    }];
    let tables = BlockUpgrades {
        migrations: &chain,
        ..tables
    };
    let mut refused = old.clone();
    let report = b2c_catalog::upgrade_document_with(&mut refused, &catalog, &tables);
    assert!(!report.changed());
    assert_eq!(refused, old);
    assert_eq!(b2c_model::security_hash(&refused), hash);
    assert_eq!(report.diagnostics.len(), 1);
    assert_eq!(report.diagnostics[0].code.0, "B2C-E0603");
}

#[test]
fn the_shipped_upgrades_change_no_example_or_security_project() {
    let root = common::repo_root();
    let folders = [root.join("examples"), root.join("tests/security/projects")];
    let mut seen = 0;
    for folder in folders {
        for entry in std::fs::read_dir(folder).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_none_or(|e| e != "b2c") {
                continue;
            }
            let Ok(mut document) = b2c_model::load(&std::fs::read(&path).unwrap()) else {
                continue;
            };
            seen += 1;
            let before = document.clone();
            let report = b2c_catalog::upgrade_document(&mut document, b2c_catalog::core_catalog());
            assert!(!report.changed(), "{}", path.display());
            assert_eq!(document, before, "{}", path.display());
        }
    }
    assert!(seen > 10, "too few projects were found");
}
