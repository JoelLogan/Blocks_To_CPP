//! Pass 2: lowering blocks to the Semantic AST while resolving names
//! (spec §6.4–6.5) and checking types (§6.6). Pass 3 (flow checks) runs on
//! each finished item.
//!
//! The walk is in program order, so scopes are exactly those of the generated
//! C++: a variable becomes visible after its `create` block, until the end of
//! its statement list.

mod convert;
mod expr;
mod resolve;
mod stmt;

use std::collections::BTreeMap;

use b2c_ir::diag::{Location, Part};
use b2c_ir::ids::{BlockId, ModuleId, SymbolId};
use b2c_ir::sast::{
    FunctionDef, Item, ItemKind, MainDef, Module, Origin, Param, PassMode, Program, Symbol, SymbolKind,
    SymbolTable,
};
use b2c_ir::text::{Comment, Ident, IdentError};
use b2c_ir::types::Type;
use b2c_model::{Block, Document};

use crate::Analysis;
use crate::access::{self, FieldProblem};
use crate::codes;
use crate::collect::{self, DeclInfo, DeclKind, DeclStatus, MAX_BLOCK_DEPTH};
use crate::flow;
use crate::messages::{Diags, quoted};
use crate::nesting;
use crate::scope::{ListKey, PARAMS_LIST, Scopes};

/// Functions of namespace `std` that are not templates and take a
/// `std::string` first. A call `stoi(text)` of a user function with the same
/// name also finds these through argument-dependent lookup, and the call is
/// ambiguous, so functions may not have these names. (`b2c_ir::text::Ident::
/// check_namespace_scope` covers names in the global namespace only.)
const STD_TEXT_FUNCTIONS: &[&str] = &[
    "stod", "stof", "stoi", "stol", "stold", "stoll", "stoul", "stoull",
];

/// Message for a project without a `when program starts` block.
const NO_MAIN_MESSAGE: &str = "Add a 'when program starts' block: every program begins there.";

/// Analyses a document (see [`crate::analyze`]).
pub(crate) fn run(document: &Document) -> Analysis {
    let standard = document.project.language.standard;
    let Some(first) = document.modules.first() else {
        let mut diags = Diags::default();
        diags.error(codes::NO_MAIN, Location::project(), NO_MAIN_MESSAGE);
        let program = Program {
            standard,
            modules: Vec::new(),
            symbols: SymbolTable::default(),
        };
        return Analysis {
            program,
            diagnostics: diags.list,
        };
    };
    let mut lowerer = Lowerer::new(document, first.id.clone());
    let modules = lowerer.program();
    Analysis {
        program: Program {
            standard,
            modules,
            symbols: lowerer.symbols,
        },
        diagnostics: lowerer.diags.list,
    }
}

/// A function's signature, known before any body is lowered so that calls
/// can be checked in any order (definition order never matters).
#[derive(Debug, Clone)]
struct FuncSig {
    /// The user's name.
    name: String,
    /// Index of the defining module.
    module: usize,
    /// Return type.
    ret: Type,
    /// Parameters that were declared successfully.
    params: Vec<ParamSig>,
}

/// One parameter of a [`FuncSig`].
#[derive(Debug, Clone)]
struct ParamSig {
    sym: SymbolId,
    name: String,
    ty: Type,
    mode: PassMode,
    origin: Origin,
    /// Whether its symbol is in the table. A parameter whose symbol ID
    /// another block already declares (a damaged file, reported) stays in
    /// the signature with an unknown type, so that calls are not reported
    /// again, but is left out of the program.
    declared: bool,
}

/// What kind of item is being lowered.
#[derive(Debug, Clone)]
enum ItemCtx {
    /// `when program starts`.
    Main,
    /// A function.
    Function {
        /// The user's name.
        name: String,
        /// Its return type.
        ret: Type,
    },
}

/// A name that C++ already sees while its declaration is being lowered: a
/// variable inside its own starting value, or a `for` counter inside the
/// loop's start, end and step.
#[derive(Debug, Clone)]
struct Pending {
    name: String,
    sym: SymbolId,
    kind: PendingKind,
}

#[derive(Debug, Clone)]
enum PendingKind {
    /// A variable's starting value; the declared type (`None` for `auto`).
    Initialiser(Option<Type>),
    /// A `for` loop header.
    Counter,
}

/// The state of the lowering pass.
struct Lowerer<'d> {
    document: &'d Document,
    /// Every declaration in the document (pass 1).
    decls: BTreeMap<SymbolId, DeclInfo>,
    /// Active functions of each module, by name (the first one for duplicates).
    module_functions: Vec<BTreeMap<String, SymbolId>>,
    /// Signatures of the functions that were declared successfully.
    functions: BTreeMap<SymbolId, FuncSig>,
    symbols: SymbolTable,
    diags: Diags,
    scopes: Scopes,
    pending: Vec<Pending>,
    module_index: usize,
    module: ModuleId,
    item: ItemCtx,
    loop_depth: usize,
    depth: usize,
    too_deep_reported: bool,
    placeholders: usize,
}

/// A field part.
fn field(name: &str) -> Part {
    Part::Field {
        name: name.to_owned(),
    }
}

/// Top-level blocks of a module in a deterministic order (by block ID).
fn sorted_blocks(module: &b2c_model::Module) -> Vec<&Block> {
    let mut blocks: Vec<&Block> = module.workspace.blocks.iter().collect();
    blocks.sort_by(|a, b| a.id.cmp(&b.id));
    blocks
}

impl<'d> Lowerer<'d> {
    fn new(document: &'d Document, module: ModuleId) -> Self {
        let decls = collect::collect(document);
        let mut module_functions = vec![BTreeMap::new(); document.modules.len()];
        for (sym, decl) in &decls {
            if decl.kind == DeclKind::Function
                && decl.status == DeclStatus::Active
                && let Some(map) = module_functions.get_mut(decl.module)
            {
                map.entry(decl.name.clone()).or_insert_with(|| sym.clone());
            }
        }
        Self {
            document,
            decls,
            module_functions,
            functions: BTreeMap::new(),
            symbols: SymbolTable::default(),
            diags: Diags::default(),
            scopes: Scopes::default(),
            pending: Vec::new(),
            module_index: 0,
            module,
            item: ItemCtx::Main,
            loop_depth: 0,
            depth: 0,
            too_deep_reported: false,
            placeholders: 0,
        }
    }

    /// Lowers every module.
    fn program(&mut self) -> Vec<Module> {
        let document = self.document;
        // Function names are unique in the whole program: every function is
        // generated with external linkage, so two modules defining the same
        // name would not link.
        let mut names = BTreeMap::new();
        for (index, module) in document.modules.iter().enumerate() {
            self.enter_module(index, &module.id);
            for block in sorted_blocks(module) {
                if block.block_type == "func.define" && !block.disabled {
                    self.declare_function(block, &mut names);
                }
            }
        }
        let chosen_main = self.check_mains();
        let mut modules = Vec::with_capacity(document.modules.len());
        for (index, module) in document.modules.iter().enumerate() {
            self.enter_module(index, &module.id);
            let mut items = Vec::new();
            for block in sorted_blocks(module) {
                if block.disabled {
                    continue;
                }
                let (item, keep) = match block.block_type.as_str() {
                    "program.main" => (Some(self.main(block)), chosen_main.as_ref() == Some(&block.id)),
                    "func.define" => (self.function(block), true),
                    _ => (None, false),
                };
                if let Some(item) = item {
                    if self.check_nesting(&item) {
                        flow::check_item(&item, &self.symbols, &mut self.diags);
                    }
                    if keep {
                        items.push(item);
                    }
                }
            }
            modules.push(Module {
                id: module.id.clone(),
                name: module.name.clone(),
                items,
            });
        }
        modules
    }

    /// Reports an item that nests too deeply for the later stages (unless
    /// its block nesting was reported already). Returns whether it is fine.
    fn check_nesting(&mut self, item: &Item) -> bool {
        let body = match &item.kind {
            ItemKind::Main(main) => &main.body,
            ItemKind::Function(function) => &function.body,
        };
        let Some(stmt) = nesting::too_deep(body) else {
            return true;
        };
        if !self.too_deep_reported {
            self.too_deep_reported = true;
            self.diags.error(
                codes::TOO_DEEP,
                stmt.origin.location(),
                "The values in this block are nested too deeply, together with the blocks around it, to be \
                 turned into C++. Split them up using variables, or move some of the blocks into a function.",
            );
        }
        false
    }

    fn enter_module(&mut self, index: usize, id: &ModuleId) {
        self.module_index = index;
        self.module = id.clone();
    }

    /// Reports missing or extra `when program starts` blocks and returns the
    /// one that becomes the program's `main`.
    fn check_mains(&mut self) -> Option<BlockId> {
        let mut mains: Vec<(&ModuleId, &BlockId)> = Vec::new();
        let mut disabled: Option<(&ModuleId, &BlockId)> = None;
        let document = self.document;
        for module in &document.modules {
            for block in sorted_blocks(module) {
                if block.block_type == "program.main" {
                    if block.disabled {
                        disabled.get_or_insert((&module.id, &block.id));
                    } else {
                        mains.push((&module.id, &block.id));
                    }
                }
            }
        }
        let Some(&(first_module, first)) = mains.first() else {
            match disabled {
                Some((module, block)) => self.diags.error(
                    codes::NO_MAIN,
                    Location::block(Some(module.clone()), block.clone()),
                    "The 'when program starts' block is disabled. Enable it so the program has a place to begin.",
                ),
                None => self.diags.error(codes::NO_MAIN, Location::project(), NO_MAIN_MESSAGE),
            }
            return None;
        };
        let first_location = Location::block(Some(first_module.clone()), first.clone());
        for &(module, block) in mains.iter().skip(1) {
            let diagnostic = b2c_ir::Diagnostic::error(
                codes::MANY_MAINS,
                b2c_ir::DiagSource::Analyser,
                Location::block(Some(module.clone()), block.clone()),
                "There is more than one 'when program starts' block. A program can only begin in one place, \
                 so keep just one of them.",
            )
            .with_related(first_location.clone(), "the first 'when program starts' block");
            self.diags.push(diagnostic);
        }
        Some(first.clone())
    }

    /// Starts lowering a new item.
    fn begin_item(&mut self, item: ItemCtx) {
        self.item = item;
        self.scopes = Scopes::default();
        self.pending.clear();
        self.loop_depth = 0;
        self.depth = 0;
        self.too_deep_reported = false;
    }

    fn main(&mut self, block: &Block) -> Item {
        self.begin_item(ItemCtx::Main);
        let body = self.body(block, "BODY", false);
        Item {
            kind: ItemKind::Main(MainDef { body }),
            origin: self.origin(&block.id, Part::Whole),
            comment: self.comment(block),
        }
    }

    fn function(&mut self, block: &Block) -> Option<Item> {
        let symbol = access::decl_field(block, "NAME").ok()?.sym.clone();
        let sig = self.functions.get(&symbol)?.clone();
        self.begin_item(ItemCtx::Function {
            name: sig.name.clone(),
            ret: sig.ret.clone(),
        });
        self.scopes.push(ListKey::new(&block.id, PARAMS_LIST), false);
        for param in &sig.params {
            self.scopes.declare(&param.name, &param.sym);
        }
        let body = self.body(block, "BODY", true);
        self.scopes.pop();
        let params = sig
            .params
            .iter()
            .filter(|p| p.declared)
            .map(|p| Param {
                symbol: p.sym.clone(),
                mode: p.mode,
                origin: p.origin.clone(),
            })
            .collect();
        Some(Item {
            kind: ItemKind::Function(FunctionDef {
                symbol,
                params,
                ret: sig.ret,
                body,
            }),
            origin: self.origin(&block.id, Part::Whole),
            comment: self.comment(block),
        })
    }

    /// Declares a function's symbol and parameters (before any body is lowered).
    fn declare_function(&mut self, block: &Block, names: &mut BTreeMap<String, SymbolId>) {
        let name_loc = self.loc(&block.id, field("NAME"));
        let Ok(decl) = access::decl_field(block, "NAME") else {
            self.incomplete(name_loc, "This function needs a name.");
            return;
        };
        let ident = self.ident(&decl.name, &name_loc, true);
        let ret = self.type_setting(block, "RETURNS", "void", true);
        if let Some(other) = names.get(&decl.name).cloned() {
            let place = match self.decls.get(&other).map(|d| d.module) {
                Some(module) if module != self.module_index => {
                    let module = self.document.modules.get(module).map(|m| quoted(&m.name));
                    format!("in module {}", module.unwrap_or_default())
                }
                _ => String::from("in this module"),
            };
            let message = format!(
                "There is already a function called {} {place}. Functions with the same name \
                 (overloads) aren't supported yet, so rename one of them.",
                quoted(&decl.name)
            );
            self.error_with_decl(
                codes::DUPLICATE_FUNCTION,
                name_loc.clone(),
                message,
                &other,
                "the other function",
            );
        } else {
            names.insert(decl.name.clone(), decl.sym.clone());
        }
        let params = self.declare_params(block, &decl.name);
        let symbol = Symbol {
            name: ident,
            kind: SymbolKind::Function {
                params: params
                    .iter()
                    .filter(|p| p.declared)
                    .map(|p| p.sym.clone())
                    .collect(),
            },
            ty: ret.clone(),
            origin: self.origin(&block.id, field("NAME")),
        };
        if self.insert_symbol(&decl.sym, symbol, name_loc) {
            let sig = FuncSig {
                name: decl.name.clone(),
                module: self.module_index,
                ret,
                params,
            };
            self.functions.insert(decl.sym.clone(), sig);
        }
    }

    fn declare_params(&mut self, block: &Block, function: &str) -> Vec<ParamSig> {
        let mut params: Vec<ParamSig> = Vec::new();
        for (i, row) in access::param_rows(block).into_iter().enumerate() {
            let part = field(&format!("params[{i}]"));
            let loc = self.loc(&block.id, part.clone());
            let Ok(row) = row else {
                self.incomplete(
                    loc,
                    "This input of the function is damaged. Remove it and add it again.",
                );
                continue;
            };
            let ident = self.ident(&row.name, &loc, false);
            let ty = row.ty.unwrap_or_else(|text| {
                let message = format!("{} is not a type that a function input can have.", quoted(&text));
                self.incomplete(loc.clone(), message);
                Type::Error
            });
            let mode = row.mode.unwrap_or_else(|text| {
                let message = format!("{} is not a way of passing a function input.", quoted(&text));
                self.incomplete(loc.clone(), message);
                PassMode::Copy
            });
            if let Some(other) = params.iter().find(|p| p.name == row.name).map(|p| p.sym.clone()) {
                let message = format!(
                    "Two inputs of {} are called {}. Rename one of them.",
                    quoted(function),
                    quoted(&row.name)
                );
                self.error_with_decl(
                    codes::DUPLICATE_NAME,
                    loc.clone(),
                    message,
                    &other,
                    "the other input",
                );
            }
            self.warn_if_function_name(&row.name, &loc);
            let origin = self.origin(&block.id, part);
            let symbol = Symbol {
                name: ident,
                kind: SymbolKind::Parameter { mode },
                ty: ty.clone(),
                origin: origin.clone(),
            };
            let declared = self.insert_symbol(&row.sym, symbol, loc);
            params.push(ParamSig {
                sym: row.sym,
                name: row.name,
                ty: if declared { ty } else { Type::Error },
                mode,
                origin,
                declared,
            });
        }
        params
    }

    // --- Shared helpers ------------------------------------------------------

    fn origin(&self, block: &BlockId, part: Part) -> Origin {
        Origin {
            module: self.module.clone(),
            block: block.clone(),
            part,
        }
    }

    fn loc(&self, block: &BlockId, part: Part) -> Location {
        Location {
            module: Some(self.module.clone()),
            block: Some(block.clone()),
            part,
        }
    }

    /// Reports a block that lacks something the analyser needs.
    fn incomplete(&mut self, location: Location, message: impl Into<String>) {
        self.diags.error(codes::INCOMPLETE_BLOCK, location, message);
    }

    /// Reports an error with a related location at a symbol's declaration.
    fn error_with_decl(
        &mut self,
        code: &str,
        location: Location,
        message: String,
        other: &SymbolId,
        note: &str,
    ) {
        let mut diagnostic = b2c_ir::Diagnostic::error(code, b2c_ir::DiagSource::Analyser, location, message);
        if let Some(decl) = self.decls.get(other) {
            diagnostic = diagnostic.with_related(decl.location.clone(), note);
        }
        self.diags.push(diagnostic);
    }

    /// Reports a warning with a related location at a symbol's declaration.
    fn warning_with_decl(
        &mut self,
        code: &str,
        location: Location,
        message: String,
        other: &SymbolId,
        note: &str,
    ) {
        let mut diagnostic =
            b2c_ir::Diagnostic::warning(code, b2c_ir::DiagSource::Analyser, location, message);
        if let Some(decl) = self.decls.get(other) {
            diagnostic = diagnostic.with_related(decl.location.clone(), note);
        }
        self.diags.push(diagnostic);
    }

    /// Validates a user name; on failure reports it and returns a placeholder
    /// so the program stays complete.
    fn ident(&mut self, name: &str, location: &Location, namespace_scope: bool) -> Ident {
        let checked = Ident::new(name).and_then(|ident| {
            if namespace_scope {
                ident.check_namespace_scope()?;
            }
            Ok(ident)
        });
        let checked = match checked {
            Ok(_) if namespace_scope && STD_TEXT_FUNCTIONS.contains(&name) => Err(format!(
                "{} can't be used as a name: it is already the name of a C++ standard library function \
                 that works on text, and C++ would not know which of the two a call means. Choose another \
                 name.",
                quoted(name)
            )),
            Ok(ident) => Ok(ident),
            Err(IdentError::Empty) => Err(String::from("A name is needed here.")),
            Err(other) => Err(format!("{} can't be used as a name: {other}.", quoted(name))),
        };
        match checked {
            Ok(ident) => ident,
            Err(message) => {
                self.diags.error(codes::INVALID_NAME, location.clone(), message);
                self.placeholders += 1;
                Ident::generated(&format!("b2c_invalid_name{}", self.placeholders))
                    .unwrap_or_else(|_| Ident::main())
            }
        }
    }

    /// Adds a symbol to the table; reports a duplicate symbol ID instead.
    fn insert_symbol(&mut self, sym: &SymbolId, symbol: Symbol, location: Location) -> bool {
        if self.symbols.symbols.contains_key(sym) {
            self.diags.error(
                codes::DUPLICATE_SYMBOL_ID,
                location,
                "This block declares a symbol that another block already declares (they share an ID). \
                 The project file may be damaged: delete this block and create it again.",
            );
            return false;
        }
        self.symbols.symbols.insert(sym.clone(), symbol);
        true
    }

    /// Warns when a local name hides a function of the module.
    fn warn_if_function_name(&mut self, name: &str, location: &Location) {
        let function = self
            .module_functions
            .get(self.module_index)
            .and_then(|f| f.get(name))
            .cloned();
        if let Some(function) = function {
            let message = format!(
                "{} has the same name as a function, so that function can't be called where {} exists. \
                 Consider another name.",
                quoted(name),
                quoted(name)
            );
            self.warning_with_decl(
                codes::SHADOWING,
                location.clone(),
                message,
                &function,
                "the function",
            );
        }
    }

    /// A type field (`int`, `std::string`, …) as a type.
    fn type_setting(&mut self, block: &Block, name: &str, default: &str, allow_void: bool) -> Type {
        let location = self.loc(&block.id, field(name));
        let Ok(text) = access::text_field(block, name, Some(default)) else {
            self.incomplete(
                location,
                "The type setting of this block is damaged. Choose a type again.",
            );
            return Type::Error;
        };
        match Type::from_field(text) {
            Some(Type::Void) if !allow_void => {
                self.incomplete(
                    location,
                    "A variable can't have the type 'nothing' (void). Choose another type.",
                );
                Type::Error
            }
            Some(ty) => ty,
            None => {
                self.incomplete(
                    location,
                    format!("{} is not a type this block can use.", quoted(text)),
                );
                Type::Error
            }
        }
    }

    /// A dropdown field mapped to a value; `default` when absent.
    fn choice<T: Clone>(&mut self, block: &Block, name: &str, default: T, options: &[(&str, T)]) -> T {
        let location = self.loc(&block.id, field(name));
        let text = match access::text_field(block, name, None) {
            Ok(text) => text,
            Err(FieldProblem::Missing) => return default,
            Err(FieldProblem::WrongKind) => {
                self.incomplete(location, "A setting of this block is damaged. Choose it again.");
                return default;
            }
        };
        if let Some((_, value)) = options.iter().find(|(option, _)| *option == text) {
            value.clone()
        } else {
            self.incomplete(
                location,
                format!("{} is not one of the choices for this setting.", quoted(text)),
            );
            default
        }
    }

    /// A checkbox field; `default` when absent.
    fn checkbox(&mut self, block: &Block, name: &str, default: bool) -> bool {
        access::bool_field(block, name, default).unwrap_or_else(|_| {
            let location = self.loc(&block.id, field(name));
            self.incomplete(
                location,
                "A checkbox of this block is damaged. Tick or untick it again.",
            );
            default
        })
    }

    /// The block's comment, if it has a usable one.
    fn comment(&mut self, block: &Block) -> Option<Comment> {
        let text = &block.comment.as_ref()?.text;
        if text.trim().is_empty() {
            return None;
        }
        match Comment::new(text) {
            Ok(comment) => Some(comment),
            Err(error) => {
                let location = self.loc(&block.id, Part::Whole);
                self.diags.warning(
                    codes::COMMENT_DROPPED,
                    location,
                    format!("This block's comment is left out of the C++ code: {error}. Shorten it."),
                );
                None
            }
        }
    }

    /// Enters a nested block; `false` (after reporting once per item) when
    /// the nesting is too deep to follow.
    fn enter(&mut self, block: &Block) -> bool {
        if self.depth >= MAX_BLOCK_DEPTH {
            if !self.too_deep_reported {
                self.too_deep_reported = true;
                let location = self.loc(&block.id, Part::Whole);
                self.diags.error(
                    codes::TOO_DEEP,
                    location,
                    format!(
                        "These blocks are nested too deeply (more than {MAX_BLOCK_DEPTH} levels). \
                         Move some of them into a function."
                    ),
                );
            }
            return false;
        }
        self.depth += 1;
        true
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }
}
