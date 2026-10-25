//! What the editor asks about types and scopes besides the preview: the
//! analyser's conversion rule as a table, and the symbols visible where
//! blocks are pasted.

use b2c_ir::{BlockId, ModuleId, SymbolInfo, SymbolInfoKind, Type};
use b2c_lang::{Analysis, Conversion, conversion};
use b2c_model::Block;
use serde::Serialize;

/// Every static type, in the order of the analyser's own tables (`b2c_ir::Type`
/// has no list of its variants).
pub const STATIC_TYPES: [Type; 7] = [
    Type::Void,
    Type::Bool,
    Type::Char,
    Type::Int,
    Type::Double,
    Type::String,
    Type::Error,
];

/// One entry of [`conversion_table`]:
/// `{"from": "int", "to": "double", "conversion": "widening"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConversionRow {
    /// The type of the value.
    pub from: Type,
    /// The type of the place it goes to.
    pub to: Type,
    /// How the analyser treats it, by [`Conversion::name`]: `same`,
    /// `widening`, `narrowing`, `boolNumber` or `invalid` (only `invalid`
    /// is an analyser error).
    #[serde(serialize_with = "conversion_name")]
    pub conversion: Conversion,
}

#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's serialize_with passes a reference"
)]
fn conversion_name<S: serde::Serializer>(conversion: &Conversion, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(conversion.name())
}

/// [`b2c_lang::conversion`] for every pair of static types: 49 rows, `from`
/// in [`STATIC_TYPES`] order, and for each `from` every `to` in the same
/// order. The editor's connection checker refuses exactly the `invalid`
/// pairs, so it can never disagree with the analyser.
pub fn conversion_table() -> Vec<ConversionRow> {
    STATIC_TYPES
        .iter()
        .flat_map(|from| {
            STATIC_TYPES.iter().map(move |to| ConversionRow {
                from: from.clone(),
                to: to.clone(),
                conversion: conversion(from, to),
            })
        })
        .collect()
}

/// The functions of one module: what is visible everywhere in it, sorted by
/// name, then ID.
fn module_functions(analysis: &Analysis, module: &ModuleId) -> Vec<SymbolInfo> {
    analysis
        .symbol_infos()
        .into_iter()
        .filter(|info| info.module == *module && matches!(info.kind, SymbolInfoKind::Function { .. }))
        .collect()
}

/// The symbols visible where blocks are pasted, for re-binding references
/// (05 §5.12), from the scope query (06 §6.5):
///
/// * `block` `None`: the module's canvas, where pasted blocks are parked:
///   the parked scope ([`Analysis::parked_scope`], what is visible at the
///   end of the module's `main` body, or its functions alone without one).
/// * `block` with an `input`: what [`Analysis::scope_at`] gives there (the
///   start of that statement list, or inside that value input).
/// * `block` without an input: directly after the block in its statement
///   list ([`Analysis::scope_after`]): what is visible at the block plus the
///   variable the block itself creates, if the analyser accepted it.
///
/// So a paste into or after a parked block has the parked scope too, and a
/// paste into or after a block inside a disabled statement has that
/// statement's scope. A block the scope query has no answer for (one nested
/// too deeply) sees the module's functions, which are visible everywhere in
/// it.
pub(crate) fn visible_at(
    analysis: &Analysis,
    module: &ModuleId,
    block: Option<&BlockId>,
    input: Option<&str>,
) -> Vec<SymbolInfo> {
    let Some(block) = block else {
        return analysis.parked_scope(module);
    };
    let answer = match input {
        Some(input) => analysis.scope_at(block, Some(input)),
        None => analysis.scope_after(block),
    };
    answer.unwrap_or_else(|| module_functions(analysis, module))
}

/// Finds a block by ID in one module's canvas (any depth, stacks included).
pub(crate) fn find_block<'a>(blocks: &'a [Block], id: &BlockId) -> Option<&'a Block> {
    let mut found = None;
    crate::tree::walk_pruned(blocks, |block| {
        if found.is_some() {
            return false;
        }
        if block.id == *id {
            found = Some(block);
            return false;
        }
        true
    });
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_covers_every_pair_in_order() {
        let table = conversion_table();
        assert_eq!(table.len(), 49);
        for (index, row) in table.iter().enumerate() {
            assert_eq!(Some(&row.from), STATIC_TYPES.get(index / 7));
            assert_eq!(Some(&row.to), STATIC_TYPES.get(index % 7));
            assert_eq!(row.conversion, conversion(&row.from, &row.to));
        }
        let json = serde_json::to_string(table.first().unwrap()).unwrap();
        assert_eq!(json, r#"{"from":"void","to":"void","conversion":"same"}"#);
        let int_to_double = table
            .iter()
            .find(|row| row.from == Type::Int && row.to == Type::Double)
            .unwrap();
        assert_eq!(
            serde_json::to_value(int_to_double).unwrap(),
            serde_json::json!({"from": "int", "to": "double", "conversion": "widening"})
        );
    }

    #[test]
    fn every_type_is_listed_once() {
        // Fails to compile when a type is added, so the list stays complete.
        // The library and user types join the table with `sample_types()`
        // (types.md §3.2), when they serialise.
        let position = |ty: &Type| match ty {
            Type::Void => 0,
            Type::Bool => 1,
            Type::Char => 2,
            Type::Int => 3,
            Type::Double => 4,
            Type::String => 5,
            Type::Error => 6,
            Type::Lib(_) | Type::Named(_) => usize::MAX,
        };
        for (index, ty) in STATIC_TYPES.iter().enumerate() {
            assert_eq!(position(ty), index);
        }
    }
}
