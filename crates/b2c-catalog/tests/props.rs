//! Property tests for the resolve stage: it never panics, whatever the
//! document holds, and it is idempotent.

#![allow(clippy::unwrap_used, reason = "test helpers fail the test by panicking")]

use std::collections::BTreeMap;

use b2c_catalog::{core_catalog, resolve};
use b2c_ir::sast::CppStandard;
use b2c_ir::{BlockId, ModuleId, ProjectId, SymbolId};
use b2c_model::{
    Block, BlockInput, Document, ExprInput, FieldValue, Generator, Input, Language, Module, Project,
    SymbolDecl, SymbolRef, Token, Workspace,
};
use proptest::collection::{btree_map, vec};
use proptest::prelude::*;
use serde_json::{Value, json};

/// Names that occur in the catalog, plus some that do not.
fn names(pick: impl Fn(&b2c_catalog::BlockDef) -> Vec<String>, junk: &[&str]) -> Vec<String> {
    let mut all: Vec<String> = core_catalog().blocks.values().flat_map(pick).collect();
    all.extend(junk.iter().map(|s| (*s).to_owned()));
    all.sort();
    all.dedup();
    all
}

fn block_types() -> Vec<String> {
    names(|def| vec![def.id.clone()], &["x.unknown", ""])
}

fn field_names() -> Vec<String> {
    names(
        |def| def.field.iter().map(|f| f.name.clone()).collect(),
        &["JUNK"],
    )
}

fn extra_names() -> Vec<String> {
    names(
        |def| def.extra.iter().map(|e| e.name.clone()).collect(),
        &["junk"],
    )
}

fn input_names() -> Vec<String> {
    names(
        |def| {
            def.input
                .iter()
                .flat_map(|i| {
                    [
                        i.name.clone(),
                        format!("{}0", i.name),
                        format!("{}1", i.name),
                        format!("{}40", i.name),
                    ]
                })
                .collect()
        },
        &["JUNK", "ITEM01"],
    )
}

fn statement_names() -> Vec<String> {
    names(
        |def| {
            def.statement
                .iter()
                .flat_map(|s| [s.name.clone(), format!("{}0", s.name), format!("{}2", s.name)])
                .collect()
        },
        &["JUNK"],
    )
}

fn texts() -> Vec<String> {
    let mut all = names(
        |def| {
            def.field
                .iter()
                .flat_map(|f| {
                    f.options
                        .iter()
                        .map(|[_, v]| v.clone())
                        .chain(f.types.iter().cloned())
                })
                .collect()
        },
        &["", " ", "42", "x", "copy", "rvalue"],
    );
    all.push("\u{202e}".into());
    all
}

fn field_value() -> impl Strategy<Value = FieldValue> {
    prop_oneof![
        any::<bool>().prop_map(FieldValue::Bool),
        prop::sample::select(texts()).prop_map(FieldValue::Text),
        Just(FieldValue::Decl(SymbolDecl {
            sym: SymbolId::new("s1").unwrap(),
            name: "x".into()
        })),
        Just(FieldValue::Ref(SymbolRef {
            target: SymbolId::new("s1").unwrap()
        })),
    ]
}

fn extra_value() -> impl Strategy<Value = Value> {
    let row = (
        prop::sample::select(vec![json!("p1"), json!("bad id"), json!(3)]),
        prop::sample::select(texts()),
        prop::sample::select(texts()),
        any::<bool>(),
    )
        .prop_map(|(sym, ty, mode, extra_key)| {
            let mut row = json!({"sym": sym, "name": "p", "type": ty, "mode": mode});
            if extra_key {
                row["other"] = json!(1);
            }
            row
        });
    prop_oneof![
        (-3i64..=70).prop_map(Value::from),
        any::<bool>().prop_map(Value::Bool),
        prop::sample::select(texts()).prop_map(Value::String),
        vec(row, 0..20).prop_map(Value::Array),
        Just(json!(1.5)),
        Just(json!({"a": 1})),
    ]
}

fn token() -> impl Strategy<Value = Token> {
    prop_oneof![
        Just(Token::Num("1".into())),
        Just(Token::Op("+".into())),
        Just(Token::Ref(SymbolId::new("s1").unwrap())),
    ]
}

fn leaf() -> impl Strategy<Value = Block> {
    (
        prop::sample::select(block_types()),
        0u32..=2,
        any::<bool>(),
        btree_map(prop::sample::select(extra_names()), extra_value(), 0..3),
        btree_map(prop::sample::select(field_names()), field_value(), 0..4),
        btree_map(
            prop::sample::select(input_names()),
            (vec(token(), 0..3), any::<bool>())
                .prop_map(|(expr, draft)| Input::Expr(ExprInput { expr, draft })),
            0..4,
        ),
    )
        .prop_map(|(block_type, v, disabled, extra, fields, inputs)| Block {
            id: BlockId::new("b").unwrap(),
            block_type,
            v,
            x: None,
            y: None,
            collapsed: false,
            disabled,
            comment: None,
            extra,
            fields,
            inputs,
            statements: BTreeMap::new(),
            stack: Vec::new(),
        })
}

fn block() -> impl Strategy<Value = Block> {
    leaf().prop_recursive(4, 32, 4, |inner| {
        (
            leaf(),
            btree_map(
                prop::sample::select(statement_names()),
                vec(inner.clone(), 0..3),
                0..3,
            ),
            btree_map(prop::sample::select(input_names()), inner, 0..2),
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

fn document() -> impl Strategy<Value = Document> {
    vec(block(), 0..5).prop_map(|blocks| Document {
        format: b2c_model::FORMAT_TAG.into(),
        format_version: 1,
        generator: Generator {
            app: "0".into(),
            catalog: "1".into(),
        },
        project: Project {
            id: ProjectId::new("p").unwrap(),
            name: "P".into(),
            description: String::new(),
            language: Language {
                standard: CppStandard::Cpp20,
                gnu_extensions: false,
            },
            options: b2c_model::ProjectOptions::default(),
            build: b2c_model::BuildSettings::default(),
            run: b2c_model::RunSettings::default(),
        },
        modules: vec![Module {
            id: ModuleId::new("m").unwrap(),
            name: "main".into(),
            workspace: Workspace {
                blocks,
                ..Workspace::default()
            },
        }],
        ext: None,
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn resolve_never_panics_and_is_idempotent(document in document()) {
        let (completed, diagnostics) = resolve(&document, core_catalog());
        let (again, diagnostics_again) = resolve(&completed, core_catalog());
        prop_assert_eq!(&again, &completed);
        prop_assert_eq!(diagnostics_again, diagnostics.clone());
        // Filling defaults only ever adds; nothing present is changed.
        prop_assert_eq!(document.modules[0].workspace.blocks.len(), completed.modules[0].workspace.blocks.len());
        for diagnostic in &diagnostics {
            prop_assert!(diagnostic.code.0.starts_with("B2C-E06"));
        }
    }

    #[test]
    fn a_clean_result_has_every_required_input(document in document()) {
        let (completed, diagnostics) = resolve(&document, core_catalog());
        if diagnostics.is_empty() {
            let mut stack: Vec<&Block> = completed.modules[0].workspace.blocks.iter().collect();
            while let Some(block) = stack.pop() {
                let def = &core_catalog().blocks[&block.block_type];
                for field in &def.field {
                    prop_assert!(block.fields.contains_key(&field.name));
                }
                for input in def.input.iter().filter(|i| i.repeat.is_none() && !i.optional) {
                    prop_assert!(block.inputs.contains_key(&input.name));
                }
                for input in block.inputs.values() {
                    if let Input::Block(nested) = input {
                        stack.push(&nested.block);
                    }
                }
                stack.extend(block.statements.values().flatten());
            }
        }
    }
}
