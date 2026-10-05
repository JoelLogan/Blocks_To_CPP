//! Property tests for the clipboard format and fresh IDs: canonical round
//! trips, `remap_ids` (fresh, valid, unique IDs; references rewritten), and
//! robustness of `load_clipboard` against arbitrary input.

// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::{BlockId, SymbolId};
use b2c_model::{
    Block, BlockComment, BlockInput, Clipboard, ClipboardRef, ExprInput, FieldValue, IdKind, IdSource, Input,
    RefKind, SeededIds, SymbolDecl, SymbolRef, Token, load_clipboard, outside_refs, remap_ids, rewrite_refs,
    to_canonical_clipboard_json,
};
use proptest::collection::{btree_map, vec};
use proptest::option;
use proptest::prelude::*;
use serde_json::{Value, json};

// ---------------------------------------------------------------------------
// Strategies
// ---------------------------------------------------------------------------

/// Characters allowed in project text (spec §5.6).
fn allowed(c: char) -> bool {
    let control = c < ' ' && c != '\t' && c != '\n';
    let bidi =
        matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
    !control && !bidi
}

fn text() -> impl Strategy<Value = String> {
    vec(any::<char>(), 0..10).prop_map(|chars| chars.into_iter().filter(|c| allowed(*c)).collect())
}

fn key() -> impl Strategy<Value = String> {
    "[A-Z][A-Z0-9_]{0,4}"
}

/// References pick from a small pool, so some name symbols declared in the
/// blocks (`s1`, `s2`, …, numbered below) and some do not (`out1`, …).
fn symbol_ref() -> impl Strategy<Value = SymbolId> {
    prop_oneof![
        (1..12u32).prop_map(|n| SymbolId::new(&format!("s{n}")).unwrap()),
        (1..4u32).prop_map(|n| SymbolId::new(&format!("out{n}")).unwrap()),
    ]
}

fn placeholder_symbol() -> SymbolId {
    SymbolId::new("placeholder").unwrap()
}

fn field_value() -> impl Strategy<Value = FieldValue> {
    prop_oneof![
        any::<bool>().prop_map(FieldValue::Bool),
        text().prop_map(FieldValue::Text),
        text().prop_map(|name| FieldValue::Decl(SymbolDecl {
            sym: placeholder_symbol(),
            name
        })),
        symbol_ref().prop_map(|target| FieldValue::Ref(SymbolRef { target })),
    ]
}

fn token() -> impl Strategy<Value = Token> {
    prop_oneof![
        text().prop_map(Token::Num),
        text().prop_map(Token::Str),
        text().prop_map(Token::Op),
        symbol_ref().prop_map(Token::Ref),
    ]
}

/// `extra`, sometimes with function parameter rows (which declare symbols).
fn extra() -> impl Strategy<Value = BTreeMap<String, Value>> {
    (option::of(vec(text(), 0..3)), option::of(0..=64u32)).prop_map(|(params, count)| {
        let mut extra = BTreeMap::new();
        if let Some(names) = params {
            let rows = names
                .into_iter()
                .map(|name| json!({"sym": "placeholder", "name": name, "type": "int", "mode": "copy"}))
                .collect();
            extra.insert("params".to_owned(), Value::Array(rows));
        }
        if let Some(count) = count {
            extra.insert("itemCount".to_owned(), Value::from(count));
        }
        extra
    })
}

fn leaf_block() -> impl Strategy<Value = Block> {
    (
        (text(), any::<u32>(), any::<bool>(), any::<bool>()),
        option::of((text(), any::<bool>())),
        extra(),
        btree_map(key(), field_value(), 0..3),
        btree_map(key(), (vec(token(), 0..5), any::<bool>()), 0..3),
    )
        .prop_map(
            |((block_type, v, collapsed, disabled), comment, extra, fields, exprs)| Block {
                id: BlockId::new("placeholder").unwrap(),
                block_type,
                v,
                x: None,
                y: None,
                collapsed,
                disabled,
                comment: comment.map(|(text, pinned)| BlockComment { text, pinned }),
                extra,
                fields,
                inputs: exprs
                    .into_iter()
                    .map(|(name, (expr, draft))| (name, Input::Expr(ExprInput { expr, draft })))
                    .collect(),
                statements: BTreeMap::new(),
                stack: Vec::new(),
            },
        )
}

fn block() -> impl Strategy<Value = Block> {
    leaf_block().prop_recursive(3, 24, 3, |inner| {
        (
            leaf_block(),
            btree_map(key(), vec(inner.clone(), 0..3), 0..2),
            btree_map(key(), inner, 0..2),
        )
            .prop_map(|(mut block, statements, nested)| {
                block.statements = statements;
                for (name, child) in nested {
                    block.inputs.insert(
                        name,
                        Input::Block(BlockInput {
                            block: Box::new(child),
                        }),
                    );
                }
                block
            })
    })
}

fn copied_block() -> impl Strategy<Value = Block> {
    (
        block(),
        option::of((-10_000_000..=10_000_000i32, -10_000_000..=10_000_000i32)),
        prop_oneof![2 => Just(Vec::new()), 1 => vec(block(), 1..3)],
    )
        .prop_map(|(mut block, at, stack)| {
            block.x = at.map(|(x, _)| x);
            block.y = at.map(|(_, y)| y);
            block.stack = stack;
            block
        })
}

/// Gives every block a unique ID and every declaration a unique symbol.
#[derive(Default)]
struct Numbering {
    blocks: usize,
    symbols: usize,
}

impl Numbering {
    fn symbol(&mut self) -> SymbolId {
        self.symbols += 1;
        SymbolId::new(&format!("s{}", self.symbols)).unwrap()
    }

    fn number(&mut self, blocks: &mut [Block]) {
        for block in blocks {
            self.blocks += 1;
            block.id = BlockId::new(&format!("b{}", self.blocks)).unwrap();
            for value in block.fields.values_mut() {
                if let FieldValue::Decl(decl) = value {
                    decl.sym = self.symbol();
                }
            }
            if let Some(Value::Array(rows)) = block.extra.get_mut("params") {
                for row in rows {
                    row["sym"] = Value::from(self.symbol().as_str());
                }
            }
            for input in block.inputs.values_mut() {
                if let Input::Block(nested) = input {
                    self.number(std::slice::from_mut(&mut *nested.block));
                }
            }
            for list in block.statements.values_mut() {
                self.number(list);
            }
            self.number(&mut block.stack);
        }
    }
}

fn ref_kind() -> impl Strategy<Value = RefKind> {
    prop::sample::select(vec![
        RefKind::Variable,
        RefKind::Parameter,
        RefKind::LoopVariable,
        RefKind::Function,
    ])
}

/// A valid clipboard payload whose `refs` describe its outside references.
fn clipboard() -> impl Strategy<Value = Clipboard> {
    (text(), vec(copied_block(), 0..4), vec((text(), ref_kind()), 4)).prop_map(
        |(catalog, mut blocks, names)| {
            Numbering::default().number(&mut blocks);
            let refs = outside_refs(&blocks)
                .into_iter()
                .zip(names.into_iter().cycle())
                .map(|(sym, (name, kind))| (sym, ClipboardRef { name, kind }))
                .collect();
            Clipboard {
                catalog,
                blocks,
                refs,
            }
        },
    )
}

// ---------------------------------------------------------------------------
// Inspecting trees
// ---------------------------------------------------------------------------

/// Every block, parents first, in the order `remap_ids` visits them.
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

/// Declared symbols, in walk order: declaration fields, then parameter rows.
fn declared(block: &Block) -> Vec<SymbolId> {
    let mut out: Vec<SymbolId> = block
        .fields
        .values()
        .filter_map(|v| match v {
            FieldValue::Decl(decl) => Some(decl.sym.clone()),
            _ => None,
        })
        .collect();
    if let Some(Value::Array(rows)) = block.extra.get("params") {
        out.extend(
            rows.iter()
                .map(|row| SymbolId::new(row["sym"].as_str().unwrap()).unwrap()),
        );
    }
    out
}

/// References of one block: fields, then tokens.
fn referenced(block: &Block) -> Vec<SymbolId> {
    let mut out: Vec<SymbolId> = block
        .fields
        .values()
        .filter_map(|v| match v {
            FieldValue::Ref(r) => Some(r.target.clone()),
            _ => None,
        })
        .collect();
    for input in block.inputs.values() {
        if let Input::Expr(expr) = input {
            out.extend(expr.expr.iter().filter_map(|t| match t {
                Token::Ref(sym) => Some(sym.clone()),
                _ => None,
            }));
        }
    }
    out
}

fn is_id(text: &str) -> bool {
    (1..=32).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// A byte-level edit of a payload.
#[derive(Debug, Clone)]
enum Edit {
    Insert(usize, u8),
    Delete(usize),
    Replace(usize, u8),
}

fn edits() -> impl Strategy<Value = Vec<Edit>> {
    let byte = prop_oneof![
        any::<u8>(),
        prop::sample::select(b"{}[]\",:0123456789-.eE\\untrf ".to_vec())
    ];
    vec(
        prop_oneof![
            (any::<usize>(), byte.clone()).prop_map(|(at, b)| Edit::Insert(at, b)),
            any::<usize>().prop_map(Edit::Delete),
            (any::<usize>(), byte).prop_map(|(at, b)| Edit::Replace(at, b)),
        ],
        1..6,
    )
}

fn apply(mut bytes: Vec<u8>, edits: &[Edit]) -> Vec<u8> {
    for edit in edits {
        let len = bytes.len();
        match *edit {
            Edit::Insert(at, b) => bytes.insert(at % (len + 1), b),
            Edit::Delete(at) if len > 0 => {
                bytes.remove(at % len);
            }
            Edit::Replace(at, b) if len > 0 => bytes[at % len] = b,
            _ => {}
        }
    }
    bytes
}

/// Whatever `load_clipboard` accepts must save and load back unchanged.
fn check_accepted(bytes: &[u8]) {
    match load_clipboard(bytes) {
        Ok(clipboard) => {
            let canonical = to_canonical_clipboard_json(&clipboard);
            let again = load_clipboard(canonical.as_bytes()).expect("canonical output must load");
            assert_eq!(again, clipboard);
            assert_eq!(to_canonical_clipboard_json(&again), canonical);
        }
        Err(error) => assert!(!error.diagnostics.is_empty()),
    }
}

/// 96 cases by default; `PROPTEST_CASES` overrides it for deeper runs.
fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(96)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    #[test]
    fn canonical_clipboard_round_trip(clipboard in clipboard()) {
        let canonical = to_canonical_clipboard_json(&clipboard);
        let loaded = match load_clipboard(canonical.as_bytes()) {
            Ok(loaded) => loaded,
            Err(error) => panic!("{:#?}\n{canonical}", error.diagnostics),
        };
        prop_assert_eq!(&loaded, &clipboard);
        prop_assert_eq!(to_canonical_clipboard_json(&loaded), canonical.clone());
        let well_formed = canonical.ends_with("}\n") && !canonical.contains('\r');
        prop_assert!(well_formed);
    }

    #[test]
    fn remapped_ids_are_fresh_and_references_follow(
        clipboard in clipboard(),
        seed in any::<[u8; 32]>(),
        skip in 0..4usize,
    ) {
        let original = clipboard.blocks.clone();
        let old_blocks: Vec<String> = all_blocks(&original).iter().map(|b| b.id.as_str().to_owned()).collect();
        let old_declared: Vec<SymbolId> = all_blocks(&original).iter().flat_map(|b| declared(b)).collect();
        // `taken`: the payload's own IDs, plus the first IDs the seed would
        // give for each kind, which must be skipped.
        let mut taken: BTreeSet<String> = old_blocks.iter().cloned().collect();
        taken.extend(old_declared.iter().map(|s| s.as_str().to_owned()));
        let mut preview = SeededIds::new(seed);
        for _ in 0..skip {
            taken.insert(preview.next_id(IdKind::Block));
            taken.insert(preview.next_id(IdKind::Symbol));
        }

        let mut blocks = original.clone();
        let renamed = remap_ids(&mut blocks, &taken, &mut SeededIds::new(seed)).unwrap();

        // Block IDs: valid, prefixed, unique, not taken; structure kept.
        let new_blocks: Vec<&Block> = all_blocks(&blocks);
        prop_assert_eq!(new_blocks.len(), old_blocks.len());
        let mut seen = BTreeSet::new();
        for block in &new_blocks {
            let id = block.id.as_str();
            prop_assert!(is_id(id) && id.starts_with("blk_"), "{}", id);
            prop_assert!(!taken.contains(id));
            prop_assert!(seen.insert(id.to_owned()), "{} given twice", id);
        }
        // Symbols: every declaration renamed, to unique, free IDs.
        let declared_set: BTreeSet<&SymbolId> = old_declared.iter().collect();
        prop_assert_eq!(renamed.keys().collect::<BTreeSet<_>>(), declared_set);
        for new in renamed.values() {
            prop_assert!(is_id(new.as_str()) && new.as_str().starts_with("sym_"));
            prop_assert!(!taken.contains(new.as_str()));
            prop_assert!(seen.insert(new.as_str().to_owned()), "{} given twice", new);
        }
        // Declarations and references point at the new IDs; references to
        // outside symbols are unchanged.
        for (before, after) in all_blocks(&original).iter().zip(&new_blocks) {
            let expected: Vec<SymbolId> = declared(before).iter().map(|s| renamed[s].clone()).collect();
            prop_assert_eq!(declared(after), expected);
            let expected: Vec<SymbolId> =
                referenced(before).iter().map(|s| renamed.get(s).unwrap_or(s).clone()).collect();
            prop_assert_eq!(referenced(after), expected);
        }
        prop_assert_eq!(outside_refs(&blocks), outside_refs(&original));

        // The pasted payload is valid.
        let pasted = Clipboard { blocks: blocks.clone(), ..clipboard.clone() };
        prop_assert!(load_clipboard(to_canonical_clipboard_json(&pasted).as_bytes()).is_ok());
        // Nothing else changed: replaying the old IDs in the same order
        // (symbols first, then blocks, as remap_ids asks for them) gives
        // back the original blocks, with references following.
        let mut restored = blocks;
        let mut ids = Replay(old_blocks, old_declared.iter().map(|s| s.as_str().to_owned()).collect());
        let back = remap_ids(&mut restored, &BTreeSet::new(), &mut ids).unwrap();
        prop_assert_eq!(back.len(), renamed.len());
        for (new, old) in &back {
            prop_assert_eq!(&renamed[old], new);
        }
        prop_assert_eq!(&restored, &original);
    }

    #[test]
    fn rewriting_references_leaves_declarations(clipboard in clipboard(), target in 1..4u32) {
        let mut blocks = clipboard.blocks.clone();
        let outside = outside_refs(&blocks);
        let new = SymbolId::new(&format!("bound{target}")).unwrap();
        let map: BTreeMap<SymbolId, SymbolId> = outside.iter().map(|s| (s.clone(), new.clone())).collect();
        rewrite_refs(&mut blocks, &map);
        for (before, after) in all_blocks(&clipboard.blocks).iter().zip(all_blocks(&blocks)) {
            prop_assert_eq!(declared(before), declared(after));
            let expected: Vec<SymbolId> =
                referenced(before).iter().map(|s| map.get(s).unwrap_or(s).clone()).collect();
            prop_assert_eq!(referenced(after), expected);
        }
        let expected: BTreeSet<SymbolId> = if outside.is_empty() { BTreeSet::new() } else { [new].into() };
        prop_assert_eq!(outside_refs(&blocks), expected);
    }

    #[test]
    fn arbitrary_bytes_never_panic(bytes in vec(any::<u8>(), 0..512)) {
        check_accepted(&bytes);
    }

    #[test]
    fn edited_payloads_never_panic(clipboard in clipboard(), edits in edits()) {
        let bytes = apply(to_canonical_clipboard_json(&clipboard).into_bytes(), &edits);
        check_accepted(&bytes);
    }
}

/// An ID source that hands out given block and symbol IDs in order.
struct Replay(Vec<String>, Vec<String>);

impl IdSource for Replay {
    fn next_id(&mut self, kind: IdKind) -> String {
        let list = match kind {
            IdKind::Block => &mut self.0,
            IdKind::Symbol => &mut self.1,
        };
        if list.is_empty() {
            String::from("exhausted")
        } else {
            list.remove(0)
        }
    }
}
