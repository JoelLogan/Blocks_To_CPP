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
//!
//! *flip* turns a `bool` variable that may change to its other value.

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
        // The structs and enums by name, the first by symbol for a name used
        // twice: looked up for each global, not searched for.
        let mut type_names: BTreeMap<String, SymbolId> = BTreeMap::new();
        for (sym, symbol) in &self.symbols.symbols {
            if matches!(symbol.kind, SymbolKind::Struct { .. } | SymbolKind::Enum { .. }) {
                type_names
                    .entry(symbol.name.as_str().to_owned())
                    .or_insert_with(|| sym.clone());
            }
        }
        let mut declared = Vec::new();
        for (index, module) in document.modules.iter().enumerate() {
            self.enter_module(index, &module.id);
            for block in sorted_blocks(module) {
                if block.disabled || block.block_type != "var.global" {
                    continue;
                }
                if let Some(sym) = self.declare_global(block, &mut names, &type_names) {
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
    fn declare_global(
        &mut self,
        block: &Block,
        names: &mut BTreeMap<String, SymbolId>,
        type_names: &BTreeMap<String, SymbolId>,
    ) -> Option<SymbolId> {
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
        self.check_definition_name(&decl.name, &name_loc, names, type_names);
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
        // The scopes hold this module's globals (`enter_module`): one more,
        // not a copy of them all.
        self.scopes.add_global(&decl.name, &decl.sym);
        Some(decl.sym.clone())
    }

    /// Reports a global named like another global, a struct, an enum or a
    /// function of the program (`B2C-E0210`).
    fn check_definition_name(
        &mut self,
        name: &str,
        location: &b2c_ir::diag::Location,
        names: &BTreeMap<String, SymbolId>,
        type_names: &BTreeMap<String, SymbolId>,
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
        let user_type = type_names.get(name).cloned();
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
        // Each waiting global, in block order, with the constants of the
        // program it reads and that are not checked yet (itself left out).
        // Checking one makes ready each global that waits for it alone; a
        // circle (no global ready) goes on with the first in block order.
        let order: Vec<BlockId> = self.global_items.keys().cloned().collect();
        let constants: BTreeMap<&SymbolId, usize> = order
            .iter()
            .enumerate()
            .filter_map(
                |(index, id)| match self.global_items.get(id).map(|item| &item.kind) {
                    Some(ItemKind::Global(def))
                        if matches!(
                            self.symbols.get(&def.symbol).map(|symbol| &symbol.kind),
                            Some(SymbolKind::Global { is_const: true })
                        ) =>
                    {
                        Some((&def.symbol, index))
                    }
                    _ => None,
                },
            )
            .collect();
        let mut waits = vec![0usize; order.len()];
        let mut waiting_for: Vec<Vec<usize>> = vec![Vec::new(); order.len()];
        for (index, id) in order.iter().enumerate() {
            let Some(ItemKind::Global(def)) = self.global_items.get(id).map(|item| &item.kind) else {
                continue;
            };
            for sym in reads(def.init.as_ref()) {
                if let Some(&constant) = constants.get(sym)
                    && constant != index
                {
                    waits[index] += 1;
                    waiting_for[constant].push(index);
                }
            }
        }
        let mut ready: BTreeSet<usize> = (0..order.len()).filter(|&index| waits[index] == 0).collect();
        let mut left: BTreeSet<usize> = (0..order.len()).collect();
        while let Some(index) = ready.pop_first().or_else(|| left.first().copied()) {
            left.remove(&index);
            ready.remove(&index);
            for &other in &waiting_for[index] {
                waits[other] -= 1;
                if waits[other] == 0 && left.contains(&other) {
                    ready.insert(other);
                }
            }
            let id = &order[index];
            let module = modules.get(id).copied().unwrap_or(0);
            self.enter_module(module, &document.modules[module].id);
            self.check_global(id);
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

    /// *flip* (`done = !done;`): a `bool` variable that may change.
    pub(super) fn global_statement(&mut self, block: &Block) -> Option<StmtKind> {
        let (target, ty) = self.target(block)?;
        if let Some(ty) = ty.filter(|ty| *ty != Type::Bool && *ty != Type::Error)
            && self.checking()
        {
            let message = format!(
                "Only a true/false variable can be flipped, but {} holds {}.",
                self.name_of(&target),
                crate::messages::a_type(&ty, &self.symbols)
            );
            let location = self.loc(&block.id, field("VAR"));
            self.diags.error(codes::CONVERSION, location, message);
        }
        Some(StmtKind::Flip { target })
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
