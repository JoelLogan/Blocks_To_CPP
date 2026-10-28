//! How references to namespace-scope entities are written (06 §6.14.2,
//! `docs/plans/m3-contracts/w3-modules.md` §2.12).
//!
//! One lookup model in the analyser decides, at the place where a reference
//! is printed, whether it is written plainly (`area`), from the global
//! namespace (`::area`), qualified (`geo::area`), or, for a standard name, in
//! full (`std::max`). Code generation prints what the analyser recorded and
//! never decides a spelling itself.
//!
//! The spellings live in a table beside the Semantic AST, [`Spellings`],
//! rather than in its nodes: a reference's spelling depends on the includes
//! of its file, which the analyser knows only once every module is lowered
//! and the layout is computed, so it is filled in by a pass after the
//! layout. A reference is identified by the block that holds it and the
//! symbol it names; within one block every reference to one symbol is at the
//! same place for lookup, so it has one spelling. Anything not in the table
//! has the default spelling: [`Spelling::Plain`] for the program's own
//! entities and [`Spelling::Full`] for standard names, which is exactly what
//! code generation printed before namespaces existed, so an empty table
//! changes no output.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::ids::{BlockId, ModuleId, SymbolId};

/// How one reference is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Spelling {
    /// The name alone (`area`, `cout`): lookup at that place finds exactly
    /// the intended entity. The default for the program's own entities.
    #[default]
    Plain,
    /// `::area`: an entity of the global namespace or of the module's
    /// anonymous namespace that plain lookup would not find alone.
    Global,
    /// `geo::area`: a member of a user namespace, named with its enclosing
    /// namespaces.
    Qualified,
    /// A standard name in full (`std::max`). The default for standard
    /// names.
    Full,
}

/// The spellings of a program that differ from the defaults.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Spellings {
    /// The program's own entities written other than plainly, by the block
    /// that holds the reference and the symbol it names.
    pub references: BTreeMap<(BlockId, SymbolId), Spelling>,
    /// For each module, the standard names its `.cpp` writes short
    /// (`cout` for `std::cout`), under a directive (06 §6.14.5).
    pub short_standard: BTreeMap<ModuleId, BTreeSet<String>>,
}

impl Spellings {
    /// How the reference to `symbol` in `block` is written:
    /// [`Spelling::Plain`] unless the table says otherwise.
    pub fn reference(&self, block: &BlockId, symbol: &SymbolId) -> Spelling {
        self.references
            .get(&(block.clone(), symbol.clone()))
            .copied()
            .unwrap_or_default()
    }

    /// How the standard name `name` (without `std::`) is written in the
    /// `.cpp` of `module`: [`Spelling::Full`] unless the table lists it as
    /// short, then [`Spelling::Plain`].
    pub fn standard(&self, module: &ModuleId, name: &str) -> Spelling {
        if self
            .short_standard
            .get(module)
            .is_some_and(|names| names.contains(name))
        {
            Spelling::Plain
        } else {
            Spelling::Full
        }
    }

    /// Whether every reference has its default spelling.
    pub fn is_empty(&self) -> bool {
        self.references.is_empty() && self.short_standard.values().all(BTreeSet::is_empty)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: &str) -> BlockId {
        BlockId::new(id).unwrap()
    }

    fn sym(id: &str) -> SymbolId {
        SymbolId::new(id).unwrap()
    }

    #[test]
    fn an_empty_table_gives_the_default_spellings() {
        let spellings = Spellings::default();
        assert!(spellings.is_empty());
        assert_eq!(spellings.reference(&block("b1"), &sym("f_area")), Spelling::Plain);
        let main = ModuleId::new("mod_main").unwrap();
        assert_eq!(spellings.standard(&main, "cout"), Spelling::Full);
        assert_eq!(Spelling::default(), Spelling::Plain);
    }

    #[test]
    fn recorded_spellings_are_found_by_block_and_symbol() {
        let mut spellings = Spellings::default();
        spellings
            .references
            .insert((block("b1"), sym("f_area")), Spelling::Qualified);
        spellings
            .references
            .insert((block("b2"), sym("f_max")), Spelling::Global);
        let main = ModuleId::new("mod_main").unwrap();
        spellings
            .short_standard
            .entry(main.clone())
            .or_default()
            .insert(String::from("cout"));
        assert!(!spellings.is_empty());
        assert_eq!(
            spellings.reference(&block("b1"), &sym("f_area")),
            Spelling::Qualified
        );
        assert_eq!(spellings.reference(&block("b2"), &sym("f_max")), Spelling::Global);
        // The same symbol in another block, and another symbol in the block.
        assert_eq!(spellings.reference(&block("b2"), &sym("f_area")), Spelling::Plain);
        assert_eq!(spellings.reference(&block("b1"), &sym("f_max")), Spelling::Plain);
        assert_eq!(spellings.standard(&main, "cout"), Spelling::Plain);
        assert_eq!(spellings.standard(&main, "endl"), Spelling::Full);
        let other = ModuleId::new("mod_utils").unwrap();
        assert_eq!(spellings.standard(&other, "cout"), Spelling::Full);
    }

    #[test]
    fn spellings_serialise_as_snake_case_words() {
        let words: Vec<String> = [
            Spelling::Plain,
            Spelling::Global,
            Spelling::Qualified,
            Spelling::Full,
        ]
        .iter()
        .map(|spelling| serde_json::to_string(spelling).unwrap())
        .collect();
        assert_eq!(words, ["\"plain\"", "\"global\"", "\"qualified\"", "\"full\""]);
    }
}
