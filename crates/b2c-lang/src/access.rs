//! Typed access to the parts of a block: fields, inputs, mutator state and
//! statement lists.
//!
//! The analyser expects a document that `b2c_catalog::resolve` has already
//! checked, but it must never panic on surprising input. Every accessor
//! therefore returns a [`Result`] or falls back to the catalog default, and the
//! caller decides whether a problem deserves a diagnostic.

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::PassMode;
use b2c_ir::types::TypeTerm;
use b2c_model::limits::MAX_VARIADIC_PARTS;
use b2c_model::{Block, FieldValue, Input, SymbolDecl};
use serde_json::Value;

/// Why a field could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldProblem {
    /// The field is absent and has no default.
    Missing,
    /// The field holds the wrong kind of value.
    WrongKind,
}

/// A text-like field (dropdown value, type name, number text or text), or
/// `default` when the field is absent.
pub(crate) fn text_field<'b>(
    block: &'b Block,
    name: &str,
    default: Option<&'b str>,
) -> Result<&'b str, FieldProblem> {
    match block.fields.get(name) {
        Some(FieldValue::Text(text)) => Ok(text),
        Some(_) => Err(FieldProblem::WrongKind),
        None => default.ok_or(FieldProblem::Missing),
    }
}

/// A checkbox field, or `default` when the field is absent.
pub(crate) fn bool_field(block: &Block, name: &str, default: bool) -> Result<bool, FieldProblem> {
    match block.fields.get(name) {
        Some(FieldValue::Bool(value)) => Ok(*value),
        Some(_) => Err(FieldProblem::WrongKind),
        None => Ok(default),
    }
}

/// A symbol-declaration field (`{"sym", "name"}`).
pub(crate) fn decl_field<'b>(block: &'b Block, name: &str) -> Result<&'b SymbolDecl, FieldProblem> {
    match block.fields.get(name) {
        Some(FieldValue::Decl(decl)) => Ok(decl),
        Some(_) => Err(FieldProblem::WrongKind),
        None => Err(FieldProblem::Missing),
    }
}

/// A symbol-reference field (`{"ref"}`).
pub(crate) fn ref_field<'b>(block: &'b Block, name: &str) -> Result<&'b SymbolId, FieldProblem> {
    match block.fields.get(name) {
        Some(FieldValue::Ref(reference)) => Ok(&reference.target),
        Some(_) => Err(FieldProblem::WrongKind),
        None => Err(FieldProblem::Missing),
    }
}

/// A value input, if present.
pub(crate) fn input<'b>(block: &'b Block, name: &str) -> Option<&'b Input> {
    block.inputs.get(name)
}

/// The blocks of a statement input (empty when absent).
pub(crate) fn statements<'b>(block: &'b Block, name: &str) -> &'b [Block] {
    block.statements.get(name).map_or(&[], Vec::as_slice)
}

/// A count from the block's mutator state, clamped to `min ..= MAX_VARIADIC_PARTS`
/// so that a damaged count can never make the analyser loop for long.
/// Absent or malformed counts give `default` (the catalog reports them).
pub(crate) fn extra_count(block: &Block, key: &str, min: usize, default: usize) -> usize {
    let value = match block.extra.get(key) {
        Some(Value::Number(number)) => number.as_u64().and_then(|n| usize::try_from(n).ok()),
        Some(Value::String(text)) => text.parse::<usize>().ok(),
        _ => None,
    };
    value.unwrap_or(default).clamp(min, MAX_VARIADIC_PARTS.max(min))
}

/// A flag from the block's mutator state (absent or malformed means `false`).
pub(crate) fn extra_flag(block: &Block, key: &str) -> bool {
    matches!(block.extra.get(key), Some(Value::Bool(true)))
}

/// One parameter row of a `func.define` block (`extra.params`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParamRow {
    /// The parameter's symbol.
    pub(crate) sym: SymbolId,
    /// The user's name for it (not yet validated as an identifier).
    pub(crate) name: String,
    /// Its type as written (a term the analyser looks up), or the unusable
    /// value as text.
    pub(crate) ty: Result<TypeTerm, String>,
    /// How it is passed, or the unusable mode text.
    pub(crate) mode: Result<PassMode, String>,
}

/// The parameter rows of a `func.define` block. A row without a usable symbol
/// ID or name is `Err(())`. At most [`MAX_VARIADIC_PARTS`] rows are read.
pub(crate) fn param_rows(block: &Block) -> Vec<Result<ParamRow, ()>> {
    let Some(Value::Array(rows)) = block.extra.get("params") else {
        return Vec::new();
    };
    rows.iter().take(MAX_VARIADIC_PARTS).map(param_row).collect()
}

fn param_row(row: &Value) -> Result<ParamRow, ()> {
    let Value::Object(map) = row else {
        return Err(());
    };
    let text = |key: &str| map.get(key).and_then(Value::as_str);
    let sym = text("sym").and_then(|s| SymbolId::new(s).ok()).ok_or(())?;
    let name = text("name").ok_or(())?.to_owned();
    // A name, a template or a user type, as in a type field (types.md §2.1).
    let ty = match map.get("type") {
        Some(value) => serde_json::from_value::<TypeTerm>(value.clone())
            .map_err(|_| value.as_str().map_or_else(|| value.to_string(), str::to_owned)),
        None => Err(String::new()),
    };
    let mode = match text("mode") {
        None | Some("copy") => Ok(PassMode::Copy),
        Some("editable") => Ok(PassMode::Editable),
        Some("read_only") => Ok(PassMode::ReadOnly),
        Some(other) => Err(other.to_owned()),
    };
    Ok(ParamRow { sym, name, ty, mode })
}

/// One row of a struct's `extra.fields` or an enum's `extra.values` (w2.md
/// §2.1).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "read by the lowering of W2's struct and enum blocks")
)]
pub(crate) struct Row {
    /// The row's symbol.
    pub(crate) sym: SymbolId,
    /// The user's name for it (not yet validated as an identifier).
    pub(crate) name: String,
    /// Its type as written, for a field row (a term the analyser looks up),
    /// or the unusable value as text; `None` for a value row.
    pub(crate) ty: Option<Result<TypeTerm, String>>,
}

/// The rows of `extra.fields` (with types) or `extra.values` (without). A
/// row without a usable symbol ID or name is `Err(())`. At most
/// [`MAX_VARIADIC_PARTS`] rows are read.
#[cfg_attr(
    not(test),
    allow(dead_code, reason = "read by the lowering of W2's struct and enum blocks")
)]
pub(crate) fn rows(block: &Block, key: &str) -> Vec<Result<Row, ()>> {
    let Some(Value::Array(rows)) = block.extra.get(key) else {
        return Vec::new();
    };
    let typed = key == "fields";
    rows.iter()
        .take(MAX_VARIADIC_PARTS)
        .map(|row| {
            let Value::Object(map) = row else {
                return Err(());
            };
            let text = |key: &str| map.get(key).and_then(Value::as_str);
            let sym = text("sym").and_then(|s| SymbolId::new(s).ok()).ok_or(())?;
            let name = text("name").ok_or(())?.to_owned();
            let ty = typed.then(|| match map.get("type") {
                Some(value) => serde_json::from_value::<TypeTerm>(value.clone())
                    .map_err(|_| value.as_str().map_or_else(|| value.to_string(), str::to_owned)),
                None => Err(String::new()),
            });
            Ok(Row { sym, name, ty })
        })
        .collect()
}

/// Number of `else if` parts of a `control.if` block.
pub(crate) fn else_if_count(block: &Block) -> usize {
    extra_count(block, "elseIfCount", 0, 0)
}

/// Number of `case` parts of a `control.switch` block (w2.md §1.6).
pub(crate) fn case_count(block: &Block) -> usize {
    extra_count(block, "caseCount", 1, 2)
}

/// Number of catch parts of an `error.try` block (w2.md §1.7).
pub(crate) fn catch_count(block: &Block) -> usize {
    extra_count(block, "catchCount", 1, 1)
}

/// The statement lists that the analyser lowers for a block, in order, with
/// their input names. Lists of other blocks (or other inputs) are not part of
/// the program. W2's blocks are here before they exist (w2.md §2.13), so
/// their packages never edit this table.
pub(crate) fn statement_lists(block: &Block) -> Vec<(String, &[Block])> {
    let list = |name: &str| (name.to_owned(), statements(block, name));
    let numbered = |base: &'static str, count: usize| (0..count).map(move |i| list(&format!("{base}{i}")));
    let flagged = |flag: &str, name: &str| extra_flag(block, flag).then(|| list(name));
    match block.block_type.as_str() {
        "control.if" => {
            let mut lists: Vec<(String, &[Block])> = numbered("DO", else_if_count(block) + 1).collect();
            lists.extend(flagged("hasElse", "ELSE"));
            lists
        }
        "control.switch" => {
            let mut lists: Vec<(String, &[Block])> = numbered("DO", case_count(block)).collect();
            lists.extend(flagged("hasDefault", "DEFAULT"));
            lists
        }
        "error.try" => std::iter::once(list("TRY"))
            .chain(numbered("CATCH", catch_count(block)))
            .collect(),
        "file.for_each_line" => std::iter::once(list("BODY"))
            .chain(flagged("hasFailed", "FAILED"))
            .collect(),
        "file.open" => flagged("hasFailed", "FAILED").into_iter().collect(),
        "control.while" | "control.repeat" | "control.for_range" | "control.forever" | "control.do_while"
        | "coll.for_each" | "coll.for_each_pair" | "program.main" | "func.define" => vec![list("BODY")],
        _ => Vec::new(),
    }
}

/// Block types the analyser lowers as statements: M2's and W2's (w2.md
/// §2.13). A test checks the list against the catalog's shapes.
pub(crate) fn is_statement_type(block_type: &str) -> bool {
    matches!(
        block_type,
        "var.declare"
            | "var.set"
            | "var.change"
            | "var.flip"
            | "control.if"
            | "control.while"
            | "control.repeat"
            | "control.for_range"
            | "control.forever"
            | "control.break"
            | "control.continue"
            | "control.switch"
            | "control.do_while"
            | "control.wait"
            | "control.fallthrough"
            | "io.print"
            | "io.ask"
            | "io.decimals"
            | "func.call_stmt"
            | "func.return"
            | "program.exit"
            | "program.check"
            | "coll.add"
            | "coll.set_item"
            | "coll.for_each"
            | "coll.for_each_pair"
            | "coll.clear"
            | "coll.remove_at"
            | "coll.remove_value"
            | "coll.remove_next"
            | "coll.sort"
            | "type.set_field"
            | "error.try"
            | "error.throw"
            | "error.rethrow"
            | "file.for_each_line"
            | "file.open"
            | "file.write"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn block(value: serde_json::Value) -> Block {
        serde_json::from_value(value).expect("valid block")
    }

    #[test]
    fn fields_and_defaults() {
        let b = block(json!({
            "id": "b1", "type": "var.declare", "v": 1,
            "fields": {"TYPE": "int", "NAME": {"sym": "s1", "name": "x"}, "CONST": true}
        }));
        assert_eq!(text_field(&b, "TYPE", Some("double")), Ok("int"));
        assert_eq!(text_field(&b, "OTHER", Some("double")), Ok("double"));
        assert_eq!(text_field(&b, "OTHER", None), Err(FieldProblem::Missing));
        assert_eq!(text_field(&b, "CONST", None), Err(FieldProblem::WrongKind));
        assert_eq!(bool_field(&b, "CONST", false), Ok(true));
        assert_eq!(bool_field(&b, "NOPE", true), Ok(true));
        assert_eq!(bool_field(&b, "TYPE", true), Err(FieldProblem::WrongKind));
        assert_eq!(decl_field(&b, "NAME").map(|d| d.name.as_str()), Ok("x"));
        assert_eq!(decl_field(&b, "TYPE").err(), Some(FieldProblem::WrongKind));
        assert_eq!(decl_field(&b, "X").err(), Some(FieldProblem::Missing));
        assert_eq!(ref_field(&b, "NAME").err(), Some(FieldProblem::WrongKind));
        assert_eq!(ref_field(&b, "X").err(), Some(FieldProblem::Missing));
    }

    #[test]
    fn counts_are_clamped() {
        let b = block(json!({"id": "b", "type": "text.join", "v": 1,
            "extra": {"a": 5, "b": "7", "c": 1_000_000, "d": -1, "e": true}}));
        assert_eq!(extra_count(&b, "a", 2, 2), 5);
        assert_eq!(extra_count(&b, "b", 2, 2), 7);
        assert_eq!(extra_count(&b, "c", 2, 2), MAX_VARIADIC_PARTS);
        assert_eq!(extra_count(&b, "d", 2, 3), 3);
        assert_eq!(extra_count(&b, "e", 0, 0), 0);
        assert_eq!(extra_count(&b, "missing", 1, 0), 1);
        assert!(extra_flag(&b, "e"));
        assert!(!extra_flag(&b, "a"));
    }

    #[test]
    fn params() {
        let b = block(
            json!({"id": "f", "type": "func.define", "v": 1, "extra": {"params": [
                {"sym": "p1", "name": "a", "type": "int", "mode": "editable"},
                {"sym": "p2", "name": "b", "type": "void"},
                {"sym": "p3", "name": "c", "type": {"std::vector": "int"}, "mode": "moved"},
                {"sym": "bad-id", "name": "d", "type": "int"},
                {"name": "e"},
                42
            ]}}),
        );
        let rows = param_rows(&b);
        assert_eq!(rows.len(), 6);
        let first = rows[0].as_ref().expect("row");
        assert_eq!(first.ty, TypeTerm::name("int").map_err(|_| String::new()));
        assert_eq!(first.mode, Ok(PassMode::Editable));
        let second = rows[1].as_ref().expect("row");
        // `void` is a term; the analyser refuses it as a parameter's type.
        assert_eq!(second.ty, TypeTerm::name("void").map_err(|_| String::new()));
        assert_eq!(second.mode, Ok(PassMode::Copy));
        let third = rows[2].as_ref().expect("row");
        assert_eq!(third.mode, Err(String::from("moved")));
        assert!(matches!(third.ty, Ok(TypeTerm::App(..))));
        assert!(rows[3].is_err() && rows[4].is_err() && rows[5].is_err());
    }

    #[test]
    fn lists() {
        let b = block(json!({"id": "i", "type": "control.if", "v": 1,
            "extra": {"elseIfCount": 1, "hasElse": true},
            "statements": {"DO0": [], "ELSE": []}}));
        let names: Vec<String> = statement_lists(&b).into_iter().map(|(n, _)| n).collect();
        assert_eq!(names, ["DO0", "DO1", "ELSE"]);
        let p = block(json!({"id": "p", "type": "io.print", "v": 1}));
        assert!(statement_lists(&p).is_empty());
        assert!(is_statement_type("io.print"));
        assert!(!is_statement_type("math.number"));
    }

    /// The lists of W2's blocks (w2.md §1.6 to §1.8), by their counts and
    /// flags.
    #[test]
    fn w2_lists() {
        let names = |value: serde_json::Value| -> Vec<String> {
            statement_lists(&block(value))
                .into_iter()
                .map(|(n, _)| n)
                .collect()
        };
        assert_eq!(
            names(json!({"id": "s", "type": "control.switch", "v": 1,
                "extra": {"caseCount": 3, "hasDefault": true}})),
            ["DO0", "DO1", "DO2", "DEFAULT"]
        );
        assert_eq!(
            names(json!({"id": "s", "type": "control.switch", "v": 1, "extra": {"hasDefault": false}})),
            ["DO0", "DO1"]
        );
        assert_eq!(
            names(json!({"id": "t", "type": "error.try", "v": 1, "extra": {"catchCount": 2}})),
            ["TRY", "CATCH0", "CATCH1"]
        );
        assert_eq!(
            names(json!({"id": "f", "type": "file.for_each_line", "v": 1, "extra": {"hasFailed": true}})),
            ["BODY", "FAILED"]
        );
        assert_eq!(
            names(json!({"id": "f", "type": "file.for_each_line", "v": 1})),
            ["BODY"]
        );
        assert_eq!(
            names(json!({"id": "o", "type": "file.open", "v": 1, "extra": {"hasFailed": true}})),
            ["FAILED"]
        );
        assert!(names(json!({"id": "o", "type": "file.open", "v": 1})).is_empty());
        for loop_type in ["control.do_while", "coll.for_each", "coll.for_each_pair"] {
            assert_eq!(names(json!({"id": "l", "type": loop_type, "v": 1})), ["BODY"]);
        }
        for statement in [
            "coll.add",
            "error.throw",
            "file.write",
            "var.flip",
            "program.check",
        ] {
            assert!(is_statement_type(statement), "{statement}");
        }
        for value in [
            "coll.item",
            "text.length",
            "type.new",
            "program.use_header",
            "type.struct",
        ] {
            assert!(!is_statement_type(value), "{value}");
        }
    }

    #[test]
    fn struct_and_enum_rows() {
        let b = block(json!({"id": "s", "type": "type.struct", "v": 1, "extra": {
            "fields": [
                {"sym": "f1", "name": "name", "type": "std::string"},
                {"sym": "f2", "name": "marks", "type": {"std::vector": "int"}},
                {"sym": "f3", "name": "x"},
                {"sym": "bad-id", "name": "y", "type": "int"},
                7
            ],
            "values": [{"sym": "v1", "name": "Red"}, {"name": "Blue"}]
        }}));
        let fields = rows(&b, "fields");
        assert_eq!(fields.len(), 5);
        let first = fields[0].as_ref().expect("row");
        assert_eq!((first.sym.as_str(), first.name.as_str()), ("f1", "name"));
        assert_eq!(
            first.ty,
            Some(TypeTerm::name("std::string").map_err(|_| String::new()))
        );
        assert!(matches!(
            fields[1].as_ref().expect("row").ty,
            Some(Ok(TypeTerm::App(..)))
        ));
        assert_eq!(fields[2].as_ref().expect("row").ty, Some(Err(String::new())));
        assert!(fields[3].is_err() && fields[4].is_err());
        let values = rows(&b, "values");
        assert_eq!(values.len(), 2);
        assert_eq!(values[0].as_ref().expect("row").ty, None);
        assert!(values[1].is_err());
        assert!(rows(&b, "params").is_empty());
    }
}
