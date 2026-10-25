//! Type terms in documents (types.md §2): which objects are templates, the
//! row keys whose rows carry a `type`, and walks over the terms of those
//! rows, which a document keeps as free-form JSON in `extra`.
//!
//! A type field holds its term as a [`FieldValue`]: a name as text, a user
//! type as a reference, a template as [`FieldValue::Type`]. A row's `type`
//! is the same term written in JSON: `"int"`, `{"ref": "sym_Pt"}` or
//! `{"std::vector": "int"}`. The walks here read that JSON the way the
//! loader does (§2.2) and leave anything else alone, so they work on any
//! document, loaded or built in memory, without recursion.

use std::collections::BTreeMap;

use b2c_ir::SymbolId;
use b2c_ir::types::TypeTerm;
use serde_json::Value;

use crate::document::{Block, FieldValue};

/// The `extra` keys whose rows have a `type` that is a type term
/// (types.md §2.1): `params`, the parameter rows of a function. The rows of
/// struct fields and enum values join with the blocks that use them (W2).
pub(crate) const ROW_KEYS: [&str; 1] = ["params"];

/// Whether `key` names a template: a qualified name (it contains `::`)
/// that follows the name rule of types.md §2.2, such as `std::vector`.
pub(crate) fn is_template_key(key: &str) -> bool {
    key.contains("::") && TypeTerm::name(key).is_ok()
}

/// The `type` of every row of the known row keys in a block's `extra`.
pub(crate) fn row_types(block: &Block) -> impl Iterator<Item = &Value> {
    ROW_KEYS
        .iter()
        .filter_map(|key| block.extra.get(*key)?.as_array())
        .flatten()
        .filter_map(|row| row.get("type"))
}

/// Like [`row_types`], with mutable access.
fn row_types_mut(block: &mut Block) -> impl Iterator<Item = &mut Value> {
    block
        .extra
        .iter_mut()
        .filter(|(key, _)| ROW_KEYS.contains(&key.as_str()))
        .filter_map(|(_, rows)| rows.as_array_mut())
        .flatten()
        .filter_map(|row| row.get_mut("type"))
}

/// Whether a row's `type` is a template: an object whose one key names a
/// template.
pub(crate) fn is_template(value: &Value) -> bool {
    matches!(value, Value::Object(map) if map.len() == 1 && map.keys().all(|key| is_template_key(key)))
}

/// Whether a block itself (not its children) holds a template term, in a
/// field or in the `type` of a row: a key of project format version 2 and
/// clipboard format version 2 (types.md §2.3, §2.4).
pub(crate) fn has_template(block: &Block) -> bool {
    block
        .fields
        .values()
        .any(|value| matches!(value, FieldValue::Type(_)))
        || row_types(block).any(is_template)
}

/// Every reference in a block's type terms (not its children's), in its
/// fields and in the `type` of its rows, in order.
pub(crate) fn refs(block: &Block) -> Vec<SymbolId> {
    let mut found = Vec::new();
    for value in block.fields.values() {
        if let FieldValue::Type(term) = value {
            found.extend(term.refs().cloned());
        }
    }
    for value in row_types(block) {
        let mut pending = vec![value];
        while let Some(value) = pending.pop() {
            match term_part(value) {
                Part::Ref(Value::String(text)) => found.extend(SymbolId::new(text).ok()),
                Part::Ref(_) | Part::Leaf => {}
                Part::Args(Value::Array(items)) => pending.extend(items.iter().rev()),
                Part::Args(arg) => pending.push(arg),
            }
        }
    }
    found
}

/// Points every reference in a block's type terms (not its children's)
/// that `map` has a new ID for at that ID.
pub(crate) fn rename(block: &mut Block, map: &BTreeMap<SymbolId, SymbolId>) {
    for value in block.fields.values_mut() {
        if let FieldValue::Type(term) = value {
            term.rename(map);
        }
    }
    for value in row_types_mut(block) {
        let mut pending = vec![value];
        while let Some(value) = pending.pop() {
            match term_part_mut(value) {
                PartMut::Ref(Value::String(text)) => {
                    if let Ok(sym) = SymbolId::new(text)
                        && let Some(new) = map.get(&sym)
                    {
                        new.as_str().clone_into(text);
                    }
                }
                PartMut::Ref(_) | PartMut::Leaf => {}
                PartMut::Args(Value::Array(items)) => pending.extend(items.iter_mut()),
                PartMut::Args(arg) => pending.push(arg),
            }
        }
    }
}

/// What a JSON value is when read as a type term.
enum Part<'v> {
    /// `{"ref": …}`: the value of `ref`.
    Ref(&'v Value),
    /// A template: its argument, or the array of its arguments.
    Args(&'v Value),
    /// A name, or anything that is not a term.
    Leaf,
}

/// Like [`Part`], with mutable access.
enum PartMut<'v> {
    Ref(&'v mut Value),
    Args(&'v mut Value),
    Leaf,
}

fn term_part(value: &Value) -> Part<'_> {
    match value {
        Value::Object(map) if map.len() == 1 => match map.iter().next() {
            Some((key, inner)) if key == "ref" => Part::Ref(inner),
            Some((key, inner)) if is_template_key(key) => Part::Args(inner),
            _ => Part::Leaf,
        },
        _ => Part::Leaf,
    }
}

fn term_part_mut(value: &mut Value) -> PartMut<'_> {
    match value {
        Value::Object(map) if map.len() == 1 => match map.iter_mut().next() {
            Some((key, inner)) if key == "ref" => PartMut::Ref(inner),
            Some((key, inner)) if is_template_key(key) => PartMut::Args(inner),
            _ => PartMut::Leaf,
        },
        _ => PartMut::Leaf,
    }
}

#[cfg(test)]
mod tests {
    use b2c_ir::BlockId;
    use serde_json::json;

    use super::*;

    fn with_extra(extra: Value) -> Block {
        Block {
            id: BlockId::new("b").unwrap(),
            block_type: "func.define".into(),
            v: 1,
            x: None,
            y: None,
            collapsed: false,
            disabled: false,
            comment: None,
            extra: serde_json::from_value(extra).unwrap(),
            fields: BTreeMap::new(),
            inputs: BTreeMap::new(),
            statements: BTreeMap::new(),
            stack: Vec::new(),
        }
    }

    fn sym(id: &str) -> SymbolId {
        SymbolId::new(id).unwrap()
    }

    #[test]
    fn template_keys_are_qualified_names() {
        for key in ["std::vector", "std::map", "a::b", "std::priority_queue", "x::y z"] {
            assert!(is_template_key(key), "{key}");
        }
        for key in [
            "vector",
            "ref",
            "sym",
            "",
            "::",
            "std::",
            "::x",
            "std:vector",
            "std::vec tor ",
            "std::\u{202e}x",
        ] {
            assert!(!is_template_key(key), "{key}");
        }
        assert!(is_template(&json!({"std::vector": "int"})));
        assert!(!is_template(&json!({"ref": "s"})));
        assert!(!is_template(&json!({"std::vector": "int", "std::set": "int"})));
        assert!(!is_template(&json!("std::vector")));
    }

    #[test]
    fn rows_of_known_keys_only() {
        let block = with_extra(json!({
            "params": [
                {"sym": "a", "type": {"std::vector": {"ref": "s_pt"}}},
                {"sym": "b", "type": "int"},
                {"sym": "c"},
                "not a row"
            ],
            "other": [{"type": {"std::vector": {"ref": "s_no"}}}]
        }));
        let types: Vec<&Value> = row_types(&block).collect();
        assert_eq!(types, [&json!({"std::vector": {"ref": "s_pt"}}), &json!("int")]);
        assert!(has_template(&block));
        assert_eq!(refs(&block), [sym("s_pt")]);
        let plain = with_extra(json!({"params": [{"type": "int"}, {"type": {"ref": "s_pt"}}]}));
        assert!(!has_template(&plain));
        assert_eq!(refs(&plain), [sym("s_pt")]);
    }

    #[test]
    fn refs_and_renames_follow_the_term_shape() {
        let mut block = with_extra(json!({"params": [{"type": {"std::map": [
            {"ref": "s_a"},
            {"std::vector": [{"ref": "s_b"}, {"ref": "bad id"}]},
            {"ref": "s_a", "x": 1},
            [{"ref": "s_c"}],
            {"vector": {"ref": "s_d"}}
        ]}}]}));
        block.fields.insert(
            "TYPE".into(),
            FieldValue::Type(TypeTerm::app("std::set", vec![TypeTerm::Ref(sym("s_e"))]).unwrap()),
        );
        // Only what the loader reads as a reference counts.
        assert_eq!(refs(&block), [sym("s_e"), sym("s_a"), sym("s_b")]);
        let map = BTreeMap::from([
            (sym("s_a"), sym("t_a")),
            (sym("s_c"), sym("t_c")),
            (sym("s_e"), sym("t_e")),
        ]);
        rename(&mut block, &map);
        assert_eq!(refs(&block), [sym("t_e"), sym("t_a"), sym("s_b")]);
        assert_eq!(
            block.extra["params"][0]["type"]["std::map"][2],
            json!({"ref": "s_a", "x": 1}),
            "not a reference: left alone"
        );
        assert_eq!(
            block.extra["params"][0]["type"]["std::map"][3],
            json!([{"ref": "s_c"}])
        );
    }
}
