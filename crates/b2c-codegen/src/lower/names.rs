//! How code generation writes names (w3-modules.md §2.12, 06 §6.14):
//! each reference to one of the program's own namespace-scope entities as
//! the analyser spelled it (`area`, `::area`, `geo::area`), and standard
//! names without `std::` where a `.cpp`'s `using namespace std;` allows it.
//!
//! The analyser decides every spelling (`Program::spellings`); this module
//! only looks it up at the place that prints the reference. That place is
//! the block that holds the reference: the expression's own block for a
//! call, a global read, an enum value or a struct value (and for the user
//! types of the expression's value), the statement's block for the
//! variable a statement assigns to, the declaring block for the type of a
//! local, parameter, field or global, and the function's block for its
//! result type.
//!
//! Standard names the analyser lists as short for a module
//! (`Spellings::short_standard`) are written short only after the `.cpp`'s
//! `using namespace std;` and never under the names of code generation's
//! own locals and parameters ([`is_generated`]), which a short name would
//! meet in the same scopes.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::ids::{BlockId, ModuleId, SymbolId};
use b2c_ir::sast::{Item, ItemKind, Program, Symbol, SymbolKind};
use b2c_ir::spelling::Spelling;
use b2c_ir::text::Ident;
use b2c_ir::types::Type;

use super::Lowerer;
use crate::cast::{CName, CType};

/// The stems of the names code generation gives its own locals and
/// parameters (`scan::fresh_name`): each alone or followed by a number
/// (`i`, `i2`); the others start with `b2c_` (`b2c_file`, `b2c_name`).
pub(super) const GENERATED_STEMS: &[&str] = &[
    "argc", "argv", "end", "i", "j", "k", "lhs", "n", "out", "rhs", "step", "value",
];

/// Whether `name` may be a name code generation gives one of its own locals
/// or parameters.
pub(crate) fn is_generated(name: &str) -> bool {
    name.starts_with("b2c_")
        || GENERATED_STEMS.iter().any(|stem| {
            name.strip_prefix(stem)
                .is_some_and(|rest| rest.bytes().all(|byte| byte.is_ascii_digit()))
        })
}

/// `text` with `std::` left out before each name of `names`: `std::cout <<
/// x` becomes `cout << x`, `std::numeric_limits<std::streamsize>::max()`
/// becomes `numeric_limits<streamsize>::max()`. Only a `std::` that starts
/// a qualified name counts (not `b2c::std::`, which code generation never
/// writes anyway).
pub(crate) fn shorten<'t>(text: &'t str, names: &BTreeSet<String>) -> Cow<'t, str> {
    const STD: &str = "std::";
    if !text.contains(STD) {
        return Cow::Borrowed(text);
    }
    let is_ident = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut at = 0;
    while let Some(found) = text[at..].find(STD) {
        let start = at + found;
        let name_start = start + STD.len();
        let name_end = text[name_start..]
            .bytes()
            .position(|byte| !is_ident(byte))
            .map_or(text.len(), |len| name_start + len);
        let starts_name = start == 0 || !(is_ident(bytes[start - 1]) || bytes[start - 1] == b':');
        if starts_name && names.contains(&text[name_start..name_end]) {
            out.push_str(&text[copied..start]);
            copied = name_start;
        }
        at = name_start;
    }
    out.push_str(&text[copied..]);
    Cow::Owned(out)
}

/// The standard names the `.cpp` of `module` writes short: those the
/// analyser lists for it but the names of code generation's own locals.
pub(crate) fn short_standard(program: &Program, module: &ModuleId) -> BTreeSet<String> {
    program
        .spellings
        .short_standard
        .get(module)
        .map(|names| names.iter().filter(|name| !is_generated(name)).cloned().collect())
        .unwrap_or_default()
}

/// The symbol an item defines, for the namespace-scope definitions.
fn item_symbol(item: &Item) -> Option<&SymbolId> {
    match &item.kind {
        ItemKind::Function(def) => Some(&def.symbol),
        ItemKind::Struct(def) => Some(&def.symbol),
        ItemKind::Enum(def) => Some(&def.symbol),
        ItemKind::Global(def) => Some(&def.symbol),
        ItemKind::Main(_) | ItemKind::UseHeader { .. } => None,
    }
}

/// The names of the user namespaces of each definition of the program
/// that is in one, outermost first: what a qualified reference writes
/// before its name.
pub(super) fn definition_scopes(program: &Program) -> BTreeMap<&SymbolId, Vec<Ident>> {
    program
        .modules
        .iter()
        .flat_map(|module| &module.items)
        .filter(|item| item.namespace.is_some())
        .filter_map(|item| {
            let scope: Vec<Ident> = program
                .symbols
                .namespace_path(item.namespace.as_ref())
                .into_iter()
                .filter_map(|id| program.symbols.get(id))
                .map(|symbol| symbol.name.clone())
                .collect();
            item_symbol(item).map(|symbol| (symbol, scope))
        })
        .collect()
}

impl Lowerer<'_> {
    /// The definition `symbol`, called `name`, as the reference to it in
    /// `block` writes it.
    pub(super) fn reference(&self, block: Option<&BlockId>, symbol: &SymbolId, name: &Ident) -> CName {
        let spelling = block.map_or(Spelling::Plain, |block| self.spellings.reference(block, symbol));
        let (scope, global) = match spelling {
            Spelling::Plain | Spelling::Full => return CName::plain(name.clone()),
            Spelling::Global => (Vec::new(), true),
            Spelling::Qualified | Spelling::GlobalQualified => {
                let scope = self.scopes_of.get(symbol).cloned().unwrap_or_default();
                let global = spelling == Spelling::GlobalQualified || scope.is_empty();
                (scope, global)
            }
        };
        CName {
            global,
            scope,
            name: name.clone(),
        }
    }

    /// The definition `symbol`, called `name`, as a reference in the block
    /// being lowered writes it.
    pub(super) fn reference_at(&self, symbol: &SymbolId, name: &Ident) -> CName {
        self.reference(self.at.as_ref(), symbol, name)
    }

    /// A variable's name where the block being lowered refers to it: a
    /// global as the analyser spelled it, any other variable plainly.
    pub(super) fn variable_name(&self, id: &SymbolId, symbol: &Symbol) -> CName {
        if matches!(symbol.kind, SymbolKind::Global { .. }) {
            self.reference_at(id, &symbol.name)
        } else {
            CName::plain(symbol.name.clone())
        }
    }

    /// The C++ type of a value of type `ty`, its user types as the block
    /// `block` writes them (`None`: plainly).
    pub(super) fn ctype_in(&self, ty: &Type, block: Option<&BlockId>) -> CType {
        CType::value_with(ty, self.symbols, &|id, name| self.reference(block, id, name))
    }

    /// The C++ type of a function result (`void` allowed), as the
    /// function's block writes it.
    pub(super) fn result_type(&self, ty: &Type, block: &BlockId) -> CType {
        CType::result_with(ty, self.symbols, &|id, name| {
            self.reference(Some(block), id, name)
        })
    }

    /// The C++ type of a local, parameter, field or global, as its
    /// declaration writes it.
    pub(super) fn declared_type(&self, symbol: &Symbol) -> CType {
        self.ctype_in(&symbol.ty, Some(&symbol.origin.block))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn shorten_leaves_out_std_before_listed_names_only() {
        let short = names(&["cout", "numeric_limits", "streamsize", "string"]);
        assert_eq!(shorten("std::cout", &short), "cout");
        assert_eq!(
            shorten("std::numeric_limits<std::streamsize>::max()", &short),
            "numeric_limits<streamsize>::max()"
        );
        assert_eq!(shorten("std::string::npos", &short), "string::npos");
        assert_eq!(shorten("std::cerr", &short), "std::cerr");
        assert_eq!(shorten("std::couter", &short), "std::couter");
        assert_eq!(shorten("b2c::std::cout", &short), "b2c::std::cout");
        assert_eq!(shorten("const std::string&", &short), "const string&");
        assert_eq!(shorten("int", &short), "int");
        assert_eq!(shorten("std::", &short), "std::");
    }

    #[test]
    fn generated_names_are_recognised_with_their_numbers() {
        for name in [
            "i",
            "j",
            "k",
            "i2",
            "n12",
            "end",
            "end3",
            "lhs",
            "value",
            "b2c_name",
            "b2c_file2",
        ] {
            assert!(is_generated(name), "{name}");
        }
        for name in ["count", "endl", "index", "ends", "max", "i_"] {
            assert!(!is_generated(name), "{name}");
        }
    }
}
