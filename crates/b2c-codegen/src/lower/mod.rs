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
#[cfg(test)]
mod w3_tests;

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::headers::{Header, ItemHeaders};
use b2c_ir::ids::SymbolId;
use b2c_ir::layout::ModuleFiles;
use b2c_ir::sast::{
    Block, CppStandard, FunctionDef, Item, ItemKind, MainDef, Module, PassMode, Program, Symbol, SymbolKind,
    SymbolTable,
};
use b2c_ir::text::{Ident, NumLit};
use b2c_ir::types::{Type, TypeFacts, UserKind};

use crate::cast::{
    BinOp, CBlock, CExpr, CExprKind, CFunction, CGlobal, CParam, CStmt, CStmtKind, CType, CTypeDef, Init,
    ParamStyle,
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
    /// The declarations, in the declaration graph's order (06 §6.7 step 3,
    /// modules.md §4): user structs and enums with their generated
    /// functions, globals, and function declarations, each with the place
    /// it is printed in.
    pub(crate) decls: Vec<Placed<CDecl>>,
    /// User functions, sorted by name (then signature, then block ID): their
    /// definitions are printed in this order.
    pub(crate) functions: Vec<Placed<CFunction>>,
    /// `main` (normally exactly one), sorted by block ID, always at global
    /// scope.
    pub(crate) mains: Vec<CFunction>,
    /// Headers and helpers used.
    pub(crate) usage: Usage,
    /// What each item's declaration part needs (types.md §5.1), by index
    /// into `Module::items`: the standard headers and the helpers of the
    /// code a header prints for it (w3-modules.md §1.7).
    pub(crate) decl_parts: BTreeMap<usize, Usage>,
}

/// A declaration or definition with the item it comes from and the
/// namespaces it is printed in.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Placed<T> {
    /// The node.
    pub(crate) node: T,
    /// The item, by index into `Module::items`; `None` for a forward
    /// declaration of a type the module does not define.
    pub(crate) item: Option<usize>,
    /// Where it is printed.
    pub(crate) region: Region,
}

impl<T> Placed<T> {
    /// A node of no item, at global scope.
    #[cfg(test)]
    pub(crate) fn global(node: T) -> Self {
        Placed {
            node,
            item: None,
            region: Region::default(),
        }
    }
}

/// The namespaces around a declaration or definition (w3-modules.md §1.7,
/// §2.12): its user namespaces, outermost first, and, inside them, the
/// anonymous namespace that keeps a non-shared definition of a
/// multi-module project to its own file. Empty for the global scope, which
/// every single-module program uses until namespaces arrive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Region {
    /// The user namespaces, outermost first.
    pub(crate) namespaces: Vec<Ident>,
    /// Whether it is inside the anonymous namespace.
    pub(crate) anonymous: bool,
}

impl Region {
    /// The region of an item: its namespaces, and the anonymous namespace
    /// when it is not shared in a project of several modules.
    pub(crate) fn of(item: &Item, symbols: &SymbolTable, multi_module: bool) -> Region {
        Region {
            namespaces: symbols
                .namespace_path(item.namespace.as_ref())
                .into_iter()
                .filter_map(|id| symbols.get(id))
                .map(|symbol| symbol.name.clone())
                .collect(),
            anonymous: multi_module && !item.shared,
        }
    }
}

/// One declaration of a module, in printing order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum CDecl {
    /// A struct or enum (or a struct's forward declaration).
    Type(CTypeDef),
    /// A global variable or constant (w2.md §2.7).
    Global(CGlobal),
    /// The declaration of `functions[i]`.
    Function(usize),
}

/// Desugars one module of a program, whose files are `files` (`None` when
/// the layout has no entry for it). `multi_module` puts the module's
/// non-shared definitions into the anonymous namespace (w3-modules.md §1.7).
pub(crate) fn lower_module(
    program: &Program,
    module: &Module,
    files: Option<&ModuleFiles>,
    multi_module: bool,
) -> LoweredModule {
    let mut lowerer = Lowerer::new(program);
    let mut mains = Vec::new();
    // The declarations in the declaration graph's order, which the analysis
    // puts in the module's layout (modules.md §4). A type or global the
    // layout does not list comes first, in item order, and a function it
    // does not list last.
    let listed = files.map_or(&[][..], |files| files.source_order.as_slice());
    let region = |index: usize| Region::of(&module.items[index], &program.symbols, multi_module);
    let seen: BTreeSet<usize> = listed.iter().copied().collect();
    let is_function = |index: &usize| matches!(module.items[*index].kind, ItemKind::Function(_));
    let unlisted = || (0..module.items.len()).filter(|index| !seen.contains(index));
    let order: Vec<usize> = unlisted()
        .filter(|index| !is_function(index))
        .chain(listed.iter().copied().filter(|index| *index < module.items.len()))
        .collect();
    let uses = scan::enum_uses(program);
    let mut printed = BTreeSet::new();
    // The structs this module defines, which a function's declaration may
    // name before their definitions, by symbol and by name.
    let (structs, struct_items) = module_structs(program, module);
    let place = |node: CDecl, index: usize| Placed {
        node,
        item: Some(index),
        region: region(index),
    };
    // A forward declaration is printed where the struct it declares is.
    let place_type = |def: CTypeDef, own: usize| {
        let item = match &def {
            CTypeDef::Forward(name) => struct_items.get(name.as_str()).copied(),
            CTypeDef::Struct(_) | CTypeDef::Enum(_) => Some(own),
        };
        Placed {
            node: CDecl::Type(def),
            item,
            region: item.map(region).unwrap_or_default(),
        }
    };
    let mut decls = Vec::new();
    let mut decl_parts = BTreeMap::new();
    // Functions by item, sorted for their definitions below.
    let mut functions: Vec<(usize, CFunction)> = Vec::new();
    for index in order {
        let item = &module.items[index];
        match &item.kind {
            ItemKind::Struct(_) | ItemKind::Enum(_) => {
                let defs = lowerer.type_item(item, &uses, &mut printed);
                decls.extend(defs.into_iter().map(|def| place_type(def, index)));
            }
            ItemKind::Global(def) => decls.extend(
                lowerer
                    .global_item(def, item)
                    .map(|global| place(CDecl::Global(global), index)),
            ),
            ItemKind::Function(def) => {
                if let Some(function) = lowerer.function(def, item) {
                    // A struct the declaration names before its definition
                    // is printed needs a forward declaration (modules.md §4).
                    let forwards: Vec<CTypeDef> =
                        lowerer.signature_forwards(def, &structs, &mut printed).collect();
                    decls.extend(forwards.into_iter().map(|def| place_type(def, index)));
                    decls.push(place(CDecl::Function(index), index));
                    functions.push((index, function));
                }
            }
            // A used header is merged into the includes; `main` comes last.
            ItemKind::UseHeader { header } => lowerer.use_header(*header, item),
            ItemKind::Main(def) => mains.push(lowerer.main(def, item)),
        }
        decl_parts.insert(index, std::mem::take(&mut lowerer.decl_part));
    }
    // Functions the layout does not list: declared last, in definition
    // order.
    let mut rest: Vec<(usize, CFunction, Usage)> = unlisted()
        .filter(is_function)
        .filter_map(|index| match &module.items[index].kind {
            ItemKind::Function(def) => lowerer
                .function(def, &module.items[index])
                .map(|f| (index, f, std::mem::take(&mut lowerer.decl_part))),
            _ => None,
        })
        .collect();
    rest.sort_by_cached_key(|(_, f, _)| (f.name.clone(), signature_key(f), f.origin.block.clone()));
    for (index, function, decl_part) in rest {
        decl_parts.insert(index, decl_part);
        if let ItemKind::Function(def) = &module.items[index].kind {
            let forwards: Vec<CTypeDef> = lowerer.signature_forwards(def, &structs, &mut printed).collect();
            decls.extend(forwards.into_iter().map(|def| place_type(def, index)));
        }
        decls.push(place(CDecl::Function(index), index));
        functions.push((index, function));
    }
    sort_definitions(&mut functions, &mut decls);
    mains.sort_by(|a, b| a.origin.block.cmp(&b.origin.block));
    LoweredModule {
        decls,
        functions: functions
            .into_iter()
            .map(|(index, node)| Placed {
                node,
                item: Some(index),
                region: region(index),
            })
            .collect(),
        mains,
        usage: lowerer.usage,
        decl_parts,
    }
}

/// Sorts the function definitions (by name, then signature, then block ID:
/// definition order never depends on canvas position, spec §6.7) and points
/// each function declaration at its definition's new place.
fn sort_definitions(functions: &mut [(usize, CFunction)], decls: &mut [Placed<CDecl>]) {
    functions.sort_by_cached_key(|(_, f)| (f.name.clone(), signature_key(f), f.origin.block.clone()));
    let position: BTreeMap<usize, usize> = functions
        .iter()
        .enumerate()
        .map(|(at, (index, _))| (*index, at))
        .collect();
    for decl in decls {
        if let CDecl::Function(index) = &mut decl.node {
            *index = position.get(index).copied().unwrap_or(usize::MAX);
        }
    }
}

/// The structs a module defines: their symbols, and by name (the first one
/// for duplicates) each one's item index.
fn module_structs(program: &Program, module: &Module) -> (BTreeSet<SymbolId>, BTreeMap<String, usize>) {
    let mut symbols = BTreeSet::new();
    let mut by_name = BTreeMap::new();
    for (index, item) in module.items.iter().enumerate() {
        let ItemKind::Struct(def) = &item.kind else {
            continue;
        };
        symbols.insert(def.symbol.clone());
        if let Some(symbol) = program.symbols.get(&def.symbol) {
            by_name.entry(symbol.name.as_str().to_owned()).or_insert(index);
        }
    }
    (symbols, by_name)
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
                ParamStyle::PointerArray => 3,
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
    /// The names that more than one function has (overloads, w2.md §2.8),
    /// whose calls cast their arguments to the parameters' types.
    overloaded: BTreeSet<&'a str>,
    /// Names of every global in the program (visible in their module, and a
    /// fresh name must not hide one).
    global_names: BTreeSet<&'a str>,
    /// Headers and helpers used so far.
    usage: Usage,
    /// The part of the current item that the code being lowered is printed
    /// in.
    part: ItemPart,
    /// The headers the current item's own code needs so far, by part.
    item_headers: ItemHeaders,
    /// The helpers the current item's declaration part calls so far.
    decl_helpers: BTreeSet<Helper>,
    /// What the declaration part of the item finished last needs.
    decl_part: Usage,
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
        let mut function_names = BTreeSet::new();
        let mut overloaded = BTreeSet::new();
        let global_names = program
            .symbols
            .symbols
            .values()
            .filter(|symbol| matches!(symbol.kind, SymbolKind::Global { .. }))
            .map(|symbol| symbol.name.as_str())
            .collect();
        for symbol in program.symbols.symbols.values() {
            if matches!(symbol.kind, SymbolKind::Function { .. })
                && !function_names.insert(symbol.name.as_str())
            {
                overloaded.insert(symbol.name.as_str());
            }
        }
        Lowerer {
            symbols: &program.symbols,
            standard: program.standard,
            function_names,
            overloaded,
            global_names,
            usage: Usage::default(),
            part: ItemPart::Body,
            item_headers: ItemHeaders::default(),
            decl_helpers: BTreeSet::new(),
            decl_part: Usage::default(),
            in_main: false,
            scopes: Vec::new(),
            depth: 0,
        }
    }

    /// The names of the user namespaces an item is in, outermost first.
    fn namespace_names(&self, item: &Item) -> Vec<Ident> {
        Region::of(item, self.symbols, false).namespaces
    }

    /// Looks up a symbol.
    fn symbol(&self, id: &SymbolId) -> Option<&'a Symbol> {
        self.symbols.get(id)
    }

    /// Looks up a symbol that holds a value (variable, parameter, loop
    /// counter or global).
    fn variable(&self, id: &SymbolId) -> Option<&'a Symbol> {
        self.symbol(id).filter(|symbol| {
            matches!(
                symbol.kind,
                SymbolKind::Variable { .. }
                    | SymbolKind::Parameter { .. }
                    | SymbolKind::LoopVariable
                    | SymbolKind::Global { .. }
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
        if self.part == ItemPart::Decl {
            self.decl_helpers.insert(helper);
        }
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
        self.decl_helpers.clear();
    }

    /// Continues with the code printed only with the item's definition.
    fn start_body(&mut self) {
        self.part = ItemPart::Body;
    }

    /// Ends an item that is printed: the headers of both its parts, the
    /// generator's and the catalog's (`Item::headers`), go where the parts
    /// are printed. The module's `.cpp` prints the whole item, so both parts
    /// go there; the declaration part's are also kept apart
    /// (`decl_part`), for a header that declares the item.
    fn finish_item(&mut self, catalog: &ItemHeaders) {
        let own = std::mem::take(&mut self.item_headers);
        self.decl_part = Usage {
            includes: own.decl.iter().chain(&catalog.decl).copied().collect(),
            helpers: std::mem::take(&mut self.decl_helpers),
        };
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

    /// Every name visible at the current point: functions, globals,
    /// parameters, and variables and counters of the enclosing scopes
    /// declared so far.
    fn visible_names(&self) -> BTreeSet<String> {
        let mut names: BTreeSet<String> = self
            .function_names
            .iter()
            .chain(&self.global_names)
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
        let (params, args) = def
            .args
            .as_ref()
            .and_then(|args| self.main_args(args, &def.body, item))
            .unzip();
        let mut body = self.block(&def.body);
        self.in_main = false;
        if let Some(args) = args {
            body.stmts.insert(0, args);
        }
        if !matches!(body.stmts.last().map(|s| &s.kind), Some(CStmtKind::Return(_))) {
            body.stmts
                .push(CStmt::new(CStmtKind::Return(Some(int_literal(0)))));
        }
        self.finish_item(&item.headers);
        CFunction {
            ret: CType::Int,
            name: Ident::main(),
            params: params.unwrap_or_default(),
            body,
            origin: item.origin.clone(),
            comment: item.comment.clone(),
        }
    }

    /// `main`'s parameters with arguments (w2.md §1.9), `int argc, char*
    /// argv[]`, and the statement that makes the list of them, `const
    /// std::vector<std::string> args(argv, argv + argc);`. `argc` and `argv`
    /// take other names when the body uses those.
    fn main_args(&mut self, args: &SymbolId, body: &Block, item: &Item) -> Option<(Vec<CParam>, CStmt)> {
        let symbol = self.variable(args)?;
        let mut taken = self.visible_names();
        taken.extend(scan::names_mentioned(self.symbols, body, &[]));
        taken.insert(symbol.name.as_str().to_owned());
        let count = scan::fresh_name(&["argc"], "argc", &taken);
        taken.insert(count.as_str().to_owned());
        let values = scan::fresh_name(&["argv"], "argv", &taken);
        let param = |ty: CType, style: ParamStyle, name: Ident| CParam {
            ty,
            style,
            name,
            origin: item.origin.clone(),
            default: None,
        };
        let params = vec![
            param(CType::Int, ParamStyle::Value, count.clone()),
            param(CType::Char, ParamStyle::PointerArray, values.clone()),
        ];
        let ty = self.ctype(&symbol.ty);
        self.note_type(&ty);
        let end = CExpr::binary(BinOp::Add, name_expr(values.clone()), name_expr(count));
        self.declare(symbol.name.clone());
        let statement = CStmt::new(CStmtKind::Decl {
            is_const: true,
            ty,
            name: symbol.name.clone(),
            init: Init::Args(vec![name_expr(values), end]),
        });
        Some((params, statement))
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
            // A default value is printed with the declaration (w2.md §2.8).
            let default = param.default.as_ref().map(|value| self.expr(value));
            params.push(CParam {
                ty,
                style,
                name: param_symbol.name.clone(),
                origin: param.origin.clone(),
                default,
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

    /// The forward declarations a function's declaration needs: one for each
    /// struct of the module (`structs`) its parameters or result name (at
    /// any depth) that is not in `printed` yet, which then holds it. The declaration graph puts a
    /// function before such a struct only when a default value of the
    /// struct's fields needs the function (modules.md §4.1).
    fn signature_forwards(
        &self,
        def: &FunctionDef,
        structs: &BTreeSet<SymbolId>,
        printed: &mut BTreeSet<SymbolId>,
    ) -> impl Iterator<Item = CTypeDef> + use<> {
        let symbols = self.symbols;
        let mut types: Vec<&Type> = def
            .params
            .iter()
            .filter_map(|param| symbols.get(&param.symbol))
            .map(|param| &param.ty)
            .collect();
        types.push(&def.ret);
        let mut forwards = Vec::new();
        while let Some(ty) = types.pop() {
            match ty {
                Type::Named(id) => {
                    if structs.contains(id)
                        && let Some(symbol) = symbols.get(id)
                        && printed.insert(id.clone())
                    {
                        forwards.push(CTypeDef::Forward(symbol.name.clone()));
                    }
                }
                Type::Lib(app) => types.extend(app.args()),
                _ => {}
            }
        }
        forwards.into_iter()
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
