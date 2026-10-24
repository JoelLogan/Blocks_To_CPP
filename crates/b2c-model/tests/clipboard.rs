//! The clipboard format (spec §5.12): the malicious-clipboard suite in
//! `tests/security/clipboard/`, the rules a payload follows, and copying and
//! pasting the blocks of every example.

// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use b2c_ir::{BlockId, DiagSource, Location, Part, Severity, SymbolId};
use b2c_model::limits::{MAX_BLOCKS, MAX_FILE_BYTES};
use b2c_model::{
    Block, Clipboard, ClipboardRef, Document, FieldValue, Input, RefKind, SeededIds, Token, load,
    load_clipboard, outside_refs, remap_ids, to_canonical_clipboard_json, to_canonical_json, used_ids,
};
use common::{assert_messages_are_safe, codes_in, example_paths, repo_root};
use serde_json::{Value, json};

/// What the loader must do with a payload.
#[derive(Debug, PartialEq, Eq)]
enum Expected {
    Accepted,
    Rejected(BTreeSet<String>),
}

/// One row of the README table.
#[derive(Debug)]
struct Case {
    file: String,
    loader: Expected,
}

fn suite_dir() -> PathBuf {
    repo_root().join("tests/security/clipboard")
}

/// The rows of the README table: file, threat, attack, loader outcome.
fn cases() -> Vec<Case> {
    let readme = std::fs::read_to_string(suite_dir().join("README.md")).unwrap();
    let mut cases = Vec::new();
    for line in readme.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        assert_eq!(cells.len(), 6, "a table row needs four cells: {line}");
        let file = cells[1].trim_matches('`').to_owned();
        let loader = match cells[4] {
            "accepted" => Expected::Accepted,
            cell => {
                let codes = codes_in(cell);
                assert!(
                    !codes.is_empty(),
                    "{file}: the Loader cell names no codes: {cell:?}"
                );
                Expected::Rejected(codes)
            }
        };
        cases.push(Case { file, loader });
    }
    assert!(cases.len() >= 20, "the README table looks truncated");
    cases
}

fn valid_bytes() -> Vec<u8> {
    std::fs::read(suite_dir().join("valid.json")).unwrap()
}

fn valid() -> Clipboard {
    load_clipboard(&valid_bytes()).unwrap()
}

fn codes(value: &Value) -> Vec<String> {
    match load_clipboard(&serde_json::to_vec(value).unwrap()) {
        Ok(_) => panic!("expected the paste to be refused: {value}"),
        Err(error) => error.diagnostics.into_iter().map(|d| d.code.0).collect(),
    }
}

fn payload(blocks: Value) -> Value {
    let mut value = json!({
        "format": "blocks2cpp/clipboard",
        "formatVersion": 1,
        "catalog": "1.0.0",
        "blocks": null,
        "refs": {}
    });
    value["blocks"] = blocks;
    value
}

#[test]
fn the_table_lists_every_file_exactly_once() {
    let listed: Vec<String> = cases().into_iter().map(|case| case.file).collect();
    let unique: BTreeSet<String> = listed.iter().cloned().collect();
    assert_eq!(unique.len(), listed.len(), "a file is listed twice");
    let on_disk: BTreeSet<String> = std::fs::read_dir(suite_dir())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != "README.md")
        .collect();
    assert_eq!(unique, on_disk);
    assert!(on_disk.iter().all(|name| {
        std::path::Path::new(name)
            .extension()
            .is_some_and(|e| e == "json")
    }));
}

#[test]
fn every_crafted_payload_has_its_expected_outcome() {
    let mut failures = Vec::new();
    for case in cases() {
        let bytes = std::fs::read(suite_dir().join(&case.file)).unwrap();
        match (&case.loader, load_clipboard(&bytes)) {
            (Expected::Accepted, Ok(clipboard)) => {
                let saved = to_canonical_clipboard_json(&clipboard);
                let reloaded = load_clipboard(saved.as_bytes())
                    .unwrap_or_else(|e| panic!("{}: the saved copy does not load: {e:?}", case.file));
                assert_eq!(reloaded, clipboard, "{}", case.file);
                assert_eq!(to_canonical_clipboard_json(&reloaded), saved, "{}", case.file);
            }
            (Expected::Accepted, Err(error)) => {
                let found: Vec<String> = error.diagnostics.iter().map(|d| d.code.0.clone()).collect();
                failures.push(format!("{}: expected to load, but got {found:?}", case.file));
            }
            (Expected::Rejected(expected), Ok(_)) => {
                failures.push(format!("{}: expected {expected:?}, but it loaded", case.file));
            }
            (Expected::Rejected(expected), Err(error)) => {
                let diagnostics = error.diagnostics;
                let found: BTreeSet<String> = diagnostics.iter().map(|d| d.code.0.clone()).collect();
                if &found != expected {
                    failures.push(format!("{}: expected {expected:?}, found {found:?}", case.file));
                }
                assert!(
                    diagnostics
                        .iter()
                        .all(|d| d.severity == Severity::Error && d.source == DiagSource::Loader),
                    "{}",
                    case.file
                );
                assert_messages_are_safe(&case.file, &diagnostics);
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn an_oversized_payload_is_refused_before_parsing() {
    // Valid JSON padded with spaces: exactly at the limit it loads, one byte
    // more is refused by size alone.
    let mut bytes = valid_bytes();
    bytes.resize(MAX_FILE_BYTES, b' ');
    assert!(load_clipboard(&bytes).is_ok());
    bytes.push(b' ');
    let error = load_clipboard(&bytes).unwrap_err();
    assert_eq!(error.diagnostics.len(), 1);
    assert_eq!(error.diagnostics[0].code.0, "B2C-E0101");
    assert_eq!(
        error.diagnostics[0].message,
        "The pasted data is larger than 33554432 bytes (32 MiB), the most a paste can be."
    );
}

#[test]
fn the_valid_payload_means_what_it_says() {
    let clipboard = valid();
    assert_eq!(clipboard.catalog, "1.0.0");
    let ids: Vec<&str> = clipboard.blocks.iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids, ["blk_head", "blk_decl"], "the copy order is kept");
    assert_eq!((clipboard.blocks[0].x, clipboard.blocks[0].y), (None, None));
    assert_eq!(clipboard.blocks[0].stack.len(), 1);
    let refs: Vec<(&str, &str, RefKind)> = clipboard
        .refs
        .iter()
        .map(|(sym, r)| (sym.as_str(), r.name.as_str(), r.kind))
        .collect();
    assert_eq!(
        refs,
        [
            ("sym_area", "geo::area", RefKind::Function),
            ("sym_score", "score", RefKind::Variable)
        ]
    );
    // `refs` describes exactly the symbols used but not declared.
    let outside: Vec<String> = outside_refs(&clipboard.blocks)
        .into_iter()
        .map(|s| s.as_str().to_owned())
        .collect();
    assert_eq!(outside, ["sym_area", "sym_score"]);
    // Canonical text: fixed key order, and stable.
    let canonical = to_canonical_clipboard_json(&clipboard);
    assert!(canonical.starts_with(
        "{\n  \"format\": \"blocks2cpp/clipboard\",\n  \"formatVersion\": 1,\n  \"catalog\": \"1.0.0\",\n  \"blocks\": [\n"
    ));
    assert!(canonical.ends_with(
        "  \"refs\": {\n    \"sym_area\": {\n      \"name\": \"geo::area\",\n      \"kind\": \"function\"\n    },\n    \"sym_score\": {\n      \"name\": \"score\",\n      \"kind\": \"variable\"\n    }\n  }\n}\n"
    ));
    let reloaded = load_clipboard(canonical.as_bytes()).unwrap();
    assert_eq!(reloaded, clipboard);
    assert_eq!(to_canonical_clipboard_json(&reloaded), canonical);
    // Blocks are written exactly like in a project file.
    let block_text = serde_json::to_string_pretty(&clipboard.blocks[1]).unwrap();
    let indented: String = block_text.lines().flat_map(|line| ["    ", line, "\n"]).collect();
    assert!(canonical.contains(&indented));
}

#[test]
fn copied_blocks_follow_the_block_rules() {
    let print = json!({"id": "b_p", "type": "io.print", "v": 1});
    // Copied blocks are top-level: a stack is allowed.
    let top = json!([{"id": "b_a", "type": "io.print", "v": 1, "stack": [print]}]);
    let clipboard = load_clipboard(&serde_json::to_vec(&payload(top)).unwrap()).unwrap();
    assert_eq!(clipboard.blocks[0].stack[0].id.as_str(), "b_p");
    // Nested and stacked blocks have no position, stacks are not nested,
    // never empty.
    let nested = json!([{"id": "b_a", "type": "control.forever", "v": 1,
        "statements": {"BODY": [{"id": "b_b", "type": "io.print", "v": 1, "x": 1, "y": 1}]}}]);
    assert_eq!(codes(&payload(nested)), ["B2C-E0128"]);
    let stacked = json!([{"id": "b_a", "type": "io.print", "v": 1,
        "stack": [{"id": "b_b", "type": "io.print", "v": 1, "y": 1}]}]);
    assert_eq!(codes(&payload(stacked)), ["B2C-E0128"]);
    let in_stack = json!([{"id": "b_a", "type": "io.print", "v": 1,
        "stack": [{"id": "b_b", "type": "io.print", "v": 1, "stack": [print]}]}]);
    assert_eq!(codes(&payload(in_stack)), ["B2C-E0139"]);
    let empty = json!([{"id": "b_a", "type": "io.print", "v": 1, "stack": []}]);
    assert_eq!(codes(&payload(empty)), ["B2C-E0112"]);
    // `blocks` is a list of blocks.
    assert_eq!(codes(&payload(json!({}))), ["B2C-E0112"]);
    assert_eq!(codes(&payload(json!([1]))), ["B2C-E0112"]);
    // Expression tokens follow the same rules as in a file.
    let tokens: Vec<Value> = (0..513).map(|_| json!({"op": "+"})).collect();
    let long = json!([{"id": "b_a", "type": "io.print", "v": 1, "inputs": {"ITEM0": {"expr": tokens}}}]);
    assert_eq!(codes(&payload(long)), ["B2C-E0122"]);
}

#[test]
fn a_paste_ignores_the_position_of_copied_blocks() {
    // A top-level copied block may carry `x`/`y` (05 §5.12): it is checked
    // like a canvas position, then dropped, so the payload means the same as
    // one without it and has one canonical spelling, without `x`/`y`.
    let top = |at: Value| {
        let mut block = json!({"id": "b_a", "type": "io.print", "v": 1,
            "stack": [{"id": "b_p", "type": "io.print", "v": 1}]});
        if let Value::Object(at) = at {
            block.as_object_mut().unwrap().extend(at);
        }
        payload(json!([block]))
    };
    let load_value = |value: &Value| load_clipboard(&serde_json::to_vec(value).unwrap()).unwrap();
    let plain = load_value(&top(json!({})));
    let canonical = to_canonical_clipboard_json(&plain);
    assert!(
        !canonical.contains("\"x\"") && !canonical.contains("\"y\""),
        "{canonical}"
    );
    for at in [
        json!({"x": -5, "y": 5}),
        json!({"x": 7}),
        json!({"y": 7}),
        json!({"x": null, "y": null}),
    ] {
        let positioned = load_value(&top(at.clone()));
        assert_eq!(positioned, plain, "{at}");
        assert_eq!(to_canonical_clipboard_json(&positioned), canonical, "{at}");
    }
    // Out of range is still an error, as on a canvas.
    assert_eq!(codes(&top(json!({"x": 2_147_483_648_i64}))), ["B2C-E0129"]);
    // The writer never writes a position, even one set by the caller (as
    // `clipboard_make` gets blocks straight from a canvas).
    let mut moved = plain.clone();
    moved.blocks[0].x = Some(300);
    moved.blocks[0].y = Some(-40);
    assert_eq!(to_canonical_clipboard_json(&moved), canonical);
}

#[test]
fn messages_talk_about_the_pasted_data() {
    let blocks = json!([{"id": "b_a", "type": "io.print", "v": 1, "fields": {"__proto__": "x"}}]);
    let error = load_clipboard(&serde_json::to_vec(&payload(blocks)).unwrap()).unwrap_err();
    let diagnostic = &error.diagnostics[0];
    assert_eq!(diagnostic.code.0, "B2C-E0127");
    assert_eq!(
        diagnostic.message,
        "\"fields\" in this block uses the key \"__proto__\", which is not allowed in pasted blocks because it could be used to tamper with the editor."
    );
    // Pasted blocks belong to no module yet.
    assert_eq!(
        diagnostic.primary,
        Location {
            module: None,
            block: Some(BlockId::new("b_a").unwrap()),
            part: Part::Whole,
        }
    );
    let mut value = payload(json!([]));
    value["junk"] = json!(1);
    let error = load_clipboard(&serde_json::to_vec(&value).unwrap()).unwrap_err();
    assert_eq!(
        error.diagnostics[0].message,
        "The pasted data has an unknown key \"junk\". Remove it or check its spelling."
    );
}

#[test]
fn the_block_limit_applies_to_pastes() {
    let blocks: Vec<Value> = (0..=MAX_BLOCKS)
        .map(|i| json!({"id": format!("b{i}"), "type": "control.break", "v": 1}))
        .collect();
    let error = load_clipboard(&serde_json::to_vec(&payload(Value::Array(blocks))).unwrap()).unwrap_err();
    assert_eq!(error.diagnostics.len(), 1);
    assert_eq!(error.diagnostics[0].code.0, "B2C-E0121");
    assert_eq!(
        error.diagnostics[0].message,
        "The pasted data has more than 100000 blocks, which is the most a project can have."
    );
}

/// Every block of a tree, parents first.
fn all_blocks(blocks: &[Block]) -> Vec<&Block> {
    let mut out = Vec::new();
    let mut pending: Vec<&Block> = blocks.iter().rev().collect();
    while let Some(block) = pending.pop() {
        out.push(block);
        pending.extend(block.stack.iter().rev());
        for list in block.statements.values().rev() {
            pending.extend(list.iter().rev());
        }
        for input in block.inputs.values().rev() {
            if let Input::Block(nested) = input {
                pending.push(&nested.block);
            }
        }
    }
    out
}

/// Every symbol reference (fields and tokens) in a tree.
fn references(blocks: &[Block]) -> Vec<SymbolId> {
    let mut out = Vec::new();
    for block in all_blocks(blocks) {
        for value in block.fields.values() {
            if let FieldValue::Ref(reference) = value {
                out.push(reference.target.clone());
            }
        }
        for input in block.inputs.values() {
            if let Input::Expr(expr) = input {
                out.extend(expr.expr.iter().filter_map(|t| match t {
                    Token::Ref(sym) => Some(sym.clone()),
                    _ => None,
                }));
            }
        }
    }
    out
}

/// Copies every module of every example to the clipboard and pastes it into
/// the same document 20 times: each copy loads, and the document with all
/// the pasted copies still loads (fresh block and symbol IDs, references
/// rewritten), which is what duplicate and paste rely on.
#[test]
fn examples_survive_copy_and_repeated_paste() {
    for path in example_paths() {
        let name = path.display().to_string();
        let mut document: Document = load(&std::fs::read(&path).unwrap()).unwrap();
        let originals = document.clone();
        for (index, module) in originals.modules.iter().enumerate() {
            let blocks = module.workspace.blocks.clone();
            let refs: BTreeMap<SymbolId, ClipboardRef> = outside_refs(&blocks)
                .into_iter()
                .map(|sym| {
                    let reference = ClipboardRef {
                        name: format!("{}::{}", module.name, sym.as_str()),
                        kind: RefKind::Variable,
                    };
                    (sym, reference)
                })
                .collect();
            let clipboard = Clipboard {
                catalog: originals.generator.catalog.clone(),
                blocks,
                refs,
            };
            let text = to_canonical_clipboard_json(&clipboard);
            let pasted = load_clipboard(text.as_bytes()).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            // The copy keeps everything but the canvas positions, which a
            // paste never uses (spec §5.12).
            let mut expected = clipboard.clone();
            for block in &mut expected.blocks {
                (block.x, block.y) = (None, None);
            }
            assert_eq!(pasted, expected, "{name}");

            for round in 0..20u8 {
                let mut blocks = pasted.blocks.clone();
                let taken = used_ids(&document);
                let mut seed = [round; 32];
                seed[0] = u8::try_from(index).unwrap();
                let renamed = remap_ids(&mut blocks, &taken, &mut SeededIds::new(seed)).unwrap();
                // Every block got a new ID that was free.
                for block in all_blocks(&blocks) {
                    assert!(!taken.contains(block.id.as_str()), "{name}");
                    assert!(block.id.as_str().starts_with("blk_"), "{name}");
                }
                // Declared symbols got new IDs; references follow them, and
                // references to outside symbols are unchanged.
                for (old, new) in &renamed {
                    assert!(!taken.contains(new.as_str()), "{name}");
                    assert!(taken.contains(old.as_str()), "{name}");
                }
                let before = references(&pasted.blocks);
                let after = references(&blocks);
                assert_eq!(before.len(), after.len(), "{name}");
                for (old, new) in before.iter().zip(&after) {
                    assert_eq!(renamed.get(old).unwrap_or(old), new, "{name}");
                }
                assert_eq!(outside_refs(&blocks), outside_refs(&pasted.blocks), "{name}");
                document.modules[index].workspace.blocks.extend(blocks);
                let saved = to_canonical_json(&document);
                if let Err(error) = load(saved.as_bytes()) {
                    panic!("{name}, paste {round}: {:#?}", error.diagnostics);
                }
            }
        }
    }
}
