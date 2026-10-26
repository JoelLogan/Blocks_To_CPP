//! Desugaring: the Semantic AST to the C++ AST (spec §6.8.1).
//!
//! This is where language-level constructs become concrete C++: `repeat`
//! gets a counter with a fresh readable name, `print` becomes a `<<` chain,
//! `ask` becomes a support-helper call or plain stream input, `join` becomes
//! `std::string` concatenation, and so on. It also records which headers and
//! support helpers the module uses.
//!
//! Headers are recorded per item and per item part (types.md §5.1): those of
//! the code printed with an item's declaration (a function's signature, a
//! type's definition) apart from those of the code printed only with its
//! definition (a function's body), next to the catalog's
//! [`ItemHeaders`] from the SAST. A module's `.cpp` prints both parts of
//! every item, so both reach its includes; ADR-0019's headers (milestone M3's
//! third wave) put a shared item's declaration part in its module's `.hpp`.
//!
//! Best effort on broken input: an expression of [`Type::Error`], or one
//! that refers to a symbol that does not exist, becomes `0 /* error */`; a
//! statement that cannot be generated becomes `/* error */;`. Nothing here
//! panics, and recursion stops at [`scan::MAX_DEPTH`].

mod collections;
mod control;
mod errors;
mod expr;
mod files;
mod forms;
mod globals;
mod scan;
mod stmt;
#[cfg(test)]
mod tests;
mod text;
mod typedefs;
#[cfg(test)]
mod w2_tests;

use std::collections::BTreeSet;

use b2c_ir::headers::{Header, ItemHeaders};
use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{
    CppStandard, FunctionDef, Item, ItemKind, MainDef, Module, PassMode, Program, Symbol, SymbolKind,
    SymbolTable,
};
use b2c_ir::text::{Ident, NumLit};
use b2c_ir::types::{Type, TypeFacts, UserKind};

use crate::cast::{
    CBlock, CExpr, CExprKind, CFunction, CParam, CStmt, CStmtKind, CType, CTypeDef, ParamStyle,
};
use crate::helpers::Helper;

/// Headers and helpers a module's `.cpp` uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Usage {
    /// Standard headers the module's own code needs (`<iostream>`, …).
    pub(crate) includes: BTreeSet<Header>,
    /// Support helpers the module calls.
    pub(crate) helpers: BTreeSet<Helper>,
}

/// A module, desugared and ordered for printing.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoweredModule {
    /// User structs and enums with their generated functions, printed before
    /// the function declarations. None before W2's definition blocks.
    pub(crate) types: Vec<CTypeDef>,
    /// User functions, sorted by name (then signature, then block ID).
    pub(crate) functions: Vec<CFunction>,
    /// `main` (normally exactly one), sorted by block ID.
    pub(crate) mains: Vec<CFunction>,
    /// Headers and helpers used.
    pub(crate) usage: Usage,
}

/// Desugars one module of a program.
pub(crate) fn lower_module(program: &Program, module: &Module) -> LoweredModule {
    let mut lowerer = Lowerer::new(program);
    let mut functions = Vec::new();
    let mut mains = Vec::new();
    let mut types = Vec::new();
    // Structs and enums in the declaration graph's order (modules.md §4),
    // which the analysis puts in the module's layout; any the layout does
    // not list come after, in item order.
    let listed = program
        .layout
        .modules
        .iter()
        .find(|files| files.module == module.id)
        .map_or(&[][..], |files| files.source_order.as_slice());
    let is_type = |index: &usize| {
        module
            .items
            .get(*index)
            .is_some_and(|item| matches!(item.kind, ItemKind::Struct(_) | ItemKind::Enum(_)))
    };
    let mut type_order: Vec<usize> = listed.iter().copied().filter(is_type).collect();
    let seen: BTreeSet<usize> = type_order.iter().copied().collect();
    let unlisted: Vec<usize> = (0..module.items.len())
        .filter(|index| is_type(index) && !seen.contains(index))
        .collect();
    type_order.extend(unlisted);
    if !type_order.is_empty() {
        let uses = scan::enum_uses(program);
        let mut printed = BTreeSet::new();
        for index in type_order {
            types.extend(lowerer.type_item(&module.items[index], &uses, &mut printed));
        }
    }
    for item in &module.items {
        match &item.kind {
            ItemKind::Main(def) => mains.push(lowerer.main(def, item)),
            ItemKind::Function(def) => functions.extend(lowerer.function(def, item)),
            // W2's definitions (w2.md §3.2): structs and enums are C1's,
            // lowered above, globals C2's, a used header D's.
            ItemKind::Struct(_) | ItemKind::Enum(_) => {}
            ItemKind::Global(def) => lowerer.global_item(def, item),
            ItemKind::UseHeader { header } => lowerer.use_header(*header, item),
        }
    }
    // Definition order never depends on canvas position (spec §6.7).
    functions.sort_by_cached_key(|f| (f.name.clone(), signature_key(f), f.origin.block.clone()));
    mains.sort_by(|a, b| a.origin.block.cmp(&b.origin.block));
    LoweredModule {
        types,
        functions,
        mains,
        usage: lowerer.usage,
    }
}

/// Orders overloads (functions with the same name) by their parameter types:
/// each type's C++ spelling, then how it is passed. `b2c_ir`'s
/// `Layout::flat` lists functions in the same order.
fn signature_key(function: &CFunction) -> Vec<(String, u8)> {
    function
        .params
        .iter()
        .map(|p| {
            let style = match p.style {
                ParamStyle::Value => 0,
                ParamStyle::Ref => 1,
                ParamStyle::ConstRef => 2,
            };
            (p.ty.sort_key(), style)
        })
        .collect()
}

/// Which part of an item the code being lowered is printed in (types.md
/// §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemPart {
    /// With the declaration: a function's signature, a type's definition.
    Decl,
    /// Only with the definition: a function's body.
    Body,
}

/// Desugaring state for one module.
struct Lowerer<'a> {
    /// The program's symbols.
    symbols: &'a SymbolTable,
    /// The standard the program is generated for.
    standard: CppStandard,
    /// Names of every function in the program (visible everywhere).
    function_names: BTreeSet<&'a str>,
    /// Headers and helpers used so far.
    usage: Usage,
    /// The part of the current item that the code being lowered is printed
    /// in.
    part: ItemPart,
    /// The headers the current item's own code needs so far, by part.
    item_headers: ItemHeaders,
    /// Whether the code being lowered is in `main`.
    in_main: bool,
    /// Names declared in the enclosing scopes of the current function,
    /// innermost last (parameters, variables, loop counters).
    scopes: Vec<Vec<Ident>>,
    /// Current statement and expression nesting.
    depth: usize,
}

impl<'a> Lowerer<'a> {
    /// A lowerer for the modules of `program`.
    fn new(program: &'a Program) -> Self {
        let function_names = program
            .symbols
            .symbols
            .values()
            .filter(|symbol| matches!(symbol.kind, SymbolKind::Function { .. }))
            .map(|symbol| symbol.name.as_str())
            .collect();
        Lowerer {
            symbols: &program.symbols,
            standard: program.standard,
            function_names,
            usage: Usage::default(),
            part: ItemPart::Body,
            item_headers: ItemHeaders::default(),
            in_main: false,
            scopes: Vec::new(),
            depth: 0,
        }
    }

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

    /// The C++ type of a value of a static type.
    fn ctype(&self, ty: &Type) -> CType {
        CType::value(ty, self.symbols)
    }

    /// Whether values of a static type are scalars (numbers, characters,
    /// `bool` and enums), which a read-only parameter takes by value: `const
    /// int&` gains nothing over a copy. Strings, library types and structs
    /// are taken as `const T&` (types.md §5.1).
    fn is_scalar_value(&self, ty: &Type) -> bool {
        is_scalar(ty) || self.is_enum(ty)
    }

    /// Whether a static type is a user enum.
    fn is_enum(&self, ty: &Type) -> bool {
        match ty {
            Type::Named(id) => self
                .symbols
                .user_type(id)
                .is_some_and(|user| user.kind == UserKind::Enum),
            _ => false,
        }
    }

    /// The headers of the part of the current item being lowered.
    fn part_headers(&mut self) -> &mut BTreeSet<Header> {
        match self.part {
            ItemPart::Decl => &mut self.item_headers.decl,
            ItemPart::Body => &mut self.item_headers.body,
        }
    }

    /// Records that a standard header is needed, by its name (`"<string>"`).
    /// The generator names only headers of the standard table (a test checks
    /// every one).
    fn include(&mut self, name: &'static str) {
        if let Some(header) = Header::standard(name) {
            self.part_headers().insert(header);
        }
    }

    /// Records that a support helper is called.
    fn use_helper(&mut self, helper: Helper) {
        self.usage.helpers.insert(helper);
    }

    /// Records the headers a type the generator prints needs: `<string>`,
    /// each library type's header and those of its arguments (types.md
    /// §5.2).
    fn note_type(&mut self, ty: &CType) {
        ty.collect_headers(self.part_headers());
    }

    /// Records the headers of a value's type when code uses the value
    /// through its type's members or operators (types.md §5.2), such as a
    /// returned `std::string` that is printed: its type is named nowhere in
    /// this module, but the code needs its header.
    fn note_value_type(&mut self, ty: &Type) {
        let ty = CType::result(ty, self.symbols);
        self.note_type(&ty);
    }

    /// Starts lowering an item: its code is printed with its declaration
    /// until [`Lowerer::start_body`].
    fn start_item(&mut self) {
        self.part = ItemPart::Decl;
        self.item_headers = ItemHeaders::default();
    }

    /// Continues with the code printed only with the item's definition.
    fn start_body(&mut self) {
        self.part = ItemPart::Body;
    }

    /// Ends an item that is printed: the headers of both its parts, the
    /// generator's and the catalog's (`Item::headers`), go where the parts
    /// are printed. The module's `.cpp` prints the whole item, so both parts
    /// go there.
    fn finish_item(&mut self, catalog: &ItemHeaders) {
        let own = std::mem::take(&mut self.item_headers);
        let includes = &mut self.usage.includes;
        includes.extend(own.decl);
        includes.extend(own.body);
        includes.extend(catalog.decl.iter().copied());
        includes.extend(catalog.body.iter().copied());
        self.part = ItemPart::Body;
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
        self.start_item();
        self.start_body();
        self.in_main = true;
        self.scopes = vec![Vec::new()];
        let mut body = self.block(&def.body);
        self.in_main = false;
        if !matches!(body.stmts.last().map(|s| &s.kind), Some(CStmtKind::Return(_))) {
            body.stmts
                .push(CStmt::new(CStmtKind::Return(Some(int_literal(0)))));
        }
        self.finish_item(&item.headers);
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
        self.start_item();
        let ret = CType::result(&def.ret, self.symbols);
        self.note_type(&ret);
        let mut params = Vec::new();
        for param in &def.params {
            let Some(param_symbol) = self.symbol(&param.symbol) else {
                continue;
            };
            let ty = self.ctype(&param_symbol.ty);
            self.note_type(&ty);
            let style = match param.mode {
                PassMode::Editable => ParamStyle::Ref,
                PassMode::ReadOnly if ty != CType::Error && !self.is_scalar_value(&param_symbol.ty) => {
                    ParamStyle::ConstRef
                }
                PassMode::Copy | PassMode::ReadOnly => ParamStyle::Value,
            };
            params.push(CParam {
                ty,
                style,
                name: param_symbol.name.clone(),
                origin: param.origin.clone(),
            });
        }
        self.start_body();
        self.in_main = false;
        self.scopes = vec![params.iter().map(|p| p.name.clone()).collect()];
        let body = self.block(&def.body);
        self.finish_item(&item.headers);
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
