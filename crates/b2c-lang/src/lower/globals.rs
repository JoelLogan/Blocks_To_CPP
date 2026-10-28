//! Lowering the globals and *flip* of W2 (`docs/plans/m3-contracts/w2.md`
//! §1.9, §2.7): package C2.
//!
//! A global (`var.global`) is declared before any function, so that every
//! parameter, default value and body of its module sees it whatever its
//! canvas position, and a local or a parameter with its name is
//! `B2C-W0501`. Its starting value is lowered once every function is
//! declared, before any body, and checked by L10 (`consts.rs`) in an order
//! where a constant's value is known before another global's starting value
//! reads it: a constant's value then folds wherever it is used, a `case`
//! label included. A constant of type `bool`, `char`, `int` or `double`
//! whose value is a constant expression is printed `constexpr`, any other
//! one `const`. A constant needs a value (`B2C-E0430`) and can't be changed
//! (`B2C-E0308`). The declaration graph (`order.rs`) puts the globals in
//! order, and reports a circle of them (`B2C-E0421`).

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::diag::Part;
use b2c_ir::ids::{BlockId, SymbolId};
use b2c_ir::sast::{Expr, ExprKind, GlobalDef, Item, ItemKind, StmtKind, Symbol, SymbolKind};
use b2c_ir::sig::PartType;
use b2c_ir::types::Type;
use b2c_model::Block;

use super::convert::Subject;
use super::{ItemCtx, Lowerer, TypeUse, field, sorted_blocks};
use crate::messages::quoted;
use crate::{access, codes, consts, flow};

/// A global that [`Lowerer::declare_globals`] put in the table, for
/// [`Lowerer::lower_globals`].
pub(super) struct DeclaredGlobal<'d> {
    /// The defining module's index.
    module: usize,
    /// The `var.global` block.
    block: &'d Block,
    /// Its symbol.
    sym: SymbolId,
}

impl<'d> Lowerer<'d> {
    /// Declares every enabled global of the program, after the user types
    /// (a global may hold one) and before the functions. Its name is unique
    /// among the program's globals, structs, enums and functions
    /// (`B2C-E0210`): every module is still compiled on its own, and two
    /// definitions with one name would not link or would be ambiguous.
    pub(super) fn declare_globals(&mut self) -> Vec<DeclaredGlobal<'d>> {
        let document = self.document;
        let mut names: BTreeMap<String, SymbolId> = BTreeMap::new();
        let mut declared = Vec::new();
        for (index, module) in document.modules.iter().enumerate() {
            self.enter_module(index, &module.id);
            for block in sorted_blocks(module) {
                if block.disabled || block.block_type != "var.global" {
                    continue;
                }
                if let Some(sym) = self.declare_global(block, &mut names) {
                    declared.push(DeclaredGlobal {
                        module: index,
                        block,
                        sym,
                    });
                }
            }
        }
        declared
    }

    /// Declares one global's symbol. Returns it when it is in the table.
    fn declare_global(&mut self, block: &Block, names: &mut BTreeMap<String, SymbolId>) -> Option<SymbolId> {
        let name_loc = self.loc(&block.id, field("NAME"));
        let is_const = self.choice(block, "KIND", false, &[("variable", false), ("constant", true)]);
        let ty = self.type_setting(block, "TYPE", "int", TypeUse::Variable);
        // Read for its damage only: one module shares nothing (modules.md §2).
        let _ = self.checkbox(block, "SHARED", false);
        let Ok(decl) = access::decl_field(block, "NAME") else {
            self.incomplete(name_loc, "This global needs a name.");
            return None;
        };
        let ident = self.ident(&decl.name, &name_loc, true);
        self.check_definition_name(&decl.name, &name_loc, names);
        let symbol = Symbol {
            name: ident,
            kind: SymbolKind::Global { is_const },
            ty,
            origin: self.origin(&block.id, field("NAME")),
        };
        if !self.insert_symbol(&decl.sym, &decl.name, symbol, name_loc) {
            return None;
        }
        names.entry(decl.name.clone()).or_insert_with(|| decl.sym.clone());
        let module = self.module_index;
        if let Some(globals) = self.module_globals.get_mut(module) {
            globals
                .entry(decl.name.clone())
                .or_insert_with(|| decl.sym.clone());
        }
        let globals = self.module_globals.get(module).cloned().unwrap_or_default();
        self.scopes.set_globals(&globals);
        Some(decl.sym.clone())
    }

    /// Reports a global named like another global, a struct, an enum or a
    /// function of the program (`B2C-E0210`).
    fn check_definition_name(
        &mut self,
        name: &str,
        location: &b2c_ir::diag::Location,
        names: &BTreeMap<String, SymbolId>,
    ) {
        if let Some(other) = names.get(name).cloned() {
            let message = format!(
                "There is already a global called {}. Rename one of them.",
                quoted(name)
            );
            self.error_with_decl(
                codes::DUPLICATE_NAME,
                location.clone(),
                message,
                &other,
                "the other global",
            );
            return;
        }
        let user_type = self
            .symbols
            .symbols
            .iter()
            .find(|(_, symbol)| {
                matches!(symbol.kind, SymbolKind::Struct { .. } | SymbolKind::Enum { .. })
                    && symbol.name.as_str() == name
            })
            .map(|(sym, _)| sym.clone());
        let function = self
            .module_functions
            .iter()
            .find_map(|functions| functions.get(name))
            .cloned();
        let (other, what, note) = match (user_type, function) {
            (Some(other), _) => (other, "a struct or enum", "the type"),
            (None, Some(other)) => (other, "a function", "the function"),
            (None, None) => return,
        };
        let message = format!(
            "There is already {what} called {}, and C++ would not know which of the two the name means. Rename \
             the global or the other one.",
            quoted(name)
        );
        self.error_with_decl(codes::DUPLICATE_NAME, location.clone(), message, &other, note);
    }

    /// Lowers the globals' starting values, then checks them (L10) in an
    /// order where a constant is checked, and its value recorded, before a
    /// global whose starting value reads it; globals that read each other in
    /// a circle (`B2C-E0421`, reported by the declaration graph) are checked
    /// in block order. The items wait in `global_items` for their module.
    pub(super) fn lower_globals(&mut self, globals: &[DeclaredGlobal<'d>]) {
        let document = self.document;
        let mut modules: BTreeMap<BlockId, usize> = BTreeMap::new();
        for global in globals {
            self.enter_module(global.module, &document.modules[global.module].id);
            if let Some(item) = self.global_item(global.block, &global.sym) {
                modules.insert(global.block.id.clone(), global.module);
                self.global_items.insert(global.block.id.clone(), item);
            }
        }
        let constant = |lowerer: &Self, sym: &SymbolId| {
            matches!(
                lowerer.symbols.get(sym).map(|symbol| &symbol.kind),
                Some(SymbolKind::Global { is_const: true })
            )
        };
        let mut waiting: Vec<BlockId> = self.global_items.keys().cloned().collect();
        while !waiting.is_empty() {
            let unchecked: BTreeSet<&SymbolId> = waiting
                .iter()
                .filter_map(|id| match self.global_items.get(id).map(|item| &item.kind) {
                    Some(ItemKind::Global(def)) if constant(self, &def.symbol) => Some(&def.symbol),
                    _ => None,
                })
                .collect();
            let ready = waiting.iter().position(|id| {
                let Some(ItemKind::Global(def)) = self.global_items.get(id).map(|item| &item.kind) else {
                    return true;
                };
                reads(def.init.as_ref())
                    .iter()
                    .all(|sym| *sym == &def.symbol || !unchecked.contains(sym))
            });
            let id = waiting.remove(ready.unwrap_or(0));
            let module = modules.get(&id).copied().unwrap_or(0);
            self.enter_module(module, &document.modules[module].id);
            self.check_global(&id);
        }
    }

    /// Lowers one global's starting value into its item.
    fn global_item(&mut self, block: &Block, sym: &SymbolId) -> Option<Item> {
        let symbol = self.symbols.get(sym)?.clone();
        let SymbolKind::Global { is_const } = symbol.kind else {
            return None;
        };
        let name = self.raw_name(sym);
        self.begin_item(ItemCtx::Main);
        self.record_block(&block.id);
        self.add_headers(block, true);
        self.record_input(&block.id, "VALUE");
        // `where = { T = "value" }`, and `T` on `TYPE`, `NAME` and `VALUE`,
        // as on *create* (slots.md §5).
        let mut typed = self.typed(self.block_sig(&block.block_type));
        typed.field("TYPE", PartType::Known(symbol.ty.clone()));
        self.typed_input(block, &mut typed, "VALUE", false);
        let typing = self.type_parts(block, &mut typed, None);
        let init = typed.take("VALUE");
        self.report_field_fit(block, &typed, &typing, "TYPE");
        if let Some(value) = &init {
            self.report_fit(&typed, &typing, "VALUE", value, &Subject::Variable(name.clone()));
        } else if is_const {
            let location = self.loc(
                &block.id,
                Part::Input {
                    name: String::from("VALUE"),
                },
            );
            self.incomplete(
                location,
                format!(
                    "{} is a constant, so it needs a value: give it one, or choose 'variable'.",
                    quoted(&name)
                ),
            );
        }
        Some(Item {
            kind: ItemKind::Global(GlobalDef {
                symbol: sym.clone(),
                init,
                constexpr: false,
            }),
            origin: self.origin(&block.id, Part::Whole),
            comment: self.comment(block),
            headers: std::mem::take(&mut self.headers),
            shared: false,
        })
    }

    /// Checks a lowered global (nesting, flow, L10), recording a constant's
    /// value, and marks a constant `constexpr` when C++ allows it: a
    /// `bool`, `char`, `int` or `double` whose value is a constant
    /// expression (w2.md §2.7).
    fn check_global(&mut self, id: &BlockId) {
        let Some(mut item) = self.global_items.remove(id) else {
            return;
        };
        if self.check_nesting(&item) {
            flow::check_item(&item, &self.symbols, &mut self.diags);
            consts::check_item(
                &item,
                &self.symbols,
                self.unchecked,
                &mut self.diags,
                &mut self.constants,
            );
        }
        if let ItemKind::Global(def) = &mut item.kind {
            let scalar = self.symbols.get(&def.symbol).is_some_and(|symbol| {
                matches!(symbol.ty, Type::Bool | Type::Char | Type::Int | Type::Double)
            });
            def.constexpr = scalar
                && self
                    .constants
                    .get(&def.symbol)
                    .is_some_and(|constant| constant.constant_expression);
        }
        self.global_items.insert(id.clone(), item);
    }

    /// *flip*.
    #[allow(clippy::unused_self, reason = "a stub until package C2 lowers the blocks")]
    pub(super) fn global_statement(&mut self, block: &Block) -> Option<StmtKind> {
        let _ = block;
        None
    }
}

/// The symbols an expression reads, at any depth.
fn reads(expr: Option<&Expr>) -> BTreeSet<&SymbolId> {
    let mut found = BTreeSet::new();
    let mut stack: Vec<&Expr> = expr.into_iter().collect();
    while let Some(expr) = stack.pop() {
        if let ExprKind::Var(sym) = &expr.kind {
            found.insert(sym);
        }
        stack.extend(flow::children(expr));
    }
    found
}
