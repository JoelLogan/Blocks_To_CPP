//! The security hash and summary used by workspace trust (spec §8.3): pinned
//! vectors, what changes the hash and what does not, and what the trust
//! dialog lists.

// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod common;

use b2c_ir::BlockId;
use b2c_model::{
    Block, Define, DefineValue, Document, FieldValue, PackRef, SecuritySummary, Viewport, hex, load,
    security_hash, security_summary, to_canonical_json,
};
use common::{example_paths, repo_root};

/// The hash of a document with no Raw C++, libraries, packs or defines.
///
/// Cross-checked when it was pinned: SHA-256 of `b2c-trust-v1\n` followed by
/// `{"rawCpp":[],"libraries":[],"packs":[],"defines":[]}`, computed
/// independently with Python's `json` and `hashlib`.
const EMPTY_CONTENT_HASH: &str = "0e50ad8a72804af51ec2785aa2b9866f4ffdf7a9c8e6cadd702f71b8c00556b2";

/// The hash of `tests/fixtures/trust_defines.b2c`, cross-checked the same
/// way (a Python reimplementation of the documented input).
const FIXTURE_HASH: &str = "5b2b37b61045330ffc624165816c42451342a08d639c97c6a5d10006c28775a8";

fn load_path(relative: &str) -> Document {
    let path = repo_root().join(relative);
    match load(&std::fs::read(&path).unwrap()) {
        Ok(document) => document,
        Err(error) => panic!("{}: {:#?}", path.display(), error.diagnostics),
    }
}

fn fixture() -> Document {
    load_path("crates/b2c-model/tests/fixtures/trust_defines.b2c")
}

fn hash(document: &Document) -> String {
    hex(&security_hash(document))
}

/// Applies `edit` to every block with the given ID, anywhere.
fn edit_block(document: &mut Document, id: &str, edit: impl Fn(&mut Block) + Copy) {
    fn visit(block: &mut Block, id: &str, edit: impl Fn(&mut Block) + Copy) {
        if block.id.as_str() == id {
            edit(block);
        }
        for list in block.statements.values_mut() {
            for child in list {
                visit(child, id, edit);
            }
        }
        for child in &mut block.stack {
            visit(child, id, edit);
        }
    }
    for module in &mut document.modules {
        for block in &mut module.workspace.blocks {
            visit(block, id, edit);
        }
    }
}

#[test]
fn hash_vectors_are_pinned() {
    assert_eq!(hash(&load_path("examples/guessing_game.b2c")), EMPTY_CONTENT_HASH);
    assert_eq!(hash(&fixture()), FIXTURE_HASH);
}

#[test]
fn examples_have_nothing_security_relevant() {
    for path in example_paths() {
        let document = load(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(hash(&document), EMPTY_CONTENT_HASH, "{}", path.display());
        assert_eq!(
            security_summary(&document),
            SecuritySummary::default(),
            "{}",
            path.display()
        );
    }
}

#[test]
fn the_hash_follows_trust_relevant_content() {
    let original = fixture();
    let base = hash(&original);
    let mut changes: Vec<(&str, Document)> = Vec::new();

    let mut d = original.clone();
    d.project.build.defines[0].value = DefineValue::Int(4);
    changes.push(("define value", d));
    let mut d = original.clone();
    d.project.build.defines[1].name = "RELEASE_MODE".into();
    changes.push(("define name", d));
    let mut d = original.clone();
    d.project.build.defines.swap(0, 1);
    changes.push(("define order", d));
    let mut d = original.clone();
    d.project.build.defines.push(Define {
        name: "EXTRA".into(),
        value: DefineValue::String(String::new()),
    });
    changes.push(("define added", d));
    let mut d = original.clone();
    d.project.build.defines[2].value = DefineValue::Bool(true);
    changes.push(("define type", d));
    let mut d = original.clone();
    d.project.build.libraries.push("pthread".into());
    changes.push(("library added", d));
    let mut d = original.clone();
    d.project.build.libraries.pop();
    changes.push(("duplicate library removed", d));
    let mut d = original.clone();
    d.project.build.packs[0].version = "^3.0".into();
    changes.push(("pack version", d));
    let mut d = original.clone();
    d.project.build.packs.push(PackRef {
        id: "net".into(),
        version: "*".into(),
    });
    changes.push(("pack added", d));
    let mut d = original.clone();
    edit_block(&mut d, "b_raw2", |b| {
        b.fields
            .insert("CODE".into(), FieldValue::Text("std::system(\"x\");".into()));
    });
    changes.push(("raw text in a statement list", d));
    let mut d = original.clone();
    edit_block(&mut d, "b_raw3", |b| {
        b.fields.insert("CODE".into(), FieldValue::Text(String::new()));
    });
    changes.push(("raw text in a stack", d));
    let mut d = original.clone();
    edit_block(&mut d, "b_raw1", |b| {
        b.fields.insert("NEW".into(), FieldValue::Text("x".into()));
    });
    changes.push(("raw text field added to a disabled block", d));
    let mut d = original.clone();
    edit_block(&mut d, "b_print", |b| b.block_type = "raw.statement".into());
    changes.push(("an ordinary block becomes Raw C++", d));
    let mut d = original.clone();
    d.modules[1]
        .workspace
        .blocks
        .retain(|b| b.id.as_str() != "a_raw0");
    changes.push(("raw block removed", d));

    for (what, changed) in &changes {
        assert_ne!(hash(changed), base, "{what} must change the hash");
    }
}

#[test]
fn the_hash_ignores_everything_else() {
    let original = fixture();
    let base = hash(&original);
    let mut same: Vec<(&str, Document)> = Vec::new();

    let mut d = original.clone();
    d.project.build.libraries.reverse();
    changes_order(&mut d);
    same.push(("library, pack and top-level block order", d));
    let mut d = original.clone();
    for module in &mut d.modules {
        module.workspace.viewport = Some(Viewport {
            x: 5,
            y: 5,
            scale: 2.0,
        });
        for block in &mut module.workspace.blocks {
            block.x = Some(block.x.unwrap_or(0) + 50);
            block.collapsed = true;
        }
    }
    same.push(("layout", d));
    let mut d = original.clone();
    edit_block(&mut d, "b_raw1", |b| {
        b.comment = None;
        b.disabled = false;
    });
    same.push(("comment and disabled state of a raw block", d));
    let mut d = original.clone();
    edit_block(&mut d, "b_raw2", |b| {
        b.fields.insert("PINNED".into(), FieldValue::Bool(false));
    });
    same.push(("a raw block's non-text field", d));
    let mut d = original.clone();
    edit_block(&mut d, "b_print", |b| {
        b.fields.insert("SEP".into(), FieldValue::Text("space".into()));
    });
    edit_block(&mut d, "b_file", |b| {
        b.fields
            .insert("PATH".into(), FieldValue::Text("other.txt".into()));
    });
    same.push(("ordinary block edits", d));
    let mut d = original.clone();
    d.project.name = "Renamed".into();
    d.project.description = "New description".into();
    d.project.build.configurations.debug.sanitizers.clear();
    same.push(("project name and build configuration", d));

    for (what, unchanged) in &same {
        assert_eq!(hash(unchanged), base, "{what} must not change the hash");
    }
    // Saving and loading again changes nothing either.
    let reloaded = load(to_canonical_json(&original).as_bytes()).unwrap();
    assert_eq!(hash(&reloaded), base);
}

fn changes_order(document: &mut Document) {
    document.project.build.packs.reverse();
    for module in &mut document.modules {
        module.workspace.blocks.reverse();
    }
}

#[test]
fn defines_are_hashed_in_their_serde_shape() {
    // The compact JSON the hash covers uses the same spelling as serde.
    let define = Define {
        name: "GREETING".into(),
        value: DefineValue::String("Héllo \"world\" ✓\tok\\".into()),
    };
    assert_eq!(
        serde_json::to_string(&define).unwrap(),
        r#"{"name":"GREETING","value":{"string":"Héllo \"world\" ✓\tok\\"}}"#
    );
}

#[test]
fn the_summary_lists_what_the_trust_dialog_shows() {
    let summary = security_summary(&fixture());
    let ids = |list: &[BlockId]| list.iter().map(|id| id.as_str().to_owned()).collect::<Vec<_>>();
    // Canonical order: modules in file order, top-level blocks by ID, then
    // children (statement lists, stacks) parents first.
    assert_eq!(
        ids(&summary.raw_cpp_blocks),
        ["b_raw3", "b_raw2", "b_raw1", "a_raw0"]
    );
    assert_eq!(ids(&summary.file_system_blocks), ["b_file", "a_file"]);
    assert_eq!(summary.libraries, ["m", "sfml-graphics"]);
    assert_eq!(
        serde_json::to_value(&summary).unwrap(),
        serde_json::json!({
            "rawCppBlocks": ["b_raw3", "b_raw2", "b_raw1", "a_raw0"],
            "libraries": ["m", "sfml-graphics"],
            "fileSystemBlocks": ["b_file", "a_file"]
        })
    );
}
