//! Typed access to the parts of a block: fields, inputs, mutator state and
//! statement lists.
//!
//! The analyser expects a document that `b2c_catalog::resolve` has already
//! checked, but it must never panic on surprising input. Every accessor
//! therefore returns a [`Result`] or falls back to the catalog default, and the
//! caller decides whether a problem deserves a diagnostic.

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::PassMode;
use b2c_ir::types::Type;
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
    /// Its type, or the unusable type text.
    pub(crate) ty: Result<Type, String>,
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
    let ty = match text("type") {
        Some(t) => match Type::from_field(t) {
            Some(Type::Void) | None => Err(t.to_owned()),
            Some(ty) => Ok(ty),
        },
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

/// Number of `else if` parts of a `control.if` block.
pub(crate) fn else_if_count(block: &Block) -> usize {
    extra_count(block, "elseIfCount", 0, 0)
}

/// The statement lists that the analyser lowers for a block, in order, with
/// their input names. Lists of other blocks (or other inputs) are not part of
/// the program.
pub(crate) fn statement_lists(block: &Block) -> Vec<(String, &[Block])> {
    match block.block_type.as_str() {
        "control.if" => {
            let mut lists: Vec<(String, &[Block])> = (0..=else_if_count(block))
                .map(|i| {
                    let name = format!("DO{i}");
                    let list = statements(block, &name);
                    (name, list)
                })
                .collect();
            if extra_flag(block, "hasElse") {
                lists.push((String::from("ELSE"), statements(block, "ELSE")));
            }
            lists
        }
        "control.while" | "control.repeat" | "control.for_range" | "control.forever" | "program.main"
        | "func.define" => vec![(String::from("BODY"), statements(block, "BODY"))],
        _ => Vec::new(),
    }
}

/// Block types the analyser lowers as statements.
pub(crate) fn is_statement_type(block_type: &str) -> bool {
    matches!(
        block_type,
        "var.declare"
            | "var.set"
            | "var.change"
            | "var.update"
            | "control.if"
            | "control.while"
            | "control.repeat"
            | "control.for_range"
            | "control.forever"
            | "control.break"
            | "control.continue"
            | "io.print"
            | "io.ask"
            | "func.call_stmt"
            | "func.return"
            | "program.exit"
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
                {"sym": "p3", "name": "c", "type": "int", "mode": "moved"},
                {"sym": "bad-id", "name": "d", "type": "int"},
                {"name": "e"},
                42
            ]}}),
        );
        let rows = param_rows(&b);
        assert_eq!(rows.len(), 6);
        let first = rows[0].as_ref().expect("row");
        assert_eq!(first.ty, Ok(Type::Int));
        assert_eq!(first.mode, Ok(PassMode::Editable));
        let second = rows[1].as_ref().expect("row");
        assert_eq!(second.ty, Err(String::from("void")));
        assert_eq!(second.mode, Ok(PassMode::Copy));
        assert_eq!(rows[2].as_ref().expect("row").mode, Err(String::from("moved")));
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
}
