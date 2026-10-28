//! Which files each module gets (ADR-0019; the shapes are those of
//! `docs/plans/m3-contracts/modules.md` §1).
//!
//! The analyser computes a program's [`Layout`] and code generation only
//! prints it. Until shared definitions arrive, every program has the flat
//! layout of milestone M2 ([`Layout::flat`]): one `.cpp` per module, no
//! header and no includes. Code generation does not read it yet.

use std::collections::BTreeSet;

use crate::ids::{ModuleId, SymbolId};
use crate::sast::{ItemKind, Module, PassMode, SymbolTable};
use crate::text::FileStem;
use crate::types::{Type, TypeFacts, UserKind};

/// The files of a program's modules, what they include and in which order
/// their declarations are printed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layout {
    /// One per module, in project order.
    pub modules: Vec<ModuleFiles>,
}

/// The files of one module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleFiles {
    /// The module.
    pub module: ModuleId,
    /// `FileStem::new(name)`. For a name it refuses (only the `b2c` prefix
    /// gets past the loader), `module<N>` with N the module's place in the
    /// project from 1, plus `_2`, `_3`, … while that clashes. That name is
    /// only ever shown next to `E0221` (types.md §6), which blocks Build,
    /// Run and Export.
    pub stem: FileStem,
    /// Whether `<stem>.hpp` exists.
    pub header: bool,
    /// Modules whose headers `<stem>.hpp` includes, sorted by stem.
    pub header_includes: Vec<ModuleId>,
    /// Other modules' types that `<stem>.hpp` forward-declares, sorted by
    /// namespace, then name.
    pub forward: Vec<SymbolId>,
    /// Modules whose headers `<stem>.cpp` includes, its own among them,
    /// sorted by stem.
    pub source_includes: Vec<ModuleId>,
    /// Step 3 of 06 §6.7 for `<stem>.cpp`: indices into `Module.items`, in
    /// the order their declarations are printed. `main` is not listed.
    pub source_order: Vec<usize>,
    /// The same for `<stem>.hpp`: the shared items.
    pub header_order: Vec<usize>,
}

impl Layout {
    /// One `.cpp` per module, no header, no includes; `source_order` lists
    /// the function items as code generation sorts them today (by name, then
    /// signature, then block ID), leaving out a function whose symbol is
    /// missing, which code generation does not print. `symbols` gives the
    /// names and parameter types that order needs.
    pub fn flat(modules: &[Module], symbols: &SymbolTable) -> Layout {
        let mut used: BTreeSet<String> = modules
            .iter()
            .filter_map(|module| FileStem::new(&module.name).ok())
            .map(|stem| stem.as_str().to_owned())
            .collect();
        let modules = modules
            .iter()
            .enumerate()
            .map(|(place, module)| ModuleFiles {
                module: module.id.clone(),
                stem: FileStem::new(&module.name).unwrap_or_else(|_| fallback_stem(place + 1, &mut used)),
                header: false,
                header_includes: Vec::new(),
                forward: Vec::new(),
                source_includes: Vec::new(),
                source_order: function_order(module, symbols),
                header_order: Vec::new(),
            })
            .collect();
        Layout { modules }
    }

    /// Whether the program has more than one module.
    pub fn multi_module(&self) -> bool {
        self.modules.len() > 1
    }
}

/// `module<N>`, then `module<N>_2`, `module<N>_3`, … until it clashes with
/// no stem in `used`, which it joins.
fn fallback_stem(place: usize, used: &mut BTreeSet<String>) -> FileStem {
    let base = format!("module{place}");
    let mut candidate = base.clone();
    let mut suffix = 2;
    loop {
        if !used.contains(&candidate)
            && let Ok(stem) = FileStem::new(&candidate)
        {
            used.insert(candidate);
            return stem;
        }
        candidate = format!("{base}_{suffix}");
        suffix += 1;
    }
}

/// The module's function items in code generation's order: by name, then
/// the parameters' C++ types and passing styles, then block ID.
fn function_order(module: &Module, symbols: &SymbolTable) -> Vec<usize> {
    let mut functions: Vec<_> = module
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let ItemKind::Function(def) = &item.kind else {
                return None;
            };
            let name = symbols.get(&def.symbol)?.name.as_str();
            let signature: Vec<(String, u8)> = def
                .params
                .iter()
                .filter_map(|param| {
                    let ty = &symbols.get(&param.symbol)?.ty;
                    let style = match param.mode {
                        PassMode::Editable => 1,
                        PassMode::ReadOnly if by_const_ref(ty, symbols) => 2,
                        PassMode::Copy | PassMode::ReadOnly => 0,
                    };
                    Some((value_spelling(ty, symbols), style))
                })
                .collect();
            Some(((name, signature, &item.origin.block), index))
        })
        .collect();
    functions.sort();
    functions.into_iter().map(|(_, index)| index).collect()
}

/// How code generation spells a value of this type in a parameter list: in
/// full, user types by their name (`std::vector<Point>`); `void` and
/// `Error` print as an error placeholder there.
fn value_spelling(ty: &Type, symbols: &SymbolTable) -> String {
    match ty {
        Type::Void | Type::Error => String::from("int /* error */"),
        _ => ty.cpp(symbols).to_string(),
    }
}

/// Whether code generation passes a read-only parameter of this type as
/// `const T&`: every type that is not a scalar (types.md §5.1), so
/// `std::string`, the library types and structs, but not enums.
fn by_const_ref(ty: &Type, symbols: &SymbolTable) -> bool {
    match ty {
        Type::String | Type::Lib(_) => true,
        Type::Named(id) => symbols
            .user_type(id)
            .is_some_and(|user| user.kind == UserKind::Struct),
        Type::Void | Type::Bool | Type::Char | Type::Int | Type::Double | Type::Error => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diag::Part;
    use crate::headers::ItemHeaders;
    use crate::ids::BlockId;
    use crate::sast::{Block, FunctionDef, Item, MainDef, Origin, Param, Symbol, SymbolKind};
    use crate::text::Ident;

    fn sym(id: &str) -> SymbolId {
        SymbolId::new(id).unwrap()
    }

    fn origin(block: &str) -> Origin {
        Origin::whole(ModuleId::new("m").unwrap(), BlockId::new(block).unwrap())
    }

    fn item(kind: ItemKind, block: &str) -> Item {
        Item {
            kind,
            origin: origin(block),
            comment: None,
            headers: ItemHeaders::default(),
            shared: false,
            namespace: None,
        }
    }

    fn function(symbol: &str, params: &[(&str, PassMode)], block: &str) -> Item {
        item(
            ItemKind::Function(FunctionDef {
                symbol: sym(symbol),
                params: params
                    .iter()
                    .map(|(p, mode)| Param {
                        symbol: sym(p),
                        mode: *mode,
                        origin: Origin {
                            part: Part::Whole,
                            ..origin(block)
                        },
                        default: None,
                    })
                    .collect(),
                ret: Type::Void,
                body: Block::default(),
            }),
            block,
        )
    }

    fn module(id: &str, name: &str, items: Vec<Item>) -> Module {
        Module {
            id: ModuleId::new(id).unwrap(),
            name: name.to_owned(),
            items,
        }
    }

    fn table(entries: &[(&str, &str, Type)]) -> SymbolTable {
        let mut table = SymbolTable::default();
        for (id, name, ty) in entries {
            table.symbols.insert(
                sym(id),
                Symbol {
                    name: Ident::new(name).unwrap(),
                    kind: SymbolKind::Variable { is_const: false },
                    ty: ty.clone(),
                    origin: origin("b"),
                },
            );
        }
        table
    }

    #[test]
    fn flat_layout_lists_functions_in_codegen_order() {
        let symbols = table(&[
            ("f_b", "beta", Type::Void),
            ("f_a1", "alpha", Type::Void),
            ("f_a2", "alpha", Type::Void),
            ("p_int", "x", Type::Int),
            ("p_str", "s", Type::String),
        ]);
        let main = module(
            "mod_main",
            "main",
            vec![
                item(
                    ItemKind::Main(MainDef {
                        body: Block::default(),
                        args: None,
                    }),
                    "b_main",
                ),
                function("f_b", &[], "b1"),
                function("f_a1", &[("p_str", PassMode::ReadOnly)], "b2"),
                function("f_a2", &[("p_int", PassMode::Copy)], "b3"),
                function("f_missing", &[], "b4"),
            ],
        );
        let layout = Layout::flat(&[main], &symbols);
        assert!(!layout.multi_module());
        let files = &layout.modules[0];
        assert_eq!(files.stem.as_str(), "main");
        assert_eq!(files.module.as_str(), "mod_main");
        // alpha(int) before alpha(const std::string&), then beta; main and
        // the function without a symbol are not listed.
        assert_eq!(files.source_order, [3, 2, 1]);
        assert!(!files.header);
        assert!(files.header_includes.is_empty() && files.forward.is_empty());
        assert!(files.source_includes.is_empty() && files.header_order.is_empty());
    }

    #[test]
    fn refused_names_get_numbered_stems() {
        let symbols = SymbolTable::default();
        let modules = [
            module("m1", "main", Vec::new()),
            module("m2", "b2c_tools", Vec::new()),
            module("m3", "module2", Vec::new()),
            module("m4", "module2_2", Vec::new()),
            module("m5", "B2Cx", Vec::new()),
        ];
        let layout = Layout::flat(&modules, &symbols);
        assert!(layout.multi_module());
        let stems: Vec<_> = layout.modules.iter().map(|m| m.stem.as_str()).collect();
        assert_eq!(stems, ["main", "module2_3", "module2", "module2_2", "module5"]);
        assert_eq!(Layout::default().modules.len(), 0);
    }
}
