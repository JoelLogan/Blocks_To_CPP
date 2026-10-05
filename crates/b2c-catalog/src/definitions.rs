//! Loading catalog TOML files and checking the definitions themselves
//! (spec §3.11.1): a broken definition is a bug in the catalog, found here
//! instead of as confusing errors on users' blocks.

use std::collections::{BTreeMap, BTreeSet};

use b2c_model::Token;
use b2c_model::limits::MAX_VARIADIC_PARTS;

use crate::schema::{
    BlockDef, CatalogFile, ExtraDef, ExtraKind, FieldDef, FieldDefault, FieldKind, InputDef, OutputType,
    Shape, is_part_name,
};
use crate::{CATALOG_VERSION, Catalog};

/// The built-in catalog files, embedded at compile time.
pub(crate) const CORE_FILES: [(&str, &str); 8] = [
    ("control.toml", include_str!("../../../catalog/core/control.toml")),
    (
        "functions.toml",
        include_str!("../../../catalog/core/functions.toml"),
    ),
    ("io.toml", include_str!("../../../catalog/core/io.toml")),
    ("logic.toml", include_str!("../../../catalog/core/logic.toml")),
    ("math.toml", include_str!("../../../catalog/core/math.toml")),
    ("program.toml", include_str!("../../../catalog/core/program.toml")),
    ("text.toml", include_str!("../../../catalog/core/text.toml")),
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
    let catalog = Catalog {
        version: CATALOG_VERSION.to_owned(),
        blocks,
    };
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
    for header in &def.headers {
        let inner = header.strip_prefix('<').and_then(|h| h.strip_suffix('>'));
        let ok = inner.is_some_and(|h| {
            !h.is_empty()
                && h.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'/'))
        });
        if !ok {
            problems.push(format!(
                "the header {header:?} is not a standard header name like <iostream>"
            ));
        }
    }
    let extras = check_extras(def, &mut problems);
    check_names(def, &mut problems);
    for field in &def.field {
        check_field(field, &mut problems);
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
    problems
}

/// Reporters and predicates declare the type of their value; other shapes
/// give no value. A predicate gives `bool`, `symbol` needs exactly one
/// `symbol_ref` field to take the type from, and `field:NAME` names a
/// `type` field.
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
    if def.shape == Shape::Predicate && *output != OutputType::Bool {
        problems.push(format!(
            "a predicate gives bool, so its output type cannot be {output}"
        ));
    }
    match output {
        OutputType::Symbol => {
            let refs = def
                .field
                .iter()
                .filter(|f| f.kind == FieldKind::SymbolRef)
                .count();
            if refs != 1 {
                problems.push(format!(
                    "the output type symbol needs exactly one symbol_ref field to take the type from, but the block has {refs}"
                ));
            }
        }
        OutputType::Field(name)
            if !def
                .field
                .iter()
                .any(|f| &f.name == name && f.kind == FieldKind::Type) =>
        {
            problems.push(format!(
                "the output type {output} does not name a type field of the block"
            ));
        }
        _ => {}
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
        if extra.kind != ExtraKind::Params && !extra.types.is_empty() {
            problems.push(format!("only params extras have types ({})", extra.name));
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
            ExtraKind::Params => {
                if extra.max == 0 || extra.max > limit {
                    problems.push(format!(
                        "params {} must allow 1 to {MAX_VARIADIC_PARTS} rows",
                        extra.name
                    ));
                }
                if extra.types.is_empty() || !all_unique(&extra.types) {
                    problems.push(format!("params {} need a list of distinct types", extra.name));
                }
                if extra.default.is_some() {
                    problems.push(format!("params {} cannot have a default", extra.name));
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

fn check_repeat(
    name: &str,
    count: &str,
    plus: u32,
    extras: &BTreeMap<&str, ExtraKind>,
    problems: &mut Vec<String>,
) {
    if extras.get(count) != Some(&ExtraKind::Count) {
        problems.push(format!(
            "{name} repeats by {count:?}, which is not a count in extra"
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
        .map(|f| (f.name.as_str(), false))
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
        FieldKind::Type if field.types.is_empty() || !all_unique(&field.types) => {
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
/// text, non-empty number text, or one of a type field's types. Symbol
/// fields take no such value: the editor fills them in.
pub(crate) fn field_value_fits(field: &FieldDef, value: &FieldDefault) -> bool {
    match (field.kind, value) {
        (FieldKind::Dropdown, FieldDefault::Text(text)) => {
            field.options.iter().any(|[_, option]| option == text)
        }
        (FieldKind::Checkbox, FieldDefault::Bool(_)) | (FieldKind::Text, FieldDefault::Text(_)) => true,
        (FieldKind::Number, FieldDefault::Text(text)) => !text.is_empty(),
        (FieldKind::Type, FieldDefault::Text(text)) => field.types.contains(text),
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
                },
                ExtraDef {
                    name: "open".into(),
                    kind: ExtraKind::Flag,
                    min: 0,
                    max: 1,
                    default: Some(FieldDefault::Bool(true)),
                    types: vec![],
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
        assert_eq!(catalog.blocks.len(), 32);
        assert_eq!(catalog.version, CATALOG_VERSION);
    }

    #[test]
    fn the_test_definition_is_valid() {
        assert_eq!(check_block(&def()), Vec::<String>::new());
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
    /// given output type and extra fields.
    fn value_block(shape: Shape, output: Option<OutputType>, fields: Vec<FieldDef>) -> Vec<String> {
        problems_after(|d| {
            d.shape = shape;
            d.statement.clear();
            d.output = output;
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
        }
    }

    #[test]
    fn output_types_follow_the_shape() {
        use OutputType as O;
        // Required for value shapes, forbidden for the others.
        assert!(!value_block(Shape::Reporter, None, vec![]).is_empty());
        assert!(!value_block(Shape::Predicate, None, vec![]).is_empty());
        for shape in [Shape::Hat, Shape::Definition, Shape::Statement] {
            let problems = value_block(shape, Some(O::Int), vec![]);
            assert_eq!(problems.len(), 1, "{shape:?}: {problems:?}");
            assert!(
                problems[0].contains("only reporter and predicate"),
                "{problems:?}"
            );
            assert!(value_block(shape, None, vec![]).is_empty(), "{shape:?}");
        }
        for output in [
            O::Any,
            O::Bool,
            O::Int,
            O::Double,
            O::Number,
            O::Char,
            O::StdString,
        ] {
            assert!(
                value_block(Shape::Reporter, Some(output.clone()), vec![]).is_empty(),
                "{output}"
            );
        }
        // A predicate gives bool.
        assert!(value_block(Shape::Predicate, Some(O::Bool), vec![]).is_empty());
        let problems = value_block(Shape::Predicate, Some(O::Int), vec![]);
        assert_eq!(
            problems,
            ["a predicate gives bool, so its output type cannot be int"]
        );
    }

    #[test]
    fn symbol_and_field_outputs_need_their_field() {
        use OutputType as O;
        let reference = || plain_field("VAR", FieldKind::SymbolRef);
        assert!(value_block(Shape::Reporter, Some(O::Symbol), vec![reference()]).is_empty());
        let none = value_block(Shape::Reporter, Some(O::Symbol), vec![]);
        assert_eq!(none.len(), 1);
        assert!(none[0].contains("exactly one symbol_ref field"), "{none:?}");
        let mut second = reference();
        second.name = "OTHER".into();
        assert!(!value_block(Shape::Reporter, Some(O::Symbol), vec![reference(), second]).is_empty());

        let type_field = || plain_field("TO", FieldKind::Type);
        assert!(value_block(Shape::Reporter, Some(O::Field("TO".into())), vec![type_field()]).is_empty());
        // The field must exist and be a type field (X is a dropdown).
        for name in ["NOPE", "X"] {
            let problems = value_block(Shape::Reporter, Some(O::Field(name.into())), vec![type_field()]);
            assert_eq!(
                problems,
                [format!(
                    "the output type field:{name} does not name a type field of the block"
                )]
            );
        }
    }

    #[test]
    fn output_types_parse_and_print() {
        use OutputType as O;
        for (text, output) in [
            ("any", O::Any),
            ("bool", O::Bool),
            ("int", O::Int),
            ("double", O::Double),
            ("number", O::Number),
            ("char", O::Char),
            ("string", O::StdString),
            ("symbol", O::Symbol),
            ("field:TO", O::Field("TO".into())),
            ("field:A_1", O::Field("A_1".into())),
        ] {
            assert_eq!(text.parse::<OutputType>(), Ok(output.clone()), "{text}");
            assert_eq!(output.to_string(), text);
            assert_eq!(String::from(output), text);
        }
        for bad in [
            "",
            "Int",
            "std::string",
            "void",
            "field:",
            "field:to",
            "field:1X",
            "field: TO",
            "TO",
        ] {
            assert_eq!(
                bad.parse::<OutputType>(),
                Err(crate::schema::OutputTypeError(bad.into())),
                "{bad}"
            );
        }
        let error = "float".parse::<OutputType>().unwrap_err().to_string();
        assert!(error.starts_with("\"float\" is not an output type"), "{error}");
        // In a catalog file, a bad output type makes the file invalid.
        let text = "[[block]]\nid = \"x.y\"\nversion = 1\ncategory = \"math\"\nshape = \"reporter\"\noutput = \"float\"\nlabel = { friendly = \"x\", cpp = \"x\" }\nlowering = \"builtin\"\nhelp = \"X.\"\n";
        let (catalog, problems) = build(&[("x.toml", text)]);
        assert!(catalog.blocks.is_empty());
        assert_eq!(problems.len(), 1);
        assert!(
            problems[0].message.contains("is not an output type"),
            "{problems:?}"
        );
        let (catalog, problems) = build(&[("x.toml", &text.replace("float", "double"))]);
        assert_eq!(problems, []);
        assert_eq!(catalog.blocks["x.y"].output, Some(O::Double));
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
        };
        assert!(problems_after(|d| d.extra.push(params(4, vec!["int".into()], None))).is_empty());
        assert!(!problems_after(|d| d.extra.push(params(0, vec!["int".into()], None))).is_empty());
        assert!(!problems_after(|d| d.extra.push(params(4, vec![], None))).is_empty());
        assert!(!problems_after(|d| d.extra.push(params(4, vec!["int".into()], text("x")))).is_empty());
    }

    #[test]
    fn broken_files_and_duplicates_are_reported() {
        let files = [
            ("a.toml", CORE_FILES[0].1),
            ("b.toml", CORE_FILES[0].1),
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
