//! Format versions (types.md §2.3, §2.4; ADR-0013, ADR-0017): which
//! versions load, which version a document or a clipboard payload is saved
//! as, and the version 1 projects that must load and save byte for byte.

// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod common;

use std::collections::BTreeSet;
use std::path::PathBuf;

use b2c_ir::types::TypeTerm;
use b2c_ir::{BlockId, SymbolId};
use b2c_model::{
    Clipboard, ClipboardRef, Document, FieldValue, MAX_CLIPBOARD_FORMAT_VERSION, MAX_FORMAT_VERSION,
    OLDEST_CLIPBOARD_FORMAT_VERSION, OLDEST_UNMIGRATED_VERSION, RefKind, SeededIds, load, load_clipboard,
    outside_refs, remap_ids, required_clipboard_format_version, required_format_version, rewrite_refs,
    to_canonical_clipboard_json, to_canonical_json,
};
use common::{base, bytes, codes, example_paths, failure, main_block, repo_root};
use serde_json::{Value, json};

fn loads(value: &Value) -> Document {
    match load(&bytes(value)) {
        Ok(document) => document,
        Err(error) => panic!("{:#?}", error.diagnostics),
    }
}

fn sym(id: &str) -> SymbolId {
    SymbolId::new(id).unwrap()
}

/// The `formatVersion` a saved text names.
fn saved_version(text: &str) -> u64 {
    serde_json::from_str::<Value>(text).unwrap()["formatVersion"]
        .as_u64()
        .unwrap()
}

/// The block projects of the M3 assignments (`tests/assignments/<name>/<name>.b2c`),
/// while they are being built.
fn assignment_projects() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(repo_root().join("tests/assignments")).unwrap() {
        let folder = entry.unwrap().path();
        if let Some(name) = folder.file_name().and_then(|n| n.to_str()) {
            let project = folder.join(format!("{name}.b2c"));
            if project.is_file() {
                paths.push(project);
            }
        }
    }
    paths
}

/// Every M2 project the repository keeps: the examples (the golden
/// projects), the assignments' block projects, the app's templates and the
/// security projects the loader accepts.
fn m2_projects() -> Vec<(PathBuf, Vec<u8>)> {
    let mut paths = example_paths();
    paths.extend(assignment_projects());
    for template in ["empty", "hello_world"] {
        paths.push(repo_root().join(format!("crates/b2c-app/templates/{template}.b2c")));
    }
    let security = repo_root().join("tests/security/projects");
    for entry in std::fs::read_dir(&security).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "b2c") {
            paths.push(path);
        }
    }
    paths
        .into_iter()
        .map(|path| {
            let raw = std::fs::read(&path).unwrap();
            (path, raw)
        })
        .collect()
}

#[test]
fn the_constants() {
    assert_eq!(MAX_FORMAT_VERSION, 2);
    assert_eq!(OLDEST_UNMIGRATED_VERSION, 1);
    assert_eq!(MAX_CLIPBOARD_FORMAT_VERSION, 2);
    assert_eq!(OLDEST_CLIPBOARD_FORMAT_VERSION, 1);
}

#[test]
fn version_1_golden_projects_load_and_save_byte_for_byte() {
    for path in example_paths() {
        let raw = std::fs::read(&path).unwrap();
        let name = path.display();
        let document = load(&raw).unwrap_or_else(|e| panic!("{name}: {:#?}", e.diagnostics));
        assert_eq!(document.format_version, 1, "{name}");
        assert_eq!(required_format_version(&document), 1, "{name}");
        assert!(
            to_canonical_json(&document).as_bytes() == raw.as_slice(),
            "{name}"
        );
    }
}

#[test]
fn every_m2_project_needs_version_1() {
    let mut loaded = 0;
    for (path, raw) in m2_projects() {
        // Rejected security projects are checked by `security.rs`.
        if let Ok(document) = load(&raw) {
            assert_eq!(required_format_version(&document), 1, "{}", path.display());
            assert_eq!(
                saved_version(&to_canonical_json(&document)),
                1,
                "{}",
                path.display()
            );
            loaded += 1;
        }
    }
    assert!(loaded >= 40, "only {loaded} projects loaded");
}

#[test]
fn a_template_needs_version_2_wherever_it_is() {
    let template = json!({"std::vector": {"ref": "s_point"}});
    let check = |document: &Value, expected: u32| {
        let loaded = loads(document);
        assert_eq!(required_format_version(&loaded), expected, "{document}");
        assert_eq!(saved_version(&to_canonical_json(&loaded)), u64::from(expected));
    };
    check(&base(), 1);
    // A user type and a name are version 1 values, in a field or a row.
    let mut document = base();
    main_block(&mut document)["statements"]["BODY"][1]["fields"]["TYPE"] = json!({"ref": "s_point"});
    document["modules"][0]["workspace"]["blocks"][1]["extra"]["params"][0]["type"] =
        json!({"ref": "s_point"});
    check(&document, 1);
    // In a type field, in a file that said version 1.
    let mut document = base();
    main_block(&mut document)["statements"]["BODY"][1]["fields"]["TYPE"] = template.clone();
    check(&document, 2);
    // In the type of a parameter row.
    let mut document = base();
    document["modules"][0]["workspace"]["blocks"][1]["extra"]["params"][0]["type"] = template.clone();
    check(&document, 2);
    // In a nested block of a disabled block in a loose stack of another
    // module.
    let mut document = base();
    let nested = json!({"id": "b_inner", "type": "var.declare", "v": 1,
        "fields": {"NAME": {"sym": "s_inner", "name": "inner"}, "TYPE": template}});
    let stacked = json!({"id": "b_loop", "type": "control.forever", "v": 1, "disabled": true,
        "statements": {"BODY": [nested]}});
    let head = json!({"id": "b_head", "type": "control.forever", "v": 1, "x": 0, "y": 0, "stack": [stacked]});
    document["modules"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "mod_other", "name": "other", "workspace": {"blocks": [head]}}));
    check(&document, 2);
}

#[test]
fn a_file_that_stops_using_templates_goes_back_to_version_1() {
    let mut document = base();
    document["formatVersion"] = json!(2);
    main_block(&mut document)["statements"]["BODY"][1]["fields"]["TYPE"] = json!({"std::set": "int"});
    let mut loaded = loads(&document);
    assert_eq!(loaded.format_version, 2);
    let saved = to_canonical_json(&loaded);
    assert_eq!(saved_version(&saved), 2);
    assert_eq!(to_canonical_json(&load(saved.as_bytes()).unwrap()), saved);
    let declare = &mut loaded.modules[0].workspace.blocks[0]
        .statements
        .get_mut("BODY")
        .unwrap()[1];
    declare
        .fields
        .insert("TYPE".into(), FieldValue::Text("int".into()));
    assert_eq!(saved_version(&to_canonical_json(&loaded)), 1);
    // A version 2 file that uses nothing of version 2 is saved as version 1.
    let mut document = base();
    document["formatVersion"] = json!(2);
    let loaded = loads(&document);
    assert_eq!(loaded.format_version, 2);
    let saved = to_canonical_json(&loaded);
    assert_eq!(saved, to_canonical_json(&loads(&base())));
}

#[test]
fn versions_1_and_2_load_without_migration_and_newer_ones_are_refused() {
    for version in [1, 2] {
        let mut document = base();
        document["formatVersion"] = json!(version);
        assert_eq!(loads(&document).format_version, version);
    }
    for version in [json!(3), json!(4_294_967_296_u64)] {
        let mut document = base();
        document["formatVersion"] = version;
        let diagnostics = failure(&bytes(&document));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.0, "B2C-E0108");
        assert_eq!(
            diagnostics[0].message,
            "This project was made with a newer version of Blocks2Cpp, which saves projects in a newer format than \
             this version can read. Update Blocks2Cpp to open it."
        );
    }
    // Older than the oldest version: the migration chain, which has no step.
    let mut document = base();
    document["formatVersion"] = json!(0);
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics[0].code.0, "B2C-E0109");
    assert_eq!(
        diagnostics[0].message,
        "This project uses project format 0, which this version of Blocks2Cpp cannot read or upgrade."
    );
}

#[test]
fn textbook_style_is_not_a_key_yet() {
    // `options.usingNamespaceStd` is a version 2 key (ADR-0013), but no
    // document holds it until Textbook style arrives: it stays unknown.
    let mut document = base();
    document["formatVersion"] = json!(2);
    document["project"]["options"] = json!({"usingNamespaceStd": true});
    assert_eq!(codes(&document), ["B2C-E0110"]);
}

// ---------------------------------------------------------------------------
// The clipboard
// ---------------------------------------------------------------------------

fn payload(version: u64, blocks: &Value, refs: &Value) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "format": "blocks2cpp/clipboard",
        "formatVersion": version,
        "catalog": "1.0.0",
        "blocks": blocks,
        "refs": refs,
    }))
    .unwrap()
}

/// A `var.declare` of `score` whose type is `ty`.
fn declare(ty: &Value) -> Value {
    json!([{"id": "b1", "type": "var.declare", "v": 1,
        "fields": {"NAME": {"sym": "s_score", "name": "score"}, "TYPE": ty}}])
}

fn clipboard_codes(bytes: &[u8]) -> Vec<String> {
    load_clipboard(bytes)
        .unwrap_err()
        .diagnostics
        .into_iter()
        .map(|d| d.code.0)
        .collect()
}

#[test]
fn clipboard_versions_1_and_2_are_read() {
    let blocks = declare(&json!("int"));
    for version in [1, 2] {
        let clipboard = load_clipboard(&payload(version, &blocks, &json!({}))).unwrap();
        assert_eq!(required_clipboard_format_version(&clipboard), 1);
        assert_eq!(saved_version(&to_canonical_clipboard_json(&clipboard)), 1);
    }
    assert_eq!(clipboard_codes(&payload(3, &blocks, &json!({}))), ["B2C-E0108"]);
    assert_eq!(clipboard_codes(&payload(0, &blocks, &json!({}))), ["B2C-E0109"]);
}

#[test]
fn templates_and_the_kinds_of_user_types_need_clipboard_version_2() {
    let check = |blocks: &Value, refs: &Value, expected: u32| {
        let clipboard = load_clipboard(&payload(1, blocks, refs)).unwrap();
        assert_eq!(
            required_clipboard_format_version(&clipboard),
            expected,
            "{blocks} {refs}"
        );
        let saved = to_canonical_clipboard_json(&clipboard);
        assert_eq!(saved_version(&saved), u64::from(expected));
        assert_eq!(load_clipboard(saved.as_bytes()).unwrap(), clipboard);
    };
    let plain = declare(&json!("int"));
    check(&plain, &json!({}), 1);
    check(&declare(&json!({"ref": "s_point"})), &json!({}), 1);
    for kind in ["variable", "parameter", "loopVariable", "function"] {
        check(&plain, &json!({"s": {"name": "x", "kind": kind}}), 1);
    }
    check(&declare(&json!({"std::vector": "int"})), &json!({}), 2);
    let row = json!([{"id": "b1", "type": "func.define", "v": 1,
        "extra": {"params": [{"sym": "s_n", "name": "n", "type": {"std::set": "int"}, "mode": "copy"}]}}]);
    check(&row, &json!({}), 2);
    let stacked = json!([{"id": "b0", "type": "control.forever", "v": 1, "stack": declare(&json!({"std::vector": "int"}))}]);
    check(&stacked, &json!({}), 2);
    for (name, kind) in [
        ("::Student", "struct"),
        ("::Action", "enum"),
        ("::Student::mark", "field"),
        ("::Action::Deposit", "enumerator"),
    ] {
        check(&plain, &json!({"s": {"name": name, "kind": kind}}), 2);
    }
}

#[test]
fn the_kinds_of_user_types_round_trip() {
    let refs = json!({
        "s_student": {"name": "::Student", "kind": "struct"},
        "s_mark": {"name": "::Student::mark", "kind": "field"},
        "s_action": {"name": "::Action", "kind": "enum"},
        "s_deposit": {"name": "::Action::Deposit", "kind": "enumerator"},
    });
    let clipboard = load_clipboard(&payload(2, &declare(&json!("int")), &refs)).unwrap();
    let kinds: Vec<(&str, &str, RefKind)> = clipboard
        .refs
        .iter()
        .map(|(sym, r)| (sym.as_str(), r.name.as_str(), r.kind))
        .collect();
    assert_eq!(
        kinds,
        [
            ("s_action", "::Action", RefKind::Enum),
            ("s_deposit", "::Action::Deposit", RefKind::Enumerator),
            ("s_mark", "::Student::mark", RefKind::Field),
            ("s_student", "::Student", RefKind::Struct),
        ]
    );
    let saved = to_canonical_clipboard_json(&clipboard);
    assert!(saved.contains("\"kind\": \"enumerator\""), "{saved}");
    assert_eq!(load_clipboard(saved.as_bytes()).unwrap(), clipboard);
}

// ---------------------------------------------------------------------------
// References inside type terms
// ---------------------------------------------------------------------------

/// Blocks that use the struct `s_point` (declared elsewhere) and
/// `s_local` (declared in the blocks) in type terms: a function whose
/// parameter is a list of points, and a variable of a map type.
fn blocks_with_terms() -> Vec<b2c_model::Block> {
    let blocks = json!([
        {"id": "b_struct", "type": "test.struct", "v": 1, "fields": {"NAME": {"sym": "s_local", "name": "Local"}}},
        {"id": "b_fn", "type": "func.define", "v": 1,
         "extra": {"params": [{"sym": "s_n", "name": "n", "type": {"std::vector": {"ref": "s_point"}}, "mode": "read_only"}]},
         "fields": {"NAME": {"sym": "s_f", "name": "f"}, "RETURNS": "void"},
         "statements": {"BODY": [
            {"id": "b_var", "type": "var.declare", "v": 1,
             "fields": {"NAME": {"sym": "s_v", "name": "v"},
                        "TYPE": {"std::map": [{"ref": "s_local"}, {"std::vector": {"ref": "s_point"}}]}}}
         ]}}
    ]);
    load_clipboard(&payload(2, &blocks, &json!({}))).unwrap().blocks
}

/// Every reference in type terms of `blocks`, in walk order.
fn term_refs(blocks: &[b2c_model::Block]) -> Vec<String> {
    let mut found = Vec::new();
    let fn_block = &blocks[1];
    let row_type = &fn_block.extra["params"][0]["type"];
    found.push(row_type["std::vector"]["ref"].as_str().unwrap().to_owned());
    let FieldValue::Type(term) = &fn_block.statements["BODY"][0].fields["TYPE"] else {
        panic!("no template");
    };
    found.extend(term.refs().map(|sym| sym.as_str().to_owned()));
    found
}

#[test]
fn outside_refs_see_references_in_type_terms() {
    let blocks = blocks_with_terms();
    assert_eq!(outside_refs(&blocks), BTreeSet::from([sym("s_point")]));
    assert_eq!(term_refs(&blocks), ["s_point", "s_local", "s_point"]);
}

#[test]
fn remapping_renames_declared_symbols_inside_type_terms() {
    let mut blocks = blocks_with_terms();
    let map = remap_ids(&mut blocks, &BTreeSet::new(), &mut SeededIds::new([7; 32])).unwrap();
    let local = map[&sym("s_local")].as_str().to_owned();
    // The struct declared in the blocks follows its new ID everywhere; the
    // one declared elsewhere keeps its own.
    assert_eq!(term_refs(&blocks), ["s_point", local.as_str(), "s_point"]);
    assert_eq!(outside_refs(&blocks), BTreeSet::from([sym("s_point")]));
    let clipboard = Clipboard {
        catalog: "1.0.0".into(),
        blocks,
        refs: [(
            sym("s_point"),
            ClipboardRef {
                name: "::Point".into(),
                kind: RefKind::Struct,
            },
        )]
        .into(),
    };
    // The remapped payload is still valid.
    load_clipboard(to_canonical_clipboard_json(&clipboard).as_bytes()).unwrap();
}

#[test]
fn rebinding_rewrites_references_inside_type_terms() {
    let mut blocks = blocks_with_terms();
    rewrite_refs(&mut blocks, &[(sym("s_point"), sym("t_point"))].into());
    assert_eq!(term_refs(&blocks), ["t_point", "s_local", "t_point"]);
    // Declarations are not references: `s_local` is still declared.
    assert!(matches!(&blocks[0].fields["NAME"], FieldValue::Decl(d) if d.sym == sym("s_local")));
    assert_eq!(blocks[0].id, BlockId::new("b_struct").unwrap());
    let FieldValue::Type(TypeTerm::App(name, _)) = &blocks[1].statements["BODY"][0].fields["TYPE"] else {
        panic!("no template");
    };
    assert_eq!(&**name, "std::map");
}
