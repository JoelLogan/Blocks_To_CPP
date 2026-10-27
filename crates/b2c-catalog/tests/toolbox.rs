//! The toolbox (`catalog/toolbox.toml`, spec §3.11.1 and 04 §4.2): the
//! shipped one is valid, reaches every catalog block and has the presets the
//! editor relies on; broken toolboxes are rejected with a problem that names
//! the place; and every toolbox block resolves cleanly apart from what the
//! editor fills in when the block is dropped.

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

use b2c_catalog::{
    Category, DynamicCategory, FieldDefault, MAX_TOOLBOX_BYTES, Preset, PresetExtra, Shape, Toolbox,
    ToolboxError, ToolboxProblem, core_catalog, toolbox,
};
use common::project;
use serde_json::{Map, Value, json};

fn toolbox_text() -> String {
    std::fs::read_to_string(common::repo_root().join("catalog/toolbox.toml")).unwrap()
}

/// The shipped toolbox, parsed from disk but not checked.
fn parsed() -> Toolbox {
    Toolbox::parse(&toolbox_text()).unwrap()
}

/// The problems of the shipped toolbox after a change.
fn problems_after(change: impl FnOnce(&mut Toolbox)) -> Vec<ToolboxProblem> {
    let mut toolbox = parsed();
    change(&mut toolbox);
    toolbox.check(core_catalog())
}

/// The messages of the problems after a change.
fn messages_after(change: impl FnOnce(&mut Toolbox)) -> Vec<String> {
    problems_after(change)
        .into_iter()
        .map(|p| p.to_string())
        .collect()
}

fn category(toolbox: &mut Toolbox, id: Category) -> &mut b2c_catalog::ToolboxCategory {
    toolbox.categories.iter_mut().find(|c| c.id == id).unwrap()
}

/// A change to a toolbox, for tables of cases.
type Change = Box<dyn FnOnce(&mut Toolbox)>;

/// A token table such as `{ num = "0" }`.
fn token(kind: &str, text: &str) -> toml::Value {
    toml::Value::Table(
        [(kind.to_owned(), toml::Value::String(text.to_owned()))]
            .into_iter()
            .collect(),
    )
}

#[test]
fn the_shipped_toolbox_is_the_embedded_one_and_is_valid() {
    let loaded = Toolbox::load(&toolbox_text(), core_catalog()).unwrap();
    assert_eq!(&loaded, toolbox());
    assert!(std::ptr::eq(toolbox(), toolbox()), "the toolbox is cached");
}

#[test]
fn categories_follow_the_spec_order_with_names_and_icons() {
    let categories: Vec<(Category, &str, &str, &str, Option<DynamicCategory>)> = toolbox()
        .categories
        .iter()
        .map(|c| {
            (
                c.id,
                c.name.as_str(),
                c.icon.as_str(),
                c.colour.as_str(),
                c.dynamic,
            )
        })
        .collect();
    assert_eq!(
        categories,
        [
            (Category::Program, "Program", "▶", "program", None),
            (
                Category::Variables,
                "Variables",
                "𝑥",
                "variables",
                Some(DynamicCategory::Variables)
            ),
            (Category::Math, "Math", "∑", "math", None),
            (Category::Logic, "Logic", "◇", "logic", None),
            (Category::Text, "Text", "“ ”", "text", None),
            (Category::Control, "Control", "⑂", "control", None),
            (Category::Loops, "Loops", "↻", "loops", None),
            (Category::Io, "Input / Output", "⌨", "io", None),
            (
                Category::Functions,
                "Functions",
                "ƒ",
                "functions",
                Some(DynamicCategory::Functions)
            ),
            // Milestone M3's second wave (w2.md §2.1).
            (Category::Collections, "Collections", "☰", "collections", None),
            (Category::Types, "Types", "◆", "types", None),
            (Category::Files, "Files", "🗎", "files", None),
            (Category::Errors, "Errors", "⚠", "errors", None),
        ]
    );
}

#[test]
fn every_catalog_block_is_reachable_exactly_as_planned() {
    let catalog_ids: BTreeSet<&str> = core_catalog().blocks.keys().map(String::as_str).collect();
    assert_eq!(catalog_ids.len(), common::block_ids_on_disk().len());
    assert_eq!(toolbox().reachable_blocks(), catalog_ids);
    // Variable and call blocks come only through the dynamic categories.
    let static_ids: BTreeSet<&str> = toolbox()
        .categories
        .iter()
        .flat_map(|c| c.entries.iter().map(|e| e.block.as_str()))
        .collect();
    let dynamic: BTreeSet<&str> = catalog_ids.difference(&static_ids).copied().collect();
    let expected: BTreeSet<&str> = ["var.get", "var.set", "var.change", "func.call", "func.call_stmt"]
        .into_iter()
        .collect();
    assert_eq!(dynamic, expected);
    // Every entry sits in its block's own category.
    for category in &toolbox().categories {
        for entry in &category.entries {
            assert_eq!(
                core_catalog().blocks[&entry.block].category,
                category.id,
                "{}",
                entry.block
            );
        }
    }
}

#[test]
fn the_presets_the_editor_relies_on() {
    let entry = |block: &str, label: Option<&str>| {
        toolbox()
            .categories
            .iter()
            .flat_map(|c| &c.entries)
            .find(|e| e.block == block && e.label.as_deref() == label)
            .unwrap_or_else(|| panic!("no entry for {block} labelled {label:?}"))
    };
    let preset = |block: &str, label: Option<&str>| entry(block, label).preset.clone().unwrap();

    // *until* and *else* are chosen on the block (its menu and ⊕), not offered as entries.
    assert!(entry("control.while", None).preset.is_none());
    assert!(entry("control.if", None).preset.is_none());

    let declare = preset("var.declare", None);
    assert_eq!(declare.fields["TYPE"], FieldDefault::Text("int".into()));
    assert_eq!(declare.inputs["VALUE"], [token("num", "0")]);

    let ask = preset("io.ask", None);
    assert_eq!(ask.inputs["PROMPT"], [token("str", "Your answer: ")]);
}

/// One idea, one block, one toolbox entry (spec §3.1): no block has two
/// entries, and no entry has a label, since a label only told presets of
/// one block apart.
#[test]
fn every_block_has_one_entry_at_most() {
    let entries: Vec<&b2c_catalog::ToolboxEntry> =
        toolbox().categories.iter().flat_map(|c| &c.entries).collect();
    let blocks: BTreeSet<&str> = entries.iter().map(|e| e.block.as_str()).collect();
    assert_eq!(blocks.len(), entries.len());
    assert!(entries.iter().all(|e| e.label.is_none()), "{entries:?}");
}

#[test]
fn a_second_entry_for_a_block_is_rejected() {
    // The two preset entries the toolbox had before (spec §3.1): the loop
    // menu and the if block's ⊕ offer these forms in place.
    let repeat_until = |t: &mut Toolbox| {
        category(t, Category::Loops).entries.insert(
            2,
            b2c_catalog::ToolboxEntry {
                block: "control.while".into(),
                label: Some("repeat until".into()),
                preset: Some(Preset {
                    fields: [("MODE".to_owned(), FieldDefault::Text("until".into()))].into(),
                    ..Preset::default()
                }),
            },
        );
    };
    assert_eq!(
        messages_after(repeat_until),
        [
            "category loops, entry 3: the block control.while already has an entry (category loops, \
          entry 2): a block has one entry at most, and its other forms are chosen on the block \
          (spec §3.1)"
        ]
    );
    let if_else = |t: &mut Toolbox| {
        category(t, Category::Control)
            .entries
            .push(b2c_catalog::ToolboxEntry {
                block: "control.if".into(),
                label: Some("if … else".into()),
                preset: Some(Preset {
                    extra: [("hasElse".to_owned(), PresetExtra::Flag(true))].into(),
                    ..Preset::default()
                }),
            });
    };
    let problems = problems_after(if_else);
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert_eq!(
        (problems[0].category, problems[0].entry),
        // After *if* and *switch on*.
        (Some(Category::Control), Some(2))
    );
    assert!(
        problems[0].message.contains("already has an entry"),
        "{problems:?}"
    );
    // The checked toolbox leaves the second entry out and keeps the first.
    let mut text = toolbox_text();
    text.push_str("\n[[category.entry]]\nblock = \"io.print\"\n");
    let error = Toolbox::load(&text, core_catalog()).unwrap_err();
    assert!(
        error.to_string().contains("io.print already has an entry"),
        "{error}"
    );
}

#[test]
fn an_entry_label_must_not_be_a_category_name() {
    for name in ["Loops", "Program", "Input / Output"] {
        let messages = messages_after(|t| {
            category(t, Category::Math).entries[0].label = Some(name.into());
        });
        assert_eq!(
            messages,
            [format!(
                "category math, entry 1: the label {name:?} is the name of a category: use other words"
            )]
        );
    }
    // Other words, including a category name inside a longer label, are fine.
    for label in ["loops", "Loops and more", "Your functions"] {
        assert_eq!(
            messages_after(|t| category(t, Category::Math).entries[0].label = Some(label.into())),
            Vec::<String>::new(),
            "{label}"
        );
    }
}

#[test]
#[allow(clippy::too_many_lines, reason = "one table of cases")]
fn bad_presets_are_rejected() {
    let with_preset = |block: &'static str, preset: Preset| {
        move |toolbox: &mut Toolbox| {
            let entry = toolbox
                .categories
                .iter_mut()
                .flat_map(|c| c.entries.iter_mut())
                .find(|e| e.block == block)
                .unwrap();
            entry.preset = Some(preset);
        }
    };
    let fields = |pairs: &[(&str, FieldDefault)]| Preset {
        fields: pairs.iter().map(|(k, v)| ((*k).to_owned(), v.clone())).collect(),
        ..Preset::default()
    };
    let extra = |pairs: &[(&str, PresetExtra)]| Preset {
        extra: pairs.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect(),
        ..Preset::default()
    };
    let inputs = |pairs: Vec<(&str, Vec<toml::Value>)>| Preset {
        inputs: pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        ..Preset::default()
    };
    let text = |t: &str| FieldDefault::Text(t.into());
    let cases: Vec<(&str, Preset, &str)> = vec![
        (
            "control.while",
            fields(&[("MODE", text("sometimes"))]),
            "the field MODE does not fit",
        ),
        (
            "control.while",
            fields(&[("NOPE", text("x"))]),
            "has no field NOPE",
        ),
        (
            "control.while",
            fields(&[("MODE", FieldDefault::Bool(true))]),
            "does not fit",
        ),
        ("var.declare", fields(&[("TYPE", text("long"))]), "does not fit"),
        ("var.declare", fields(&[("CONST", text("yes"))]), "does not fit"),
        ("math.number", fields(&[("VALUE", text(""))]), "does not fit"),
        (
            "control.for_range",
            fields(&[("VAR", text("i"))]),
            "cannot be preset",
        ),
        (
            "control.if",
            extra(&[("nope", PresetExtra::Flag(true))]),
            "has no extra nope",
        ),
        (
            "control.if",
            extra(&[("hasElse", PresetExtra::Count(1))]),
            "must be true or false",
        ),
        (
            "control.if",
            extra(&[("elseIfCount", PresetExtra::Count(33))]),
            "from 0 to 32",
        ),
        (
            "control.if",
            extra(&[("elseIfCount", PresetExtra::Flag(true))]),
            "from 0 to 32",
        ),
        (
            "func.define",
            extra(&[("params", PresetExtra::Count(0))]),
            "cannot be preset",
        ),
        (
            "io.print",
            inputs(vec![("ITEM1", vec![token("num", "1")])]),
            "has no input ITEM1",
        ),
        (
            "io.print",
            inputs(vec![("NOPE", vec![token("num", "1")])]),
            "has no input NOPE",
        ),
        ("io.print", inputs(vec![("ITEM0", vec![])]), "has no tokens"),
        (
            "io.print",
            inputs(vec![("ITEM0", vec![token("ref", "sym_x")])]),
            "cannot be defaults",
        ),
        (
            "io.print",
            inputs(vec![("ITEM0", vec![token("chr", "ab")])]),
            "exactly one character",
        ),
        (
            "io.print",
            inputs(vec![("ITEM0", vec![token("str", "a\0b")])]),
            "the preset of the input ITEM0",
        ),
        (
            "io.print",
            inputs(vec![("ITEM0", vec![token("num", " ")])]),
            "needs digits",
        ),
        (
            "io.print",
            inputs(vec![("ITEM0", vec![token("num", "1"); 513])]),
            "more than 512 tokens",
        ),
        ("io.print", Preset::default(), "the preset sets nothing"),
    ];
    for (block, preset, expected) in cases {
        let messages = messages_after(with_preset(block, preset.clone()));
        assert_eq!(messages.len(), 1, "{block} {preset:?}: {messages:?}");
        assert!(messages[0].contains(expected), "{block} {preset:?}: {messages:?}");
        assert!(messages[0].starts_with("category "), "{messages:?}");
    }
    // The preset's count decides which numbered inputs exist.
    let more_items = Preset {
        extra: [("itemCount".to_owned(), PresetExtra::Count(2))].into(),
        inputs: [("ITEM1".to_owned(), vec![token("str", "x")])].into(),
        ..Preset::default()
    };
    assert_eq!(
        messages_after(with_preset("io.print", more_items)),
        Vec::<String>::new()
    );
    // control.if has one condition more than elseIfCount.
    let second_condition = inputs(vec![("COND1", vec![token("kw", "false")])]);
    assert_eq!(
        messages_after(with_preset("control.if", second_condition.clone())).len(),
        1
    );
    let mut with_branch = second_condition;
    with_branch
        .extra
        .insert("elseIfCount".into(), PresetExtra::Count(1));
    assert_eq!(
        messages_after(with_preset("control.if", with_branch)),
        Vec::<String>::new()
    );
}

#[test]
fn bad_entries_and_categories_are_rejected() {
    let first_entry_of = |id: Category, change: fn(&mut b2c_catalog::ToolboxEntry)| {
        move |toolbox: &mut Toolbox| change(&mut category(toolbox, id).entries[0])
    };
    // A changed copy of the first entry, added at the end (so every block
    // stays reachable).
    let added_to = |id: Category, change: fn(&mut b2c_catalog::ToolboxEntry)| {
        move |toolbox: &mut Toolbox| {
            let entries = &mut category(toolbox, id).entries;
            let mut entry = entries[0].clone();
            change(&mut entry);
            entries.push(entry);
        }
    };
    let cases: Vec<(Change, &str)> = vec![
        (
            // Logic, whose entries no W2 package adds to (w2.md §3.3).
            Box::new(added_to(Category::Logic, |e| e.block = "logic.nope".into())),
            "category logic, entry 5: the catalog has no block \"logic.nope\"",
        ),
        (
            Box::new(added_to(Category::Math, |e| e.block = "logic.not".into())),
            "belongs to the category logic",
        ),
        (
            Box::new(first_entry_of(Category::Math, |e| e.label = Some(" ".into()))),
            "the label is empty",
        ),
        (
            Box::new(first_entry_of(Category::Math, |e| e.label = Some(" x".into()))),
            "starts or ends with a space",
        ),
        (
            Box::new(first_entry_of(Category::Math, |e| {
                e.label = Some("a\u{202e}b".into());
            })),
            "control or invisible",
        ),
        (
            Box::new(first_entry_of(Category::Math, |e| e.label = Some("a\nb".into()))),
            "control or invisible",
        ),
        (
            Box::new(first_entry_of(Category::Math, |e| e.label = Some("x".repeat(65)))),
            "longer than 64",
        ),
        (
            Box::new(|t: &mut Toolbox| category(t, Category::Math).colour = "red".into()),
            "colour token must be the category ID",
        ),
        (
            Box::new(|t: &mut Toolbox| category(t, Category::Math).name = "M".repeat(33)),
            "longer than 32",
        ),
        (
            Box::new(|t: &mut Toolbox| category(t, Category::Math).icon = "12345".into()),
            "longer than 4",
        ),
        (
            Box::new(|t: &mut Toolbox| {
                category(t, Category::Math).dynamic = Some(DynamicCategory::Functions);
            }),
            "belong to the category functions",
        ),
        (
            Box::new(|t: &mut Toolbox| t.categories.swap(2, 3)),
            "must come before logic",
        ),
        (
            Box::new(|t: &mut Toolbox| {
                let copy = t.categories[2].clone();
                t.categories.insert(3, copy);
            }),
            "listed twice",
        ),
        (
            Box::new(|t: &mut Toolbox| {
                let copy = category(t, Category::Logic).entries[0].clone();
                category(t, Category::Logic).entries.push(copy);
            }),
            "category logic, entry 5: the block logic.operation already has an entry (category logic, \
             entry 1)",
        ),
    ];
    for (change, expected) in cases {
        let messages = messages_after(change);
        assert_eq!(messages.len(), 1, "{expected}: {messages:?}");
        assert!(messages[0].contains(expected), "{expected}: {messages:?}");
    }
    // A category without entries is valid (the editor does not show it;
    // w2.md §2.1), but removing the entries leaves their blocks
    // unreachable.
    let messages = messages_after(|t| category(t, Category::Logic).entries.clear());
    assert_eq!(
        messages,
        [
            "the block logic.boolean is not in the toolbox: add an entry for it",
            "the block logic.not is not in the toolbox: add an entry for it",
            "the block logic.operation is not in the toolbox: add an entry for it",
            "the block logic.ternary is not in the toolbox: add an entry for it",
        ]
    );
    // Without its dynamic kind, the Variables category no longer reaches
    // the variable blocks.
    let messages = messages_after(|t| category(t, Category::Variables).dynamic = None);
    assert_eq!(messages.len(), 3, "{messages:?}");
    // An empty toolbox.
    let messages = messages_after(|t| t.categories.clear());
    assert_eq!(messages[0], "the toolbox has no categories");
    // Then every block of the catalog, which none of them reaches.
    assert_eq!(messages.len(), 1 + core_catalog().blocks.len());
}

#[test]
fn malformed_files_are_rejected_before_checking() {
    for (text, expected) in [
        (
            "[[category]]\nid = \"program\"\nname = \"P\"\nicon = \"P\"\ncolour = \"program\"\nshade = 1\n",
            "unknown field `shade`",
        ),
        ("[[category]]\nid = \"graphics\"\n", "unknown variant `graphics`"),
        (
            "[[category]]\nid = \"program\"\nname = \"P\"\nicon = \"P\"\ncolour = \"program\"\ndynamic = \"lists\"\n",
            "unknown variant `lists`",
        ),
        (
            "[[category]]\nid = \"program\"\nname = \"P\"\nicon = \"P\"\ncolour = \"program\"\n[[category.entry]]\nblock = \"control.if\"\npreset = { extra = { elseIfCount = -1 } }\n",
            "did not match any variant",
        ),
        (
            "[[category]]\nid = \"program\"\nname = \"P\"\nicon = \"P\"\ncolour = \"program\"\n[[category.entry]]\nblock = \"math.number\"\npreset = { fields = { VALUE = 5 } }\n",
            "did not match any variant",
        ),
        ("not toml [", "the toolbox file is not valid"),
    ] {
        let error = Toolbox::parse(text).unwrap_err();
        assert!(matches!(error, ToolboxError::Syntax(_)), "{error:?}");
        assert!(error.to_string().contains(expected), "{expected}: {error}");
        assert!(matches!(
            Toolbox::load(text, core_catalog()),
            Err(ToolboxError::Syntax(_))
        ));
    }
    let huge = format!("# {}\n", "x".repeat(MAX_TOOLBOX_BYTES));
    assert_eq!(
        Toolbox::parse(&huge),
        Err(ToolboxError::TooLarge { size: huge.len() })
    );
    // A file that parses but does not fit the catalog.
    let error = Toolbox::load("", core_catalog()).unwrap_err();
    let ToolboxError::Invalid(problems) = &error else {
        panic!("{error:?}");
    };
    assert_eq!(problems.len(), 1 + core_catalog().blocks.len());
    assert!(
        error
            .to_string()
            .starts_with("the toolbox does not fit the catalog: the toolbox has no categories; the block")
    );
}

/// A project block for a toolbox entry: its preset, nothing else (the
/// catalog defaults are filled in by resolve).
fn entry_block(id: String, block: &str, preset: Option<&Preset>) -> Value {
    let mut fields = Map::new();
    let mut extra = Map::new();
    let mut inputs = Map::new();
    if let Some(preset) = preset {
        for (name, value) in &preset.fields {
            fields.insert(name.clone(), serde_json::to_value(value).unwrap());
        }
        for (name, value) in &preset.extra {
            extra.insert(name.clone(), serde_json::to_value(value).unwrap());
        }
        for (name, tokens) in &preset.inputs {
            inputs.insert(
                name.clone(),
                json!({ "expr": serde_json::to_value(tokens).unwrap() }),
            );
        }
    }
    json!({"id": id, "type": block, "v": 1, "fields": fields, "extra": extra, "inputs": inputs})
}

/// Every toolbox entry, dropped where its shape belongs, resolves with only
/// the problem the editor fixes when the block is dropped: the parameter
/// list of a new function. A name or variable nobody has chosen yet (a new
/// variable's or function's name, the variable an `ask` stores into) is
/// absent, which is no catalog problem: the analyser reports it once the
/// block is part of the program (ADR-0012).
#[test]
fn every_toolbox_block_resolves_apart_from_what_the_editor_fills_in() {
    let mut body = Vec::new();
    let mut top = Vec::new();
    let mut values = Vec::new();
    let mut types = BTreeMap::new();
    for (c, category) in toolbox().categories.iter().enumerate() {
        for (e, entry) in category.entries.iter().enumerate() {
            let id = format!("t{c}_{e}");
            types.insert(id.clone(), entry.block.clone());
            let block = entry_block(id, &entry.block, entry.preset.as_ref());
            match core_catalog().blocks[&entry.block].shape {
                Shape::Hat | Shape::Definition => top.push(block),
                Shape::Statement => body.push(block),
                Shape::Reporter | Shape::Predicate => values.push(block),
            }
        }
    }
    body.extend(common::prints("b_values", values));
    let (_, diagnostics) = common::resolve(&project(Value::Array(body), Value::Array(top)));
    let found: BTreeSet<(String, String, String)> = diagnostics
        .iter()
        .map(|d| {
            let block = d.primary.block.as_ref().unwrap().as_str();
            let part = match &d.primary.part {
                b2c_ir::Part::Field { name } => name.clone(),
                other => format!("{other:?}"),
            };
            (types[block].clone(), d.code.0.clone(), part)
        })
        .collect();
    let mut expected: BTreeSet<(String, String, String)> = [("func.define", "B2C-E0612", "Whole")]
        .into_iter()
        .map(|(a, b, c)| (a.to_owned(), b.to_owned(), c.to_owned()))
        .collect();
    // An input typed by another part (`elem(C)`, `index(C)`) has no catalog
    // default: the editor writes its start value once that part is chosen,
    // which in the toolbox it is not (types.md §4.3, slots.md §6).
    for entry in toolbox().categories.iter().flat_map(|category| &category.entries) {
        let def = &core_catalog().blocks[&entry.block];
        for input in &def.input {
            if !input.default.is_empty() || input.optional || input.pattern.is_none() {
                continue;
            }
            let parts = match &input.repeat {
                None => vec![input.name.clone()],
                Some(repeat) => {
                    let count = def
                        .extra
                        .iter()
                        .find(|extra| extra.name == repeat.count)
                        .and_then(|extra| match &extra.default {
                            Some(b2c_catalog::FieldDefault::Text(text)) => text.parse().ok(),
                            _ => None,
                        })
                        .unwrap_or(0);
                    (0..count).map(|i| format!("{}{i}", input.name)).collect()
                }
            };
            for name in parts {
                let part = format!("{:?}", b2c_ir::Part::Input { name });
                expected.insert((def.id.clone(), String::from("B2C-E0609"), part));
            }
        }
    }
    assert_eq!(found, expected, "{diagnostics:#?}");
    // The second program.main (from the toolbox) is the analyser's business,
    // not the catalog's.
    assert_eq!(diagnostics.len(), expected.len());
}
