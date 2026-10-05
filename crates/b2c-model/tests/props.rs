//! Property tests: canonical round trips, hashing, agreement with `serde`,
//! and robustness against arbitrary input.

// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

use std::collections::BTreeMap;

use b2c_ir::sast::CppStandard;
use b2c_ir::{BlockId, ModuleId, ProjectId, SymbolId};
use b2c_model::{
    Block, BlockComment, BlockInput, BuildConfiguration, BuildSettings, Configurations, Define, DefineValue,
    Document, ExprInput, FieldValue, FormattingStyle, Frame, FrameColor, Generator, Input, Language, Module,
    Note, Optimization, PackRef, Project, ProjectOptions, RunSettings, Sanitizer, SymbolDecl, SymbolRef,
    Token, Viewport, WarningLevel, WorkingDirectory, Workspace, content_hash, load, to_canonical_json,
};
use proptest::collection::{btree_map, vec};
use proptest::option;
use proptest::prelude::*;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Strategies for valid documents
// ---------------------------------------------------------------------------

/// Characters allowed in project text (spec §5.6).
fn allowed(c: char) -> bool {
    let control = c < ' ' && c != '\t' && c != '\n';
    let bidi =
        matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
    !control && !bidi
}

fn text() -> impl Strategy<Value = String> {
    vec(any::<char>(), 0..12).prop_map(|chars| chars.into_iter().filter(|c| allowed(*c)).collect())
}

/// A map key that is not reserved (`__proto__`, …) and not `params`.
fn key() -> impl Strategy<Value = String> {
    prop_oneof![
        "[A-Z][A-Z0-9_]{0,6}",
        text().prop_filter("reserved", |k| {
            !matches!(k.as_str(), "__proto__" | "constructor" | "prototype" | "params")
        }),
    ]
}

fn id_text() -> impl Strategy<Value = String> {
    "[A-Za-z0-9_]{1,32}"
}

fn placeholder_block_id() -> BlockId {
    BlockId::new("placeholder").unwrap()
}

fn placeholder_symbol() -> SymbolId {
    SymbolId::new("placeholder").unwrap()
}

fn symbol_ref() -> impl Strategy<Value = SymbolId> {
    id_text().prop_map(|id| SymbolId::new(&id).unwrap())
}

fn token() -> impl Strategy<Value = Token> {
    prop_oneof![
        text().prop_map(Token::Num),
        text().prop_map(Token::Str),
        text().prop_map(Token::Chr),
        symbol_ref().prop_map(Token::Ref),
        text().prop_map(Token::Op),
        text().prop_map(Token::Kw),
        text().prop_map(Token::Text),
    ]
}

fn field_value() -> impl Strategy<Value = FieldValue> {
    prop_oneof![
        any::<bool>().prop_map(FieldValue::Bool),
        text().prop_map(FieldValue::Text),
        text().prop_map(|name| FieldValue::Decl(SymbolDecl {
            sym: placeholder_symbol(),
            name,
        })),
        symbol_ref().prop_map(|target| FieldValue::Ref(SymbolRef { target })),
    ]
}

/// A finite `f64` as a JSON number.
fn float() -> impl Strategy<Value = Value> {
    any::<f64>()
        .prop_filter("finite", |x| x.is_finite())
        .prop_map(|x| Value::Number(serde_json::Number::from_f64(x).unwrap()))
}

/// Free-form JSON whose strings and keys follow the text rules. `extra`
/// numbers stay at most 64 (the variadic limit).
fn free_json(for_extra: bool) -> impl Strategy<Value = Value> {
    let number = if for_extra {
        prop_oneof![
            (-1000i64..=64).prop_map(Value::from),
            (-1.0e6f64..64.0).prop_map(Value::from),
        ]
        .boxed()
    } else {
        prop_oneof![
            any::<i64>().prop_map(Value::from),
            any::<u64>().prop_map(Value::from),
            float()
        ]
        .boxed()
    };
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        number,
        text().prop_map(Value::String),
    ];
    leaf.prop_recursive(3, 16, 4, |inner| {
        prop_oneof![
            vec(inner.clone(), 0..4).prop_map(Value::Array),
            btree_map(key(), inner, 0..4).prop_map(|map| Value::Object(map.into_iter().collect())),
        ]
    })
}

fn comment() -> impl Strategy<Value = BlockComment> {
    (text(), any::<bool>()).prop_map(|(text, pinned)| BlockComment { text, pinned })
}

fn input() -> impl Strategy<Value = Input> {
    (vec(token(), 0..6), any::<bool>()).prop_map(|(expr, draft)| Input::Expr(ExprInput { expr, draft }))
}

/// A block without children; IDs are filled in later.
fn leaf_block() -> impl Strategy<Value = Block> {
    (
        (
            text(),
            any::<u32>(),
            any::<bool>(),
            any::<bool>(),
            option::of(comment()),
        ),
        btree_map(key(), free_json(true), 0..3),
        btree_map(key(), field_value(), 0..3),
        btree_map(key(), input(), 0..3),
    )
        .prop_map(
            |((block_type, v, collapsed, disabled, comment), extra, fields, inputs)| Block {
                id: placeholder_block_id(),
                block_type,
                v,
                x: None,
                y: None,
                collapsed,
                disabled,
                comment,
                extra,
                fields,
                inputs,
                statements: BTreeMap::new(),
                stack: Vec::new(),
            },
        )
}

fn block() -> impl Strategy<Value = Block> {
    leaf_block().prop_recursive(3, 20, 3, |inner| {
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

fn position() -> impl Strategy<Value = i32> {
    -10_000_000..=10_000_000i32
}

/// A top-level block, sometimes with a loose stack below it (ADR-0011).
fn top_level_block() -> impl Strategy<Value = Block> {
    (
        block(),
        option::of((position(), position())),
        prop_oneof![3 => Just(Vec::new()), 1 => vec(block(), 1..3)],
    )
        .prop_map(|(mut block, at, stack)| {
            block.x = at.map(|(x, _)| x);
            block.y = at.map(|(_, y)| y);
            block.stack = stack;
            block
        })
}

fn frame() -> impl Strategy<Value = Frame> {
    (
        text(),
        position(),
        position(),
        0..=10_000_000i32,
        0..=10_000_000i32,
        prop::sample::select(vec![
            FrameColor::Grey,
            FrameColor::Blue,
            FrameColor::Green,
            FrameColor::Yellow,
            FrameColor::Orange,
            FrameColor::Purple,
        ]),
        any::<bool>(),
    )
        .prop_map(|(title, x, y, w, h, color, emit_banner)| Frame {
            id: placeholder_block_id(),
            title,
            x,
            y,
            w,
            h,
            color,
            emit_banner,
        })
}

fn note() -> impl Strategy<Value = Note> {
    (text(), position(), position()).prop_map(|(text, x, y)| Note {
        id: placeholder_block_id(),
        text,
        x,
        y,
    })
}

fn workspace() -> impl Strategy<Value = Workspace> {
    (
        vec(top_level_block(), 0..4),
        vec(frame(), 0..2),
        vec(note(), 0..2),
        option::of((position(), position(), 0.1f64..=4.0)),
    )
        .prop_map(|(blocks, frames, notes, viewport)| Workspace {
            blocks,
            frames,
            notes,
            viewport: viewport.map(|(x, y, scale)| Viewport { x, y, scale }),
        })
}

fn configuration() -> impl Strategy<Value = BuildConfiguration> {
    (
        prop::sample::select(vec![
            Optimization::None,
            Optimization::Debug,
            Optimization::Speed,
            Optimization::Size,
        ]),
        any::<bool>(),
        vec(
            prop::sample::select(vec![Sanitizer::Address, Sanitizer::Undefined]),
            0..3,
        ),
        prop::sample::select(vec![
            WarningLevel::Minimal,
            WarningLevel::Helpful,
            WarningLevel::Strict,
        ]),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            |(optimization, debug_info, sanitizers, warnings, warnings_as_errors, hardening)| {
                BuildConfiguration {
                    optimization,
                    debug_info,
                    sanitizers,
                    warnings,
                    warnings_as_errors,
                    hardening,
                }
            },
        )
}

fn define_value() -> impl Strategy<Value = DefineValue> {
    prop_oneof![
        any::<i64>().prop_map(DefineValue::Int),
        any::<bool>().prop_map(DefineValue::Bool),
        text().prop_map(DefineValue::String),
    ]
}

fn project() -> impl Strategy<Value = Project> {
    (
        (id_text(), text(), text()),
        (
            prop::sample::select(vec![
                CppStandard::Cpp17,
                CppStandard::Cpp20,
                CppStandard::Cpp23,
                CppStandard::Cpp26,
            ]),
            any::<bool>(),
        ),
        (
            any::<[bool; 4]>(),
            prop::sample::select(vec![FormattingStyle::Stream, FormattingStyle::Format]),
        ),
        (
            configuration(),
            configuration(),
            vec(define_value(), 0..3),
            vec("[A-Za-z0-9_+.-]{1,16}", 0..3),
            vec("[<>=~^]{0,2}[0-9]{1,3}(\\.[0-9*]{1,3}){0,2}", 0..3),
        ),
        (
            vec(text(), 0..3),
            prop::sample::select(vec![WorkingDirectory::Project, WorkingDirectory::Sandbox]),
        ),
    )
        .prop_map(
            |(
                (id, name, description),
                (standard, gnu_extensions),
                (flags, formatting_style),
                (debug, release, defines, libraries, packs),
                (args, working_directory),
            )| Project {
                id: ProjectId::new(&id).unwrap(),
                name,
                description,
                language: Language {
                    standard,
                    gnu_extensions,
                },
                options: ProjectOptions {
                    show_advanced: flags[0],
                    manual_memory: flags[1],
                    prefer_plain_std: flags[2],
                    formatting_style,
                    checked_indexing: flags[3],
                },
                build: BuildSettings {
                    configurations: Configurations { debug, release },
                    defines: defines
                        .into_iter()
                        .enumerate()
                        .map(|(i, value)| Define {
                            name: format!("DEFINE_{i}"),
                            value,
                        })
                        .collect(),
                    libraries,
                    // Pack IDs must be unique: number them.
                    packs: packs
                        .into_iter()
                        .enumerate()
                        .map(|(i, version)| PackRef {
                            id: format!("pack-{i}"),
                            version,
                        })
                        .collect(),
                },
                run: RunSettings {
                    args,
                    working_directory,
                },
            },
        )
}

/// Gives every block, frame and note a unique ID, and every declaration a
/// unique symbol, so the document is valid.
struct Numbering {
    blocks: usize,
    symbols: usize,
}

impl Numbering {
    fn block_id(&mut self) -> BlockId {
        self.blocks += 1;
        BlockId::new(&format!("b{:05}", self.blocks)).unwrap()
    }

    fn block(&mut self, block: &mut Block) {
        block.id = self.block_id();
        for value in block.fields.values_mut() {
            if let FieldValue::Decl(decl) = value {
                self.symbols += 1;
                decl.sym = SymbolId::new(&format!("s{}", self.symbols)).unwrap();
            }
        }
        for input in block.inputs.values_mut() {
            if let Input::Block(nested) = input {
                self.block(&mut nested.block);
            }
        }
        for list in block.statements.values_mut() {
            for child in list {
                self.block(child);
            }
        }
        for child in &mut block.stack {
            self.block(child);
        }
    }
}

fn document() -> impl Strategy<Value = Document> {
    (
        (text(), text()),
        project(),
        vec(workspace(), 1..3),
        // "x-ext" is always an object (spec §5.6).
        option::of(
            btree_map(key(), free_json(false), 0..4).prop_map(|map| Value::Object(map.into_iter().collect())),
        ),
    )
        .prop_map(|((app, catalog), project, workspaces, ext)| {
            let mut numbering = Numbering {
                blocks: 0,
                symbols: 0,
            };
            let modules = workspaces
                .into_iter()
                .enumerate()
                .map(|(i, mut workspace)| {
                    for block in &mut workspace.blocks {
                        numbering.block(block);
                    }
                    for frame in &mut workspace.frames {
                        frame.id = numbering.block_id();
                    }
                    for note in &mut workspace.notes {
                        note.id = numbering.block_id();
                    }
                    workspace.blocks.sort_by(|a, b| a.id.cmp(&b.id));
                    Module {
                        id: ModuleId::new(&format!("mod{i}")).unwrap(),
                        name: format!("m{i}"),
                        workspace,
                    }
                })
                .collect();
            Document {
                format: b2c_model::FORMAT_TAG.to_owned(),
                format_version: b2c_model::CURRENT_FORMAT_VERSION,
                generator: Generator { app, catalog },
                project,
                modules,
                ext,
            }
        })
}

// ---------------------------------------------------------------------------
// Arbitrary (mostly invalid) input
// ---------------------------------------------------------------------------

/// Any JSON value, with any strings and keys.
fn any_json() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::from),
        float(),
        any::<String>().prop_map(Value::String),
        prop::sample::select(vec![
            "blocks2cpp/project",
            "__proto__",
            "program.main",
            "c++20",
            "none",
            "mod_main",
        ])
        .prop_map(Value::from),
    ];
    leaf.prop_recursive(5, 64, 6, |inner| {
        let keys = prop_oneof![
            any::<String>(),
            prop::sample::select(vec![
                "format",
                "formatVersion",
                "generator",
                "project",
                "modules",
                "id",
                "name",
                "type",
                "v",
                "x",
                "y",
                "blocks",
                "workspace",
                "fields",
                "inputs",
                "statements",
                "stack",
                "extra",
                "expr",
                "block",
                "sym",
                "ref",
                "language",
                "standard",
                "x-ext",
            ])
            .prop_map(String::from),
        ];
        prop_oneof![
            vec(inner.clone(), 0..6).prop_map(Value::Array),
            btree_map(keys, inner, 0..6).prop_map(|map| Value::Object(map.into_iter().collect())),
        ]
    })
}

/// A byte-level edit of a file.
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

/// Replaces the value at a pseudo-random path (stopping early at a scalar).
fn replace_at(value: &mut Value, path: &[prop::sample::Index], replacement: Value) {
    let Some((first, rest)) = path.split_first() else {
        *value = replacement;
        return;
    };
    match value {
        Value::Array(items) if !items.is_empty() => {
            let i = first.index(items.len());
            replace_at(&mut items[i], rest, replacement);
        }
        Value::Object(map) if !map.is_empty() => {
            let i = first.index(map.len());
            replace_at(map.values_mut().nth(i).unwrap(), rest, replacement);
        }
        _ => *value = replacement,
    }
}

/// Whatever `load` accepts must mean the same to `serde`, and must survive
/// another save/load cycle unchanged.
fn check_accepted(bytes: &[u8]) {
    if let Ok(document) = load(bytes) {
        let through_serde: Document = serde_json::from_slice(bytes).expect("serde rejects what load accepts");
        assert_eq!(document, through_serde);
        let canonical = to_canonical_json(&document);
        let again = load(canonical.as_bytes()).expect("canonical output must load");
        assert_eq!(to_canonical_json(&again), canonical);
    }
}

// ---------------------------------------------------------------------------
// Properties
// ---------------------------------------------------------------------------

/// 96 cases by default (the strategies are large); `PROPTEST_CASES`
/// overrides it for deeper runs.
fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(96)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    #[test]
    fn canonical_round_trip(document in document()) {
        let canonical = to_canonical_json(&document);
        let loaded = match load(canonical.as_bytes()) {
            Ok(loaded) => loaded,
            Err(error) => panic!("{:#?}\n{canonical}", error.diagnostics),
        };
        prop_assert_eq!(&loaded, &document);
        prop_assert_eq!(to_canonical_json(&loaded), canonical.clone());
        let well_formed = canonical.ends_with("}\n") && !canonical.contains('\r');
        prop_assert!(well_formed);
    }

    #[test]
    fn canonical_output_is_what_serde_writes(document in document()) {
        let canonical = to_canonical_json(&document);
        prop_assert_eq!(&canonical, &(serde_json::to_string_pretty(&document).unwrap() + "\n"));
        let through_serde: Document = serde_json::from_str(&canonical).unwrap();
        prop_assert_eq!(through_serde, document);
    }

    #[test]
    fn top_level_order_is_not_significant(document in document()) {
        let mut shuffled = document.clone();
        for module in &mut shuffled.modules {
            module.workspace.blocks.reverse();
        }
        prop_assert_eq!(to_canonical_json(&shuffled), to_canonical_json(&document));
        prop_assert_eq!(content_hash(&shuffled), content_hash(&document));
    }

    #[test]
    fn layout_never_changes_the_hash(document in document(), dx in 1..1000i32) {
        let mut moved = document.clone();
        for module in &mut moved.modules {
            let workspace = &mut module.workspace;
            for block in &mut workspace.blocks {
                block.x = Some(block.x.unwrap_or(0).saturating_add(dx).min(10_000_000));
                block.y = None;
                block.collapsed = !block.collapsed;
                if let Some(comment) = &mut block.comment {
                    comment.pinned = !comment.pinned;
                }
            }
            workspace.frames.clear();
            workspace.notes.push(Note { id: BlockId::new("note_new").unwrap(), text: "moved".into(), x: dx, y: 0 });
            workspace.viewport = Some(Viewport { x: dx, y: -dx, scale: 2.5 });
        }
        prop_assert_eq!(content_hash(&moved), content_hash(&document));
    }

    #[test]
    fn semantic_changes_change_the_hash(document in document()) {
        let original = content_hash(&document);
        let mut renamed = document.clone();
        renamed.project.name.push('!');
        prop_assert_ne!(content_hash(&renamed), original);

        let mut standard = document.clone();
        standard.project.language.gnu_extensions = !standard.project.language.gnu_extensions;
        prop_assert_ne!(content_hash(&standard), original);

        let first_block = document.modules.iter().position(|m| !m.workspace.blocks.is_empty());
        if let Some(index) = first_block {
            let mut disabled = document.clone();
            let block = &mut disabled.modules[index].workspace.blocks[0];
            block.disabled = !block.disabled;
            prop_assert_ne!(content_hash(&disabled), original);

            let mut retyped = document.clone();
            retyped.modules[index].workspace.blocks[0].block_type.push('x');
            prop_assert_ne!(content_hash(&retyped), original);
        }
    }

    #[test]
    fn arbitrary_bytes_never_panic(bytes in vec(any::<u8>(), 0..512)) {
        check_accepted(&bytes);
    }

    #[test]
    fn arbitrary_json_never_panics(value in any_json()) {
        check_accepted(&serde_json::to_vec(&value).unwrap());
        check_accepted(&serde_json::to_vec_pretty(&value).unwrap());
    }

    #[test]
    fn edited_projects_never_panic(document in document(), edits in edits()) {
        let bytes = apply(to_canonical_json(&document).into_bytes(), &edits);
        check_accepted(&bytes);
    }

    #[test]
    fn edited_json_values_never_panic(document in document(), path in vec(any::<prop::sample::Index>(), 1..6), replacement in any_json()) {
        // Replace one value somewhere in a valid document.
        let mut value = serde_json::to_value(&document).unwrap();
        replace_at(&mut value, &path, replacement);
        check_accepted(&serde_json::to_vec(&value).unwrap());
    }
}

#[test]
fn deep_and_wide_inputs_fail_cleanly() {
    // Deeply nested blocks: each level costs three JSON levels.
    let mut block = serde_json::json!({"id": "leaf", "type": "control.break", "v": 1});
    for i in 0..60 {
        block = serde_json::json!({"id": format!("b{i}"), "type": "control.forever", "v": 1, "statements": {"BODY": [block]}});
    }
    let mut text = String::from(
        r#"{"format":"blocks2cpp/project","formatVersion":1,"generator":{"app":"0","catalog":"1"},"project":{"id":"p","name":"n","language":{"standard":"c++20"}},"modules":[{"id":"m","name":"main","workspace":{"blocks":["#,
    );
    text.push_str(&block.to_string());
    text.push_str("]}}]}");
    let error = load(text.as_bytes()).unwrap_err();
    assert_eq!(error.diagnostics[0].code.0, "B2C-E0104");
}
