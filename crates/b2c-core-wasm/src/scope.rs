//! What the editor asks about types and scopes besides the preview: the
//! analyser's conversion rule as a table, and the symbols visible where
//! blocks are pasted.

use b2c_ir::types::sample_types;
use b2c_ir::{BlockId, ModuleId, SymbolInfo, SymbolInfoKind, Type};
use b2c_lang::{Analysis, Conversion, conversion};
use b2c_model::Block;
use serde::Serialize;

/// M2's seven static types, in the order of the analyser's own tables: the
/// first seven of `b2c_ir::types::sample_types()`, and the only types whose
/// rows the `conversion_table` export writes until the switch to the term
/// form ([`exported_conversion_table`]).
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

/// [`b2c_lang::conversion`] for every pair of `b2c_ir::types::sample_types()`
/// (types.md §3.2): M2's seven types, then a fixed list of library and user
/// types, `from` in sample order and, for each `from`, every `to` in the same
/// order. These are the rows of the shared type fixture's `conversions`
/// (`packages/blockly-ext/src/types/fixture.json`). The editor's connection
/// checker refuses exactly the `invalid` pairs, so it can never disagree
/// with the analyser.
pub fn conversion_table() -> Vec<ConversionRow> {
    let samples = sample_types();
    samples
        .iter()
        .flat_map(|from| {
            samples.iter().map(move |to| ConversionRow {
                from: from.clone(),
                to: to.clone(),
                conversion: conversion(from, to),
            })
        })
        .collect()
}

/// The rows of [`conversion_table`] that the `conversion_table` export
/// writes. Until the switch to the term form (types.md §8.2), `Type`
/// serialises only M2's seven types, so these are the 49 rows between
/// [`STATIC_TYPES`], in the table's order; the switch makes the export
/// write the whole table.
pub fn exported_conversion_table() -> Vec<ConversionRow> {
    conversion_table()
        .into_iter()
        .filter(|row| STATIC_TYPES.contains(&row.from) && STATIC_TYPES.contains(&row.to))
        .collect()
}

/// What is visible everywhere in one module, from every symbol of the
/// program (sorted by name, then ID): its functions, structs and enums, with
/// the fields and enumerators of those structs and enums, which blocks refer
/// to through their type (types.md §2.4), as the scope query lists them.
fn module_wide(symbols: Vec<SymbolInfo>, module: &ModuleId) -> Vec<SymbolInfo> {
    symbols
        .into_iter()
        .filter(|info| {
            info.module == *module
                && matches!(
                    info.kind,
                    SymbolInfoKind::Function { .. }
                        | SymbolInfoKind::Struct { .. }
                        | SymbolInfoKind::Enum { .. }
                        | SymbolInfoKind::Field { .. }
                        | SymbolInfoKind::Enumerator { .. }
                )
        })
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
/// too deeply) sees what is visible everywhere in the module: its functions
/// and user types, with their members. Every answer lists each struct and
/// enum with its fields or enumerators, which carry their owner, so a pasted
/// member binds through its owner (`crate::clipboard`).
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
    answer.unwrap_or_else(|| module_wide(analysis.symbol_infos(), module))
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
    fn the_table_covers_every_pair_of_samples_in_order() {
        let samples = sample_types();
        let count = samples.len();
        let table = conversion_table();
        assert_eq!(table.len(), count * count);
        for (index, row) in table.iter().enumerate() {
            assert_eq!(Some(&row.from), samples.get(index / count));
            assert_eq!(Some(&row.to), samples.get(index % count));
            assert_eq!(row.conversion, conversion(&row.from, &row.to));
        }
        // Library and user types convert only to themselves (and `error`).
        let vector = samples.iter().find(|ty| matches!(ty, Type::Lib(_))).unwrap();
        for row in table.iter().filter(|row| row.from == *vector) {
            let fits = row.to == *vector || row.to == Type::Error;
            assert_eq!(row.conversion == Conversion::Same, fits, "{:?}", row.to);
            assert_eq!(row.conversion == Conversion::Invalid, !fits, "{:?}", row.to);
        }
    }

    #[test]
    fn the_export_writes_the_rows_of_m2_types_until_the_switch() {
        assert_eq!(sample_types().get(..7), Some(&STATIC_TYPES[..]));
        let table = exported_conversion_table();
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
    fn module_wide_symbols_are_definitions_and_members() {
        let module = ModuleId::new("m").unwrap();
        let other = ModuleId::new("other").unwrap();
        let info = |id: &str, kind: SymbolInfoKind, module: &ModuleId| SymbolInfo {
            id: b2c_ir::SymbolId::new(id).unwrap(),
            name: id.to_owned(),
            kind,
            ty: Type::Int,
            module: module.clone(),
            decl_block: BlockId::new("b").unwrap(),
            shared: false,
        };
        let owner = b2c_ir::SymbolId::new("s_pt").unwrap();
        let symbols = vec![
            info(
                "e_red",
                SymbolInfoKind::Enumerator { owner: owner.clone() },
                &module,
            ),
            info(
                "f_area",
                SymbolInfoKind::Function {
                    params: Vec::new(),
                    returns: Type::Int,
                },
                &module,
            ),
            info(
                "f_far",
                SymbolInfoKind::Function {
                    params: Vec::new(),
                    returns: Type::Int,
                },
                &other,
            ),
            info("f_x", SymbolInfoKind::Field { owner }, &module),
            info("s_col", SymbolInfoKind::Enum { values: Vec::new() }, &module),
            info("s_i", SymbolInfoKind::LoopVariable, &module),
            info(
                "s_pt",
                SymbolInfoKind::Struct {
                    fields: Vec::new(),
                    comparable: false,
                    printable: false,
                },
                &module,
            ),
            info("s_x", SymbolInfoKind::Variable { is_const: false }, &module),
            info(
                "p_n",
                SymbolInfoKind::Parameter {
                    mode: b2c_ir::sast::PassMode::Copy,
                },
                &module,
            ),
        ];
        let ids: Vec<String> = module_wide(symbols, &module)
            .iter()
            .map(|info| info.id.to_string())
            .collect();
        assert_eq!(ids, ["e_red", "f_area", "f_x", "s_col", "s_pt"]);
    }

    #[test]
    fn every_type_is_listed_once() {
        // Fails to compile when a type is added, so the list stays complete.
        // The library and user types are in the table through
        // `sample_types()`, and in the export once they serialise.
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
