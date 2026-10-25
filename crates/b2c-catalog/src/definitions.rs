//! Loading catalog TOML files and checking the definitions themselves
//! (spec §3.11.1): a broken definition is a bug in the catalog, found here
//! instead of as confusing errors on users' blocks.

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::sig::TypePattern;
use b2c_ir::types::Type;
use b2c_model::Token;
use b2c_model::limits::MAX_VARIADIC_PARTS;

use crate::schema::{
    BlockDef, CatalogFile, ExtraDef, ExtraKind, FieldDef, FieldDefault, FieldKind, InputDef, Shape,
    is_part_name,
};
use crate::{CATALOG_VERSION, Catalog, signature};

/// The built-in catalog files, embedded at compile time. Milestone M3's
/// second wave adds `collections`, `errors`, `files` and `types`, empty
/// until its packages fill them (w2.md §3.1).
pub(crate) const CORE_FILES: [(&str, &str); 12] = [
    (
        "collections.toml",
        include_str!("../../../catalog/core/collections.toml"),
    ),
    ("control.toml", include_str!("../../../catalog/core/control.toml")),
    ("errors.toml", include_str!("../../../catalog/core/errors.toml")),
    ("files.toml", include_str!("../../../catalog/core/files.toml")),
    (
        "functions.toml",
        include_str!("../../../catalog/core/functions.toml"),
    ),
    ("io.toml", include_str!("../../../catalog/core/io.toml")),
    ("logic.toml", include_str!("../../../catalog/core/logic.toml")),
    ("math.toml", include_str!("../../../catalog/core/math.toml")),
    ("program.toml", include_str!("../../../catalog/core/program.toml")),
    ("text.toml", include_str!("../../../catalog/core/text.toml")),
    ("types.toml", include_str!("../../../catalog/core/types.toml")),
    (
        "variables.toml",
        include_str!("../../../catalog/core/variables.toml"),
    ),
];

/// The parameter pass modes a `params` row may use.
pub(crate) const PARAM_MODES: [&str; 3] = ["copy", "editable", "read_only"];

/// A problem with the catalog itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Problem {
    /// The block definition, when the problem is about one.
    pub(crate) block: Option<String>,
    /// What is wrong.
    pub(crate) message: String,
}

/// Parses catalog files into a catalog. Files that do not parse and blocks
/// whose ID is already taken are left out and reported.
pub(crate) fn build(files: &[(&str, &str)]) -> (Catalog, Vec<Problem>) {
    let mut blocks = BTreeMap::new();
    let mut problems = Vec::new();
    for (name, text) in files {
        match toml::from_str::<CatalogFile>(text) {
            Ok(file) => {
                for def in file.block {
                    if blocks.contains_key(&def.id) {
                        problems.push(Problem {
                            block: Some(def.id.clone()),
                            message: format!("the ID is defined more than once (again in {name})"),
                        });
                    } else {
                        blocks.insert(def.id.clone(), def);
                    }
                }
            }
            Err(error) => problems.push(Problem {
                block: None,
                message: format!("{name} is not a valid catalog file: {error}"),
            }),
        }
    }
    let catalog = Catalog::new(CATALOG_VERSION, blocks);
    problems.extend(check_catalog(&catalog));
    (catalog, problems)
}

/// Checks every definition of a catalog.
pub(crate) fn check_catalog(catalog: &Catalog) -> Vec<Problem> {
    let mut problems = Vec::new();
    for (key, def) in &catalog.blocks {
        if key != &def.id {
            problems.push(Problem {
                block: Some(key.clone()),
                message: format!("it is stored under the key {key:?} but its ID is {:?}", def.id),
            });
        }
        problems.extend(check_block(def).into_iter().map(|message| Problem {
            block: Some(def.id.clone()),
            message,
        }));
    }
    problems
}

/// `^[a-z]+(\.[a-z_]+)+$`.
pub(crate) fn is_block_id(id: &str) -> bool {
    let mut parts = id.split('.');
    let first_ok = parts
        .next()
        .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase()));
    let mut rest = parts.peekable();
    first_ok
        && rest.peek().is_some()
        && rest.all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
}

/// `extra` key names: `^[a-z][A-Za-z0-9]*$`.
fn is_extra_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|b| b.is_ascii_lowercase()) && bytes.all(|b| b.is_ascii_alphanumeric())
}

/// The index of a repeated part: `ITEM12` with base `ITEM` is 12. Indexes are
/// written without leading zeros, so `ITEM01` is not a part of `ITEM`.
pub(crate) fn repeat_index(name: &str, base: &str) -> Option<u32> {
    let digits = name.strip_prefix(base)?;
    let canonical = !digits.is_empty()
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'));
    if canonical { digits.parse().ok() } else { None }
}

/// The definition of a block's field part: a field by its name, or a part
/// of a repeated field by its numbered name (`ERROR2`). The part's index is
/// not checked against the block's count.
pub(crate) fn field_def<'d>(def: &'d BlockDef, name: &str) -> Option<&'d FieldDef> {
    def.field.iter().find(|field| match field.repeat {
        None => field.name == name,
        Some(_) => repeat_index(name, &field.name).is_some(),
    })
}

/// Converts a catalog default token (`{ num = "0" }`) to a project token.
/// Only literal and operator tokens make sense as defaults: a default cannot
/// refer to a symbol, and draft text is never a default.
pub(crate) fn default_token(value: &toml::Value) -> Result<Token, String> {
    let toml::Value::Table(table) = value else {
        return Err(format!(
            "a default token must be a table such as {{ num = \"0\" }}, not a {}",
            value.type_str()
        ));
    };
    let mut entries = table.iter();
    let (Some((kind, text)), None) = (entries.next(), entries.next()) else {
        return Err(String::from("a default token must have exactly one key"));
    };
    let toml::Value::String(text) = text else {
        return Err(format!("the value of a {kind:?} token must be a string"));
    };
    let text = text.clone();
    match kind.as_str() {
        "num" => Ok(Token::Num(text)),
        "str" => Ok(Token::Str(text)),
        "chr" => Ok(Token::Chr(text)),
        "op" => Ok(Token::Op(text)),
        "kw" if text == "true" || text == "false" => Ok(Token::Kw(text)),
        "kw" => Err(format!("{text:?} is not a keyword literal (true or false)")),
        _ => Err(format!(
            "{kind:?} tokens cannot be defaults (use num, str, chr, op or kw)"
        )),
    }
}

/// The default tokens of an input.
pub(crate) fn default_tokens(input: &InputDef) -> Result<Vec<Token>, String> {
    input.default.iter().map(default_token).collect()
}

/// Every problem with one definition.
pub(crate) fn check_block(def: &BlockDef) -> Vec<String> {
    let mut problems = Vec::new();
    if !is_block_id(&def.id) {
        problems.push(format!(
            "the ID {:?} does not match category.name (lower-case letters and _)",
            def.id
        ));
    }
    if def.version == 0 {
        problems.push(String::from("versions start at 1"));
    }
    if def.label.friendly.trim().is_empty() || def.label.cpp.trim().is_empty() {
        problems.push(String::from("both labels must be non-empty"));
    }
    if def.help.trim().is_empty() {
        problems.push(String::from("the help text is empty"));
    }
    let extras = check_extras(def, &mut problems);
    check_names(def, &mut problems);
    for field in &def.field {
        check_field(field, &mut problems);
        if let Some(repeat) = &field.repeat {
            check_repeat(&field.name, &repeat.count, repeat.plus, &extras, &mut problems);
            if !matches!(
                field.kind,
                FieldKind::Dropdown | FieldKind::Checkbox | FieldKind::Type | FieldKind::SymbolDecl
            ) {
                problems.push(format!(
                    "the field {} repeats, but only dropdown, checkbox, type and symbol_decl fields can",
                    field.name
                ));
            }
        }
    }
    for input in &def.input {
        if let Some(repeat) = &input.repeat {
            check_repeat(&input.name, &repeat.count, repeat.plus, &extras, &mut problems);
        }
        if let Err(error) = default_tokens(input) {
            problems.push(format!("input {}: {error}", input.name));
        }
        if input.optional && !input.default.is_empty() {
            problems.push(format!(
                "input {} is optional, so it cannot also have a default",
                input.name
            ));
        }
    }
    for statement in &def.statement {
        if let Some(repeat) = &statement.repeat {
            check_repeat(
                &statement.name,
                &repeat.count,
                repeat.plus,
                &extras,
                &mut problems,
            );
        }
        if let Some(flag) = &statement.when {
            if extras.get(flag.as_str()) != Some(&ExtraKind::Flag) {
                problems.push(format!(
                    "statement {} is shown when {flag:?}, which is not a flag in extra",
                    statement.name
                ));
            }
            if statement.repeat.is_some() {
                problems.push(format!(
                    "statement {} cannot both repeat and depend on a flag",
                    statement.name
                ));
            }
        }
    }
    if def.shape.is_value() && !def.statement.is_empty() {
        problems.push(String::from(
            "reporter and predicate blocks cannot have statement inputs",
        ));
    }
    check_output(def, &mut problems);
    check_labels(def, &mut problems);
    check_friendly_label(def, &mut problems);
    problems.extend(signature::problems(def));
    problems
}

/// Reporters and predicates declare the type of their value; other shapes
/// give no value, and a predicate gives `bool`. The forms of the output
/// pattern (`symbol`, `field:NAME`, its variables) are checked with the
/// other patterns ([`signature::problems`]).
fn check_output(def: &BlockDef, problems: &mut Vec<String>) {
    let Some(output) = &def.output else {
        if def.shape.is_value() {
            problems.push(String::from("reporter and predicate blocks need an output type"));
        }
        return;
    };
    if !def.shape.is_value() {
        problems.push(format!(
            "only reporter and predicate blocks give a value, so this block cannot have the output type {output}"
        ));
        return;
    }
    if def.shape == Shape::Predicate && *output != TypePattern::Type(Type::Bool) {
        problems.push(format!(
            "a predicate gives bool, so its output type cannot be {output}"
        ));
    }
}

/// Checks `extra` definitions; returns their kinds by name.
fn check_extras<'a>(def: &'a BlockDef, problems: &mut Vec<String>) -> BTreeMap<&'a str, ExtraKind> {
    let mut kinds = BTreeMap::new();
    let limit = u32::try_from(MAX_VARIADIC_PARTS).unwrap_or(u32::MAX);
    for extra in &def.extra {
        if !is_extra_name(&extra.name) {
            problems.push(format!("the extra name {:?} is not camelCase", extra.name));
        }
        if kinds.insert(extra.name.as_str(), extra.kind).is_some() {
            problems.push(format!("the extra {:?} is defined twice", extra.name));
        }
        if !extra.kind.has_row_types() && !extra.types.is_empty() {
            problems.push(format!(
                "only params and fields extras have types ({})",
                extra.name
            ));
        }
        if let Some(name) = extra.kind.row_name()
            && extra.name != name
        {
            problems.push(format!(
                "an extra of kind {name} must be called {name:?}, not {:?}",
                extra.name
            ));
        }
        match extra.kind {
            ExtraKind::Count => {
                if extra.min > extra.max || extra.max > limit {
                    problems.push(format!(
                        "the count {} must have min ≤ max ≤ {MAX_VARIADIC_PARTS}",
                        extra.name
                    ));
                }
                if let Some(default) = &extra.default
                    && count_default(extra, default).is_none()
                {
                    problems.push(format!(
                        "the default of the count {} must be a whole number from {} to {}, written as text",
                        extra.name, extra.min, extra.max
                    ));
                }
            }
            ExtraKind::Flag => {
                if matches!(extra.default, Some(FieldDefault::Text(_))) {
                    problems.push(format!(
                        "the default of the flag {} must be true or false",
                        extra.name
                    ));
                }
            }
            ExtraKind::Params | ExtraKind::Fields | ExtraKind::Values => {
                let kind = extra.kind.row_name().unwrap_or_default();
                if extra.max == 0 || extra.max > limit {
                    problems.push(format!(
                        "{kind} {} must allow 1 to {MAX_VARIADIC_PARTS} rows",
                        extra.name
                    ));
                }
                if extra.kind.has_row_types() && (extra.types.is_empty() || !all_unique(&extra.types)) {
                    problems.push(format!("{kind} {} need a list of distinct types", extra.name));
                }
                if extra.default.is_some() {
                    problems.push(format!("{kind} {} cannot have a default", extra.name));
                }
            }
        }
    }
    kinds
}

/// A count's default, when it is valid.
pub(crate) fn count_default(extra: &ExtraDef, default: &FieldDefault) -> Option<u32> {
    let FieldDefault::Text(text) = default else {
        return None;
    };
    text.parse::<u32>()
        .ok()
        .filter(|n| (extra.min..=extra.max).contains(n))
}

/// A repeated part's count: a `count` extra, or a row extra whose rows it
/// counts (`repeat = { count = "params" }`; w2.md §2.1).
fn check_repeat(
    name: &str,
    count: &str,
    plus: u32,
    extras: &BTreeMap<&str, ExtraKind>,
    problems: &mut Vec<String>,
) {
    if !extras
        .get(count)
        .is_some_and(|kind| *kind == ExtraKind::Count || kind.is_rows())
    {
        problems.push(format!(
            "{name} repeats by {count:?}, which is not a count or a row extra (params, fields, values) in extra"
        ));
    }
    if plus > 1 {
        problems.push(format!(
            "{name} adds {plus} to its count; only 0 or 1 is supported"
        ));
    }
}

/// Field, input and statement names share one namespace (labels refer to
/// them all as `%NAME`), including the numbered names of repeated parts.
fn check_names(def: &BlockDef, problems: &mut Vec<String>) {
    let parts: Vec<(&str, bool)> = def
        .field
        .iter()
        .map(|f| (f.name.as_str(), f.repeat.is_some()))
        .chain(def.input.iter().map(|i| (i.name.as_str(), i.repeat.is_some())))
        .chain(
            def.statement
                .iter()
                .map(|s| (s.name.as_str(), s.repeat.is_some())),
        )
        .collect();
    let mut seen = BTreeSet::new();
    for (name, repeated) in &parts {
        if !is_part_name(name) {
            problems.push(format!("the name {name:?} is not UPPER_CASE"));
        }
        if !seen.insert(*name) {
            problems.push(format!("the name {name} is used twice"));
        }
        if *repeated && name.ends_with(|c: char| c.is_ascii_digit()) {
            problems.push(format!("the repeated name {name} must not end with a digit"));
        }
    }
    for (base, repeated) in &parts {
        if !repeated {
            continue;
        }
        for (other, _) in &parts {
            if other != base && repeat_index(other, base).is_some() {
                problems.push(format!(
                    "the name {other} collides with the repeated parts of {base}"
                ));
            }
        }
    }
}

fn check_field(field: &FieldDef, problems: &mut Vec<String>) {
    let name = &field.name;
    if field.kind != FieldKind::Dropdown && !field.options.is_empty() {
        problems.push(format!("only dropdown fields have options ({name})"));
    }
    if field.kind != FieldKind::Type && !field.types.is_empty() {
        problems.push(format!("only type fields have types ({name})"));
    }
    match field.kind {
        FieldKind::Dropdown => {
            let values: Vec<&String> = field.options.iter().map(|[_, value]| value).collect();
            let labels_ok = field
                .options
                .iter()
                .all(|[label, value]| !label.is_empty() && !value.is_empty());
            if values.is_empty() || !labels_ok || !all_unique(&values) {
                problems.push(format!(
                    "the dropdown {name} needs options with distinct, non-empty values"
                ));
            }
        }
        // An empty list is for a type field filled only from the module
        // (`struct`), which needs a pattern: the signature checks that no M2
        // type fits it (w2.md §2.1).
        FieldKind::Type
            if (field.types.is_empty() && field.pattern.is_none()) || !all_unique(&field.types) =>
        {
            problems.push(format!("the type field {name} needs a list of distinct types"));
        }
        _ => {}
    }
    if !field.default.as_ref().is_none_or(|d| field_value_fits(field, d)) {
        problems.push(format!("the default of {name} does not fit its kind or choices"));
    }
}

/// Whether a value written in the catalog (a default, or a toolbox preset)
/// fits a field: a dropdown's option value, a checkbox's `true` or `false`,
/// text, non-empty number text, one of a type field's types, or a header of
/// the header table. Symbol fields take no such value: the editor fills
/// them in.
pub(crate) fn field_value_fits(field: &FieldDef, value: &FieldDefault) -> bool {
    match (field.kind, value) {
        (FieldKind::Dropdown, FieldDefault::Text(text)) => {
            field.options.iter().any(|[_, option]| option == text)
        }
        (FieldKind::Checkbox, FieldDefault::Bool(_)) | (FieldKind::Text, FieldDefault::Text(_)) => true,
        (FieldKind::Number, FieldDefault::Text(text)) => !text.is_empty(),
        (FieldKind::Type, FieldDefault::Text(text)) => field.types.contains(text),
        (FieldKind::Header, FieldDefault::Text(text)) => signature::header_fits(text),
        _ => false,
    }
}

/// Every `%NAME` in a label must name a field, input or statement.
fn check_labels(def: &BlockDef, problems: &mut Vec<String>) {
    let names: BTreeSet<&str> = def
        .field
        .iter()
        .map(|f| f.name.as_str())
        .chain(def.input.iter().map(|i| i.name.as_str()))
        .chain(def.statement.iter().map(|s| s.name.as_str()))
        .collect();
    for label in [&def.label.friendly, &def.label.cpp] {
        for piece in label.split('%').skip(1) {
            let name: String = piece
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '_')
                .collect();
            if !name.is_empty() && !names.contains(name.as_str()) {
                problems.push(format!(
                    "the label {label:?} refers to %{name}, which the block does not have"
                ));
            }
        }
    }
}

/// One piece of a label template: text, or a `%NAME` placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LabelPiece<'a> {
    /// Text between placeholders (with its spaces, and any `…`).
    Text(&'a str),
    /// The name of a `%NAME` placeholder.
    Arg(&'a str),
}

/// Splits a label template into text and `%NAME` pieces, with the
/// grammar of the editor's label parser (`packages/catalog-gen`): a `%`
/// followed by `[A-Z][A-Z0-9_]*` is a placeholder, any other `%` is text.
fn label_pieces(label: &str) -> Vec<LabelPiece<'_>> {
    let mut pieces = Vec::new();
    let mut text_start = 0;
    let mut index = 0;
    let bytes = label.as_bytes();
    while index < bytes.len() {
        let starts_name = bytes[index] == b'%' && bytes.get(index + 1).is_some_and(u8::is_ascii_uppercase);
        if !starts_name {
            index += 1;
            continue;
        }
        let name_start = index + 1;
        let name_end = bytes[name_start..]
            .iter()
            .position(|b| !(b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_'))
            .map_or(bytes.len(), |offset| name_start + offset);
        if text_start < index {
            pieces.push(LabelPiece::Text(&label[text_start..index]));
        }
        pieces.push(LabelPiece::Arg(&label[name_start..name_end]));
        text_start = name_end;
        index = name_end;
    }
    if text_start < label.len() {
        pieces.push(LabelPiece::Text(&label[text_start..]));
    }
    pieces
}

/// The friendly label is all a block shows, so it must name every part a
/// user sets on the block (spec §3.11.1): every field and every value input
/// that is not repeated (repeated parts come with the block's ⊕/⊖ parts)
/// appears in it as `%NAME`, and a checkbox stands next to words that say
/// what ticking it means (`new line %NEWLINE`), because a bare ☑ explains
/// nothing.
fn check_friendly_label(def: &BlockDef, problems: &mut Vec<String>) {
    let pieces = label_pieces(&def.label.friendly);
    let shown: BTreeSet<&str> = pieces
        .iter()
        .filter_map(|piece| match piece {
            LabelPiece::Arg(name) => Some(*name),
            LabelPiece::Text(_) => None,
        })
        .collect();
    let parts = def
        .field
        .iter()
        .filter(|field| field.repeat.is_none())
        .map(|field| ("field", field.name.as_str()))
        .chain(
            def.input
                .iter()
                .filter(|input| input.repeat.is_none())
                .map(|input| ("input", input.name.as_str())),
        );
    for (what, name) in parts {
        if !shown.contains(name) {
            problems.push(format!(
                "the friendly label does not show the {what} {name}: add %{name} where it belongs"
            ));
        }
    }
    let has_words = |piece: Option<&LabelPiece<'_>>| matches!(piece, Some(LabelPiece::Text(text)) if text.chars().any(char::is_alphabetic));
    for field in def.field.iter().filter(|field| field.kind == FieldKind::Checkbox) {
        let name = field.name.as_str();
        for (index, piece) in pieces.iter().enumerate() {
            if *piece != LabelPiece::Arg(name) {
                continue;
            }
            let before = index.checked_sub(1).and_then(|i| pieces.get(i));
            if !has_words(before) && !has_words(pieces.get(index + 1)) {
                problems.push(format!(
                    "the checkbox {name} has no words next to it in the friendly label: say what \
                     ticking it means, as in \"new line %NEWLINE\""
                ));
            }
        }
    }
}

fn all_unique<T: Ord>(items: &[T]) -> bool {
    let set: BTreeSet<&T> = items.iter().collect();
    set.len() == items.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Category, Label, Lowering, Repeat, Shape, StatementDef, TypeClass};

    fn def() -> BlockDef {
        BlockDef {
            id: "test.block".into(),
            version: 1,
            category: Category::Control,
            shape: Shape::Statement,
            output: None,
            type_vars: BTreeMap::new(),
            label: Label {
                friendly: "do %X with %ITEM".into(),
                cpp: "%X(%ITEM)".into(),
            },
            lowering: Lowering::Builtin,
            headers: vec!["<iostream>".into()],
            help: "Help.".into(),
            field: vec![FieldDef {
                name: "X".into(),
                kind: FieldKind::Dropdown,
                options: vec![["a".into(), "a".into()], ["b".into(), "b".into()]],
                types: vec![],
                default: Some(FieldDefault::Text("a".into())),
                pattern: None,
                members: None,
                headers: BTreeMap::new(),
                repeat: None,
            }],
            input: vec![InputDef {
                name: "ITEM".into(),
                check: TypeClass::Any,
                optional: false,
                repeat: Some(Repeat {
                    count: "itemCount".into(),
                    plus: 0,
                }),
                default: vec![toml::Value::Table(
                    [("num".to_owned(), toml::Value::String("1".into()))]
                        .into_iter()
                        .collect(),
                )],
                pattern: None,
                assign: false,
            }],
            statement: vec![StatementDef {
                name: "BODY".into(),
                repeat: None,
                when: Some("open".into()),
            }],
            extra: vec![
                ExtraDef {
                    name: "itemCount".into(),
                    kind: ExtraKind::Count,
                    min: 1,
                    max: 8,
                    default: Some(FieldDefault::Text("1".into())),
                    types: vec![],
                    type_class: None,
                },
                ExtraDef {
                    name: "open".into(),
                    kind: ExtraKind::Flag,
                    min: 0,
                    max: 1,
                    default: Some(FieldDefault::Bool(true)),
                    types: vec![],
                    type_class: None,
                },
            ],
        }
    }

    fn problems_after(change: impl FnOnce(&mut BlockDef)) -> Vec<String> {
        let mut block = def();
        change(&mut block);
        check_block(&block)
    }

    #[test]
    fn the_core_catalog_is_valid_and_complete() {
        let (catalog, problems) = build(&CORE_FILES);
        assert_eq!(problems, []);
        // Every `[[block]]` of every file is in the catalog (M2 has 31), so
        // the packages that fill W2's files never edit this test.
        let tables: usize = CORE_FILES
            .iter()
            .map(|(_, text)| text.lines().filter(|line| *line == "[[block]]").count())
            .sum();
        assert!(tables >= 31);
        assert_eq!(catalog.blocks.len(), tables);
        assert_eq!(catalog.version, CATALOG_VERSION);
        let names: Vec<&str> = CORE_FILES.iter().map(|(name, _)| *name).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted, "the files are listed by name");
    }

    #[test]
    fn the_test_definition_is_valid() {
        assert_eq!(check_block(&def()), Vec::<String>::new());
    }

    #[test]
    fn labels_split_into_text_and_placeholders() {
        use LabelPiece::{Arg, Text};
        assert_eq!(
            label_pieces("print %ITEM … %SEP new line %NEWLINE to %STREAM"),
            [
                Text("print "),
                Arg("ITEM"),
                Text(" … "),
                Arg("SEP"),
                Text(" new line "),
                Arg("NEWLINE"),
                Text(" to "),
                Arg("STREAM"),
            ]
        );
        assert_eq!(label_pieces("%A%B_2 x"), [Arg("A"), Arg("B_2"), Text(" x")]);
        // A % that does not start an upper-case name is text, as in the editor's parser.
        assert_eq!(
            label_pieces("100% of %x and %_Y %"),
            [Text("100% of %x and %_Y %")]
        );
        assert_eq!(label_pieces("'%VALUE'"), [Text("'"), Arg("VALUE"), Text("'")]);
        assert_eq!(label_pieces(""), []);
    }

    #[test]
    fn friendly_labels_show_every_field_and_fixed_input() {
        // The test definition shows X; its ITEM is repeated, so ⊕/⊖ show it.
        assert_eq!(
            problems_after(|d| d.label.friendly = "do it with %ITEM".into()),
            ["the friendly label does not show the field X: add %X where it belongs"]
        );
        let fixed_item = |d: &mut BlockDef| {
            d.input[0].repeat = None;
            d.extra.remove(0);
        };
        assert_eq!(problems_after(fixed_item), Vec::<String>::new());
        assert_eq!(
            problems_after(|d| {
                fixed_item(d);
                d.label.friendly = "do %X".into();
            }),
            ["the friendly label does not show the input ITEM: add %ITEM where it belongs"]
        );
        // The C++ label may leave parts out, and statement inputs may be left
        // out of both (they go after the label).
        assert_eq!(
            problems_after(|d| d.label.cpp = "f();".into()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_checkbox_needs_words_next_to_it() {
        let checkbox = |d: &mut BlockDef, label: &str| {
            d.field[0] = FieldDef {
                name: "X".into(),
                kind: FieldKind::Checkbox,
                options: vec![],
                types: vec![],
                default: Some(FieldDefault::Bool(true)),
                pattern: None,
                members: None,
                headers: BTreeMap::new(),
                repeat: None,
            };
            d.label.friendly = label.into();
        };
        for good in [
            "print %ITEM … new line %X",
            "%X new line %ITEM",
            "%ITEM … constant %X",
            "do %X",
            "%X once",
        ] {
            assert_eq!(
                problems_after(|d| checkbox(d, good)),
                Vec::<String>::new(),
                "{good}"
            );
        }
        let expected = "the checkbox X has no words next to it in the friendly label: say what \
                        ticking it means, as in \"new line %NEWLINE\"";
        for bad in ["%ITEM … %X", "%X %ITEM", "%X", "%ITEM = %X …", "'%X'"] {
            assert_eq!(problems_after(|d| checkbox(d, bad)), [expected], "{bad}");
        }
        // A dropdown or a type field needs no words: its choices say it.
        assert_eq!(
            problems_after(|d| d.label.friendly = "%X %ITEM".into()),
            Vec::<String>::new()
        );
    }

    #[test]
    fn ids_and_names() {
        for good in ["io.print", "a.b", "std.algorithm.reverse", "x.long_name"] {
            assert!(is_block_id(good), "{good}");
        }
        for bad in [
            "",
            "io",
            "io.",
            ".print",
            "IO.print",
            "io.print2",
            "io..print",
            "io.pr-int",
        ] {
            assert!(!is_block_id(bad), "{bad}");
        }
        assert!(!problems_after(|d| d.id = "Bad".into()).is_empty());
        assert!(!problems_after(|d| d.version = 0).is_empty());
        assert!(!problems_after(|d| d.field[0].name = "x".into()).is_empty());
        assert!(!problems_after(|d| d.extra[0].name = "Item".into()).is_empty());
        assert!(!problems_after(|d| d.field[0].name = "BODY".into()).is_empty());
        assert!(!problems_after(|d| d.field[0].name = "ITEM3".into()).is_empty());
        assert!(!problems_after(|d| d.input[0].name = "ITEM2".into()).is_empty());
        assert!(!problems_after(|d| d.extra[1].name = "itemCount".into()).is_empty());
        assert!(!problems_after(|d| d.headers = vec!["iostream".into()]).is_empty());
        assert!(!problems_after(|d| d.headers = vec!["<bits/stdc++.h>".into()]).is_empty());
        assert!(!problems_after(|d| d.help = " ".into()).is_empty());
        assert!(!problems_after(|d| d.label.cpp = String::new()).is_empty());
        assert!(!problems_after(|d| d.label.friendly = "%MISSING".into()).is_empty());
    }

    #[test]
    fn repeat_indexes() {
        assert_eq!(repeat_index("ITEM0", "ITEM"), Some(0));
        assert_eq!(repeat_index("ITEM12", "ITEM"), Some(12));
        for not_a_part in ["ITEM", "ITEM01", "ITEMX", "ITEM-1", "ITE", "ITEM99999999999"] {
            assert_eq!(repeat_index(not_a_part, "ITEM"), None, "{not_a_part}");
        }
    }

    #[test]
    fn field_rules() {
        let text = |t: &str| Some(FieldDefault::Text(t.into()));
        assert!(!problems_after(|d| d.field[0].default = text("c")).is_empty());
        assert!(!problems_after(|d| d.field[0].default = Some(FieldDefault::Bool(true))).is_empty());
        assert!(!problems_after(|d| d.field[0].options.clear()).is_empty());
        assert!(!problems_after(|d| d.field[0].options[1][1] = "a".into()).is_empty());
        assert!(!problems_after(|d| d.field[0].types = vec!["int".into()]).is_empty());
        assert!(problems_after(|d| d.field[0].default = None).is_empty());
        let field = |kind, default, types: Vec<String>| FieldDef {
            name: "X".into(),
            kind,
            options: vec![],
            types,
            default,
            pattern: None,
            members: None,
            headers: BTreeMap::new(),
            repeat: None,
        };
        for (kind, default, types, ok) in [
            (FieldKind::Checkbox, Some(FieldDefault::Bool(true)), vec![], true),
            (FieldKind::Checkbox, text("true"), vec![], false),
            (FieldKind::Text, text(""), vec![], true),
            (FieldKind::Number, text(""), vec![], false),
            (FieldKind::Number, text("0"), vec![], true),
            (FieldKind::Type, text("int"), vec!["int".into()], true),
            (FieldKind::Type, text("long"), vec!["int".into()], false),
            (FieldKind::Type, None, vec![], false),
            (FieldKind::Type, None, vec!["int".into(), "int".into()], false),
            (FieldKind::SymbolDecl, None, vec![], true),
            (FieldKind::SymbolRef, text("x"), vec![], false),
            (
                FieldKind::Type,
                text("int"),
                vec!["int".into(), "string".into()],
                false,
            ),
            (FieldKind::Header, text("<vector>"), vec![], true),
            (FieldKind::Header, text("<math.h>"), vec![], false),
            (FieldKind::Header, None, vec!["int".into()], false),
        ] {
            let problems = problems_after(|d| d.field[0] = field(kind, default, types));
            assert_eq!(problems.is_empty(), ok, "{kind:?}: {problems:?}");
        }
    }

    #[test]
    fn input_and_statement_rules() {
        assert!(!problems_after(|d| d.input[0].repeat.as_mut().unwrap().count = "open".into()).is_empty());
        assert!(!problems_after(|d| d.input[0].repeat.as_mut().unwrap().count = "nope".into()).is_empty());
        assert!(!problems_after(|d| d.input[0].repeat.as_mut().unwrap().plus = 2).is_empty());
        assert!(!problems_after(|d| d.input[0].optional = true).is_empty());
        assert!(!problems_after(|d| d.statement[0].when = Some("itemCount".into())).is_empty());
        assert!(
            !problems_after(|d| d.statement[0].repeat = Some(Repeat {
                count: "itemCount".into(),
                plus: 0
            }))
            .is_empty()
        );
        assert!(!problems_after(|d| d.shape = Shape::Reporter).is_empty());
        assert!(!problems_after(|d| d.shape = Shape::Predicate).is_empty());
        assert!(problems_after(|d| d.shape = Shape::Definition).is_empty());
    }

    /// The test definition as a reporter (no statement inputs) with the
    /// given output type and extra fields (shown in its friendly label).
    fn value_block(shape: Shape, output: Option<&str>, fields: Vec<FieldDef>) -> Vec<String> {
        problems_after(|d| {
            d.shape = shape;
            d.statement.clear();
            d.output = output.map(|text| TypePattern::parse(text).unwrap());
            for field in &fields {
                d.label.friendly.push_str(" %");
                d.label.friendly.push_str(&field.name);
            }
            d.field.extend(fields);
        })
    }

    fn plain_field(name: &str, kind: FieldKind) -> FieldDef {
        FieldDef {
            name: name.into(),
            kind,
            options: vec![],
            types: if kind == FieldKind::Type {
                vec!["int".into(), "double".into()]
            } else {
                vec![]
            },
            default: None,
            pattern: None,
            members: None,
            headers: BTreeMap::new(),
            repeat: None,
        }
    }

    #[test]
    fn output_types_follow_the_shape() {
        // Required for value shapes, forbidden for the others.
        assert!(!value_block(Shape::Reporter, None, vec![]).is_empty());
        assert!(!value_block(Shape::Predicate, None, vec![]).is_empty());
        for shape in [Shape::Hat, Shape::Definition, Shape::Statement] {
            let problems = value_block(shape, Some("int"), vec![]);
            assert_eq!(problems.len(), 1, "{shape:?}: {problems:?}");
            assert!(
                problems[0].contains("only reporter and predicate"),
                "{problems:?}"
            );
            assert!(value_block(shape, None, vec![]).is_empty(), "{shape:?}");
        }
        for output in [
            "any",
            "bool",
            "int",
            "double",
            "number",
            "char",
            "std::string",
            "std::vector<int>",
        ] {
            assert_eq!(
                value_block(Shape::Reporter, Some(output), vec![]),
                Vec::<String>::new(),
                "{output}"
            );
        }
        // A predicate gives bool.
        assert!(value_block(Shape::Predicate, Some("bool"), vec![]).is_empty());
        let problems = value_block(Shape::Predicate, Some("int"), vec![]);
        assert_eq!(
            problems,
            ["a predicate gives bool, so its output type cannot be int"]
        );
    }

    #[test]
    fn symbol_and_field_outputs_need_their_field() {
        let reference = || plain_field("VAR", FieldKind::SymbolRef);
        assert!(value_block(Shape::Reporter, Some("symbol"), vec![reference()]).is_empty());
        let none = value_block(Shape::Reporter, Some("symbol"), vec![]);
        assert_eq!(none.len(), 1);
        assert!(none[0].contains("exactly one symbol_ref field"), "{none:?}");
        let mut second = reference();
        second.name = "OTHER".into();
        assert!(!value_block(Shape::Reporter, Some("symbol"), vec![reference(), second]).is_empty());
        assert!(value_block(Shape::Reporter, Some("symbol:VAR"), vec![reference()]).is_empty());

        let type_field = || plain_field("TO", FieldKind::Type);
        assert!(value_block(Shape::Reporter, Some("field:TO"), vec![type_field()]).is_empty());
        // The field must exist and be a type field (X is a dropdown).
        for name in ["NOPE", "X"] {
            let output = format!("field:{name}");
            let problems = value_block(Shape::Reporter, Some(&output), vec![type_field()]);
            assert_eq!(
                problems,
                [format!(
                    "the output type uses field:{name}, but the block has no type field {name}"
                )]
            );
        }
    }

    #[test]
    fn output_types_parse_as_patterns() {
        let file = |output: &str| {
            format!(
                "[[block]]\nid = \"x.y\"\nversion = 1\ncategory = \"math\"\nshape = \"reporter\"\noutput = \"{output}\"\nlabel = {{ friendly = \"x\", cpp = \"x\" }}\nlowering = \"builtin\"\nhelp = \"X.\"\n"
            )
        };
        // In a catalog file, an output that is no pattern makes the file
        // invalid: M2's `string` is written `std::string` from catalog 1.3.0.
        for bad in ["float", "string", "Int", "void*", "field:", "std::vector<int"] {
            let (catalog, problems) = build(&[("x.toml", &file(bad))]);
            assert!(catalog.blocks.is_empty(), "{bad}");
            assert_eq!(problems.len(), 1, "{bad}");
            assert!(
                problems[0].message.contains("is not a type pattern"),
                "{problems:?}"
            );
        }
        let (catalog, problems) = build(&[("x.toml", &file("double"))]);
        assert_eq!(problems, []);
        assert_eq!(
            catalog.blocks["x.y"].output,
            Some(TypePattern::Type(Type::Double))
        );
        let (catalog, problems) = build(&[("x.toml", &file("std::string"))]);
        assert_eq!(problems, []);
        assert_eq!(
            catalog.blocks["x.y"].output,
            Some(TypePattern::Type(Type::String))
        );
    }

    #[test]
    fn default_tokens_are_checked() {
        let token = |kind: &str, text: &str| {
            toml::Value::Table(
                [(kind.to_owned(), toml::Value::String(text.into()))]
                    .into_iter()
                    .collect(),
            )
        };
        assert_eq!(default_token(&token("num", "0")), Ok(Token::Num("0".into())));
        assert_eq!(default_token(&token("str", "hi")), Ok(Token::Str("hi".into())));
        assert_eq!(default_token(&token("chr", "a")), Ok(Token::Chr("a".into())));
        assert_eq!(default_token(&token("op", "-")), Ok(Token::Op("-".into())));
        assert_eq!(default_token(&token("kw", "true")), Ok(Token::Kw("true".into())));
        for bad in [
            token("kw", "nullptr"),
            token("ref", "sym_x"),
            token("text", "1 +"),
            token("bogus", "x"),
            toml::Value::String("0".into()),
            toml::Value::Table(toml::Table::new()),
            toml::Value::Table(
                [
                    ("num".to_owned(), toml::Value::String("1".into())),
                    ("str".to_owned(), toml::Value::String("1".into())),
                ]
                .into_iter()
                .collect(),
            ),
            toml::Value::Table(
                [("num".to_owned(), toml::Value::Integer(1))]
                    .into_iter()
                    .collect(),
            ),
        ] {
            assert!(default_token(&bad).is_err(), "{bad:?}");
            assert!(!problems_after(|d| d.input[0].default = vec![bad.clone()]).is_empty());
        }
    }

    #[test]
    fn extra_rules() {
        let text = |t: &str| Some(FieldDefault::Text(t.into()));
        assert!(!problems_after(|d| d.extra[0].default = text("0")).is_empty());
        assert!(!problems_after(|d| d.extra[0].default = text("x")).is_empty());
        assert!(!problems_after(|d| d.extra[0].default = Some(FieldDefault::Bool(false))).is_empty());
        assert!(!problems_after(|d| d.extra[0].min = 9).is_empty());
        assert!(!problems_after(|d| d.extra[0].max = 65).is_empty());
        assert!(!problems_after(|d| d.extra[0].types = vec!["int".into()]).is_empty());
        assert!(!problems_after(|d| d.extra[1].default = text("true")).is_empty());
        let params = |max, types: Vec<String>, default| ExtraDef {
            name: "params".into(),
            kind: ExtraKind::Params,
            min: 0,
            max,
            default,
            types,
            type_class: None,
        };
        assert!(problems_after(|d| d.extra.push(params(4, vec!["int".into()], None))).is_empty());
        assert!(!problems_after(|d| d.extra.push(params(0, vec!["int".into()], None))).is_empty());
        assert!(!problems_after(|d| d.extra.push(params(4, vec![], None))).is_empty());
        assert!(!problems_after(|d| d.extra.push(params(4, vec!["int".into()], text("x")))).is_empty());
        assert!(!problems_after(|d| d.extra.push(params(4, vec!["float".into()], None))).is_empty());
        let mut wide = params(4, vec!["int".into()], None);
        wide.type_class = Some(TypeClass::Value);
        assert!(problems_after(|d| d.extra.push(wide.clone())).is_empty());
        assert_eq!(
            problems_after(|d| d.extra[0].type_class = Some(TypeClass::Value)),
            ["only params and fields extras have a type_class (itemCount)"]
        );
    }

    /// A row extra of each kind (w2.md §2.1), named after its kind.
    fn row_extra(kind: ExtraKind) -> ExtraDef {
        ExtraDef {
            name: kind.row_name().unwrap().into(),
            kind,
            min: 0,
            max: 8,
            default: None,
            types: if kind.has_row_types() {
                vec!["int".into()]
            } else {
                vec![]
            },
            type_class: None,
        }
    }

    #[test]
    fn row_extras_have_their_kind_s_name_and_shape() {
        for kind in [ExtraKind::Params, ExtraKind::Fields, ExtraKind::Values] {
            assert_eq!(
                problems_after(|d| d.extra.push(row_extra(kind))),
                Vec::<String>::new(),
                "{kind:?}"
            );
            let mut misnamed = row_extra(kind);
            misnamed.name = "rows".into();
            let name = kind.row_name().unwrap();
            assert_eq!(
                problems_after(|d| d.extra.push(misnamed.clone())),
                [format!(
                    "an extra of kind {name} must be called {name:?}, not \"rows\""
                )],
                "{kind:?}"
            );
        }
        let mut fields = row_extra(ExtraKind::Fields);
        fields.types.clear();
        assert_eq!(
            problems_after(|d| d.extra.push(fields.clone())),
            ["fields fields need a list of distinct types"]
        );
        fields.types = vec!["int".into()];
        fields.type_class = Some(TypeClass::Value);
        assert_eq!(
            problems_after(|d| d.extra.push(fields.clone())),
            Vec::<String>::new()
        );
        let mut values = row_extra(ExtraKind::Values);
        values.types = vec!["int".into()];
        assert_eq!(
            problems_after(|d| d.extra.push(values.clone())),
            ["only params and fields extras have types (values)"]
        );
        values.types.clear();
        values.max = 0;
        assert_eq!(
            problems_after(|d| d.extra.push(values.clone())),
            ["values values must allow 1 to 64 rows"]
        );
    }

    #[test]
    fn a_repeat_may_count_rows_but_not_a_flag() {
        let by = |count: &'static str| {
            move |d: &mut BlockDef| {
                d.extra.push(row_extra(ExtraKind::Fields));
                d.input[0].repeat = Some(Repeat {
                    count: count.into(),
                    plus: 0,
                });
            }
        };
        assert_eq!(problems_after(by("fields")), Vec::<String>::new());
        assert_eq!(problems_after(by("itemCount")), Vec::<String>::new());
        assert_eq!(
            problems_after(by("open")),
            [
                "ITEM repeats by \"open\", which is not a count or a row extra (params, fields, values) in extra"
            ]
        );
    }

    /// A repeated field `F` of `kind`, counted by `itemCount`.
    fn repeated_field(name: &str, kind: FieldKind) -> FieldDef {
        FieldDef {
            name: name.into(),
            kind,
            options: if kind == FieldKind::Dropdown {
                vec![["a".into(), "a".into()]]
            } else {
                vec![]
            },
            types: if kind == FieldKind::Type {
                vec!["std::exception".into(), "std::runtime_error".into()]
            } else {
                vec![]
            },
            default: None,
            pattern: None,
            members: None,
            headers: BTreeMap::new(),
            repeat: Some(Repeat {
                count: "itemCount".into(),
                plus: 0,
            }),
        }
    }

    #[test]
    fn repeated_fields() {
        // The friendly label need not name a repeated field: it is drawn with
        // its group, as a repeated input is.
        for kind in [
            FieldKind::Dropdown,
            FieldKind::Checkbox,
            FieldKind::Type,
            FieldKind::SymbolDecl,
        ] {
            assert_eq!(
                problems_after(|d| d.field.push(repeated_field("F", kind))),
                Vec::<String>::new(),
                "{kind:?}"
            );
        }
        for kind in [
            FieldKind::Text,
            FieldKind::Number,
            FieldKind::SymbolRef,
            FieldKind::Header,
        ] {
            assert_eq!(
                problems_after(|d| d.field.push(repeated_field("F", kind))),
                ["the field F repeats, but only dropdown, checkbox, type and symbol_decl fields can"],
                "{kind:?}"
            );
        }
        // A repeated field without a count to repeat by.
        let mut uncounted = repeated_field("F", FieldKind::Dropdown);
        uncounted.repeat = Some(Repeat {
            count: "nope".into(),
            plus: 0,
        });
        assert_eq!(
            problems_after(|d| d.field.push(uncounted.clone())),
            ["F repeats by \"nope\", which is not a count or a row extra (params, fields, values) in extra"]
        );
        // Its numbered parts share the names of the block's parts.
        assert_eq!(
            problems_after(|d| {
                d.field.push(repeated_field("F", FieldKind::Checkbox));
                d.field[0].name = "F2".into();
                d.label.friendly = "do %F2 with %ITEM".into();
                d.label.cpp = "%F2(%ITEM)".into();
            }),
            ["the name F2 collides with the repeated parts of F"]
        );
    }

    #[test]
    fn field_defs_find_repeated_parts() {
        let mut block = def();
        block.field.push(repeated_field("ERROR", FieldKind::Type));
        assert_eq!(field_def(&block, "X").map(|f| f.name.as_str()), Some("X"));
        assert_eq!(
            field_def(&block, "ERROR3").map(|f| f.name.as_str()),
            Some("ERROR")
        );
        assert_eq!(field_def(&block, "ERROR"), None);
        assert_eq!(field_def(&block, "ERROR01"), None);
        assert_eq!(field_def(&block, "X0"), None);
    }

    #[test]
    fn a_type_field_without_types_needs_a_pattern() {
        let mut empty = plain_field("TYPE", FieldKind::Type);
        empty.types.clear();
        assert_eq!(
            problems_after(|d| {
                d.field.push(empty.clone());
                d.label.friendly.push_str(" %TYPE");
            }),
            ["the type field TYPE needs a list of distinct types"]
        );
    }

    #[test]
    fn broken_files_and_duplicates_are_reported() {
        let control = CORE_FILES
            .iter()
            .find(|(name, _)| *name == "control.toml")
            .map(|(_, text)| *text)
            .unwrap();
        let files = [
            ("a.toml", control),
            ("b.toml", control),
            ("c.toml", "[[block]]\nid = \"x.y\"\nunknown = 1\n"),
            ("d.toml", "not toml at all ["),
        ];
        let (catalog, problems) = build(&files);
        assert_eq!(catalog.blocks.len(), 7);
        assert_eq!(problems.len(), 9, "{problems:#?}");
        assert!(
            problems[0]
                .message
                .contains("defined more than once (again in b.toml)")
        );
        assert!(
            problems[7]
                .message
                .starts_with("c.toml is not a valid catalog file")
        );
    }

    #[test]
    fn keys_must_match_ids() {
        let mut catalog = build(&CORE_FILES).0;
        let def = catalog.blocks.remove("io.print").unwrap();
        catalog.blocks.insert("io.write".into(), def);
        let problems = check_catalog(&catalog);
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].block.as_deref(), Some("io.write"));
    }
}
