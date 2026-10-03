//! Desugaring: the Semantic AST to the C++ AST (spec §6.8.1).
//!
//! This is where language-level constructs become concrete C++: `repeat`
//! gets a counter with a fresh readable name, `print` becomes a `<<` chain,
//! `ask` becomes a support-helper call or plain stream input, `join` becomes
//! `std::string` concatenation, and so on. It also records which headers and
//! support helpers the module uses.
//!
//! Best effort on broken input: an expression of [`Type::Error`], or one
//! that refers to a symbol that does not exist, becomes `0 /* error */`; a
//! statement that cannot be generated becomes `/* error */;`. Nothing here
//! panics, and recursion stops at [`scan::MAX_DEPTH`].

mod expr;
mod scan;
mod stmt;

use std::collections::BTreeSet;

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{
    FunctionDef, Item, ItemKind, MainDef, Module, PassMode, Program, Symbol, SymbolKind, SymbolTable,
};
use b2c_ir::text::{Ident, NumLit};
use b2c_ir::types::Type;

use crate::cast::{CBlock, CExpr, CExprKind, CFunction, CParam, CStmt, CStmtKind, CType, ParamStyle};
use crate::helpers::Helper;

/// Headers and helpers a module uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Usage {
    /// Standard headers the module's own code needs (`<iostream>`, …).
    pub(crate) includes: BTreeSet<&'static str>,
    /// Support helpers the module calls.
    pub(crate) helpers: BTreeSet<Helper>,
}

/// A module, desugared and ordered for printing.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoweredModule {
    /// User functions, sorted by name (then signature, then block ID).
    pub(crate) functions: Vec<CFunction>,
    /// `main` (normally exactly one), sorted by block ID.
    pub(crate) mains: Vec<CFunction>,
    /// Headers and helpers used.
    pub(crate) usage: Usage,
}

/// Desugars one module of a program.
pub(crate) fn lower_module(program: &Program, module: &Module) -> LoweredModule {
    let function_names = program
        .symbols
        .symbols
        .values()
        .filter(|symbol| matches!(symbol.kind, SymbolKind::Function { .. }))
        .map(|symbol| symbol.name.as_str())
        .collect();
    let mut lowerer = Lowerer {
        symbols: &program.symbols,
        function_names,
        usage: Usage::default(),
        in_main: false,
        scopes: Vec::new(),
        depth: 0,
    };
    let mut functions = Vec::new();
    let mut mains = Vec::new();
    for item in &module.items {
        match &item.kind {
            ItemKind::Main(def) => mains.push(lowerer.main(def, item)),
            ItemKind::Function(def) => functions.extend(lowerer.function(def, item)),
        }
    }
    // Definition order never depends on canvas position (spec §6.7).
    functions.sort_by_cached_key(|f| (f.name.clone(), signature_key(f), f.origin.block.clone()));
    mains.sort_by(|a, b| a.origin.block.cmp(&b.origin.block));
    LoweredModule {
        functions,
        mains,
        usage: lowerer.usage,
    }
}

/// Orders overloads (functions with the same name) by their parameter types.
fn signature_key(function: &CFunction) -> Vec<(&'static str, u8)> {
    function
        .params
        .iter()
        .map(|p| {
            let style = match p.style {
                ParamStyle::Value => 0,
                ParamStyle::Ref => 1,
                ParamStyle::ConstRef => 2,
            };
            (p.ty.spelling(), style)
        })
        .collect()
}

/// Desugaring state for one module.
struct Lowerer<'a> {
    /// The program's symbols.
    symbols: &'a SymbolTable,
    /// Names of every function in the program (visible everywhere).
    function_names: BTreeSet<&'a str>,
    /// Headers and helpers used so far.
    usage: Usage,
    /// Whether the code being lowered is in `main`.
    in_main: bool,
    /// Names declared in the enclosing scopes of the current function,
    /// innermost last (parameters, variables, loop counters).
    scopes: Vec<Vec<Ident>>,
    /// Current statement and expression nesting.
    depth: usize,
}

impl<'a> Lowerer<'a> {
    /// Looks up a symbol.
    fn symbol(&self, id: &SymbolId) -> Option<&'a Symbol> {
        self.symbols.get(id)
    }

    /// Looks up a symbol that holds a value (variable, parameter or loop counter).
    fn variable(&self, id: &SymbolId) -> Option<&'a Symbol> {
        self.symbol(id).filter(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Variable { .. } | SymbolKind::Parameter { .. } | SymbolKind::LoopVariable
            )
        })
    }

    /// Looks up a function symbol.
    fn function_symbol(&self, id: &SymbolId) -> Option<&'a Symbol> {
        self.symbol(id)
            .filter(|symbol| matches!(symbol.kind, SymbolKind::Function { .. }))
    }

    /// Records that a standard header is needed.
    fn include(&mut self, header: &'static str) {
        self.usage.includes.insert(header);
    }

    /// Records that a support helper is called.
    fn use_helper(&mut self, helper: Helper) {
        self.usage.helpers.insert(helper);
    }

    /// Records the header a declared type needs.
    fn note_type(&mut self, ty: CType) {
        if ty == CType::String {
            self.include("<string>");
        }
    }

    /// Declares a name in the innermost scope.
    fn declare(&mut self, name: Ident) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.push(name);
        }
    }

    /// Every name visible at the current point: functions, parameters, and
    /// variables and counters of the enclosing scopes declared so far.
    fn visible_names(&self) -> BTreeSet<String> {
        let mut names: BTreeSet<String> = self
            .function_names
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        names.extend(self.scopes.iter().flatten().map(|name| name.as_str().to_owned()));
        names
    }

    /// Desugars `main`, adding the implicit `return 0;`.
    fn main(&mut self, def: &MainDef, item: &Item) -> CFunction {
        self.in_main = true;
        self.scopes = vec![Vec::new()];
        let mut body = self.block(&def.body);
        self.in_main = false;
        if !matches!(body.stmts.last().map(|s| &s.kind), Some(CStmtKind::Return(_))) {
            body.stmts
                .push(CStmt::new(CStmtKind::Return(Some(int_literal(0)))));
        }
        CFunction {
            ret: CType::Int,
            name: Ident::main(),
            params: Vec::new(),
            body,
            origin: item.origin.clone(),
            comment: item.comment.clone(),
        }
    }

    /// Desugars a user function. Returns `None` if its symbol is missing.
    fn function(&mut self, def: &FunctionDef, item: &Item) -> Option<CFunction> {
        let symbol = self.symbol(&def.symbol)?;
        let ret = CType::result(&def.ret);
        self.note_type(ret);
        let mut params = Vec::new();
        for param in &def.params {
            let Some(param_symbol) = self.symbol(&param.symbol) else {
                continue;
            };
            let ty = CType::value(&param_symbol.ty);
            self.note_type(ty);
            let style = match param.mode {
                PassMode::Editable => ParamStyle::Ref,
                PassMode::ReadOnly if ty == CType::String => ParamStyle::ConstRef,
                // `const int&` gains nothing over a copy for scalars.
                PassMode::Copy | PassMode::ReadOnly => ParamStyle::Value,
            };
            params.push(CParam {
                ty,
                style,
                name: param_symbol.name.clone(),
                origin: param.origin.clone(),
            });
        }
        self.in_main = false;
        self.scopes = vec![params.iter().map(|p| p.name.clone()).collect()];
        let body = self.block(&def.body);
        Some(CFunction {
            ret,
            name: symbol.name.clone(),
            params,
            body,
            origin: item.origin.clone(),
            comment: item.comment.clone(),
        })
    }

    /// Desugars a block in a new scope.
    fn block(&mut self, block: &b2c_ir::sast::Block) -> CBlock {
        self.scoped_block(block, Vec::new())
    }

    /// Desugars a block in a new scope that starts with `names` declared
    /// (e.g. a loop counter).
    fn scoped_block(&mut self, block: &b2c_ir::sast::Block, names: Vec<Ident>) -> CBlock {
        self.scopes.push(names);
        let stmts = block.stmts.iter().map(|stmt| self.stmt(stmt)).collect();
        self.scopes.pop();
        CBlock { stmts }
    }
}

/// An `int` literal chosen by the generator. Negative values become `-n`.
fn int_literal(value: i32) -> CExpr {
    let literal = CExpr::new(CExprKind::Num(NumLit::int(value)));
    if value < 0 {
        CExpr::unary(crate::cast::UnOp::Neg, literal)
    } else {
        literal
    }
}

/// A name expression.
fn name_expr(name: Ident) -> CExpr {
    CExpr::new(CExprKind::Name(name))
}

/// The token for a boolean literal.
fn bool_token(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

/// Whether a static type is a number, character or `bool` (the targets
/// `std::cin >>` and `b2c::ask<T>` read).
fn is_scalar(ty: &Type) -> bool {
    matches!(ty, Type::Int | Type::Double | Type::Char | Type::Bool)
}
