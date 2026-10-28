//! Pass 2: lowering blocks to the Semantic AST while resolving names
//! (spec §6.4–6.5) and checking types (§6.6). Pass 3 (flow checks) runs on
//! each finished item.
//!
//! The walk is in program order, so scopes are exactly those of the generated
//! C++: a variable becomes visible after its `create` block, until the end of
//! its statement list.

mod collections;
mod control;
mod convert;
mod errors;
mod expr;
mod files;
mod globals;
mod resolve;
mod stmt;
mod text;
mod typed;
mod types;
mod user_types;

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::diag::{Location, Part};
use b2c_ir::headers::{Header, ItemHeaders};
use b2c_ir::ids::{BlockId, ModuleId, SymbolId};
use b2c_ir::layout::Layout;
use b2c_ir::sast::{
    Expr, ExprKind, FunctionDef, Item, ItemKind, MainDef, Module, Origin, Param, PassMode, Program, Symbol,
    SymbolKind, SymbolTable,
};
use b2c_ir::sig::SigTable;
use b2c_ir::text::{Comment, Ident, IdentError};
use b2c_ir::types::{Type, TypeClass, TypeTerm};
use b2c_model::{Block, Document, FieldValue};

use crate::Analysis;
use crate::access::{self, FieldProblem};
use crate::codes;
use crate::collect::{self, DeclInfo, DeclKind, DeclStatus, MAX_BLOCK_DEPTH};
use crate::consts::{self, Constant};
use crate::flow;
use crate::messages::{self, Diags, quoted, term_cpp};
use crate::nesting;
use crate::query::{Point, ScopeIndex};
use crate::scope::{HideMark, ListKey, PARAMS_LIST, Scopes};

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

/// Analyses a document (see [`crate::analyze_with`]) with the catalog's
/// signatures; `unchecked` is the blocks the catalog kept as they were
/// without a check (see [`crate::analyze_checked_with`]).
pub(crate) fn run(document: &Document, sigs: &SigTable, unchecked: &BTreeSet<BlockId>) -> Analysis {
    let standard = document.project.language.standard;
    let Some(first) = document.modules.first() else {
        let mut diags = Diags::default();
        diags.error(codes::NO_MAIN, Location::project(), NO_MAIN_MESSAGE);
        let program = Program {
            standard,
            modules: Vec::new(),
            symbols: SymbolTable::default(),
            layout: Layout::default(),
        };
        return Analysis {
            program,
            diagnostics: diags.list,
            index: ScopeIndex::default(),
            slot_types: BTreeMap::new(),
            constants: BTreeMap::new(),
        };
    };
    let mut lowerer = Lowerer::new(document, first.id.clone(), sigs, unchecked);
    let modules = lowerer.program();
    // Parked blocks and skipped definitions answer the scope query too; the
    // parked scope is known once every `main` is lowered.
    lowerer.index.record_unlowered(document);
    let trail = lowerer.scopes.take_trail();
    // A module's definitions are visible everywhere in it: its functions,
    // structs, enums and globals.
    let functions = lowerer
        .functions
        .iter()
        .map(|(sym, sig)| (sig.module, sig.name.as_str(), sym));
    let user_types = lowerer.symbols.symbols.iter().filter_map(|(sym, symbol)| {
        let module = lowerer.decls.get(sym)?.module;
        matches!(
            symbol.kind,
            SymbolKind::Struct { .. } | SymbolKind::Enum { .. } | SymbolKind::Global { .. }
        )
        .then(|| (module, symbol.name.as_str(), sym))
    });
    lowerer
        .index
        .finish(trail, document.modules.len(), functions.chain(user_types));
    // One `.cpp` per module until shared definitions arrive (ADR-0019), its
    // declarations in the declaration graph's order (modules.md §4).
    let mut layout = Layout::flat(&modules, &lowerer.symbols);
    crate::order::order_declarations(&modules, &lowerer.symbols, &mut layout, &mut lowerer.diags);
    Analysis {
        program: Program {
            standard,
            modules,
            symbols: lowerer.symbols,
            layout,
        },
        diagnostics: lowerer.diags.list,
        index: lowerer.index,
        slot_types: lowerer.slot_types,
        constants: lowerer.constants,
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
    /// How many arguments a call must give: the parameters up to the last
    /// one without a default value (w2.md §2.8).
    required: usize,
    /// The catalog headers of the blocks in its default values, which are
    /// printed with its declaration.
    default_headers: ItemHeaders,
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
    /// The parameter's row, which its default value's input is numbered by
    /// (`DEFAULT<row>`).
    row: usize,
    /// Whether its row holds a default value (w2.md §2.8).
    has_default: bool,
    /// Its default value, lowered once every function is declared.
    default: Option<Expr>,
}

/// How a type field or a parameter row's type is used, for its checks and
/// messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeUse {
    /// A variable's type (`create`).
    Variable,
    /// What a function gives back: also `void`.
    Result,
    /// A function input's type.
    Parameter,
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
    /// Ends the hiding of `name` in the scope stack's trail.
    mark: HideMark,
}

#[derive(Debug, Clone)]
enum PendingKind {
    /// A variable's starting value; the declared type (`None` for `auto`).
    Initialiser(Option<Type>),
    /// A `for` loop header.
    Counter,
    /// The path of *open file*, which C++ reads with the new stream's name
    /// already declared.
    OpenedFile,
}

/// The state of the lowering pass.
struct Lowerer<'d> {
    document: &'d Document,
    /// Every declaration in the document (pass 1).
    decls: BTreeMap<SymbolId, DeclInfo>,
    /// Active functions of each module, by name (the first one for duplicates).
    module_functions: Vec<BTreeMap<String, SymbolId>>,
    /// The globals of each module that were declared, by name (w2.md §2.7):
    /// visible in every item of their module.
    module_globals: Vec<BTreeMap<String, SymbolId>>,
    /// The structs and enums of the program, by name (the first one for
    /// duplicates), known before any field, parameter or body is lowered.
    user_types: BTreeMap<String, SymbolId>,
    /// The globals' items, lowered before any body so that a constant's
    /// value is known wherever it is used, by defining block.
    global_items: BTreeMap<BlockId, Item>,
    /// Signatures of the functions that were declared successfully.
    functions: BTreeMap<SymbolId, FuncSig>,
    symbols: SymbolTable,
    diags: Diags,
    scopes: Scopes,
    /// What is visible at each lowered block, for the editor's scope query.
    index: ScopeIndex,
    pending: Vec<Pending>,
    module_index: usize,
    module: ModuleId,
    item: ItemCtx,
    /// The loops and switches around the block being lowered, innermost
    /// last (w2.md §2.13).
    breakable: Vec<Breakable>,
    /// How many catch parts are around the block being lowered: *throw the
    /// error again* needs one (w2.md §2.5).
    catch_depth: usize,
    depth: usize,
    too_deep_reported: bool,
    placeholders: usize,
    /// The catalog's signatures (ADR-0017 §5, ADR-0018).
    sigs: &'d SigTable,
    /// The blocks the catalog kept as they were without a check.
    unchecked: &'d BTreeSet<BlockId>,
    /// For each block being lowered, innermost last, whether its parts are
    /// checked: not for a block the catalog kept unchecked.
    checks: Vec<bool>,
    /// The catalog headers of the item being lowered (types.md §5.1).
    headers: ItemHeaders,
    /// The static type of each expression slot lowered, by block and input.
    slot_types: BTreeMap<BlockId, BTreeMap<String, Type>>,
    /// The `const` variables whose value is known (L10).
    constants: BTreeMap<SymbolId, Constant>,
    /// Whether a function's default values are being lowered, which see the
    /// module's definitions but not the function's parameters (w2.md §2.8).
    in_defaults: bool,
    /// The struct fields whose row holds a default value (w2.md §1.9),
    /// known before any *new … with* is lowered.
    defaulted_fields: BTreeSet<SymbolId>,
}

/// A field part.
fn field(name: &str) -> Part {
    Part::Field {
        name: name.to_owned(),
    }
}

/// The types of the inputs two functions of one name both take for some
/// number of arguments (w2.md §2.8), if there is such a number: each function
/// takes from its `required` inputs to all of them. Pass modes do not tell
/// functions apart (`f(int)` and `f(int&)` take the same `int`), and neither
/// does an input whose type is not known.
fn same_inputs(a: &[ParamSig], a_required: usize, b: &[ParamSig], b_required: usize) -> Option<Vec<Type>> {
    let least = a_required.max(b_required);
    let most = a.len().min(b.len());
    (least..=most).find_map(|count| {
        let (a, b) = (a.get(..count)?, b.get(..count)?);
        let same = a.iter().zip(b).all(|(a, b)| a.ty == b.ty && a.ty != Type::Error);
        same.then(|| a.iter().map(|param| param.ty.clone()).collect())
    })
}

/// Whether an input holds a value: an enabled block or a slot with tokens.
fn holds_value(block: &Block, name: &str) -> bool {
    match access::input(block, name) {
        Some(b2c_model::Input::Block(nested)) => !nested.block.disabled,
        Some(b2c_model::Input::Expr(slot)) => !slot.expr.is_empty(),
        None => false,
    }
}

/// Top-level blocks of a module in a deterministic order (by block ID).
fn sorted_blocks(module: &b2c_model::Module) -> Vec<&Block> {
    let mut blocks: Vec<&Block> = module.workspace.blocks.iter().collect();
    blocks.sort_by(|a, b| a.id.cmp(&b.id));
    blocks
}

/// Block types the analyser lowers as values (`Lowerer::value_block`): the
/// reporters and predicates of the catalog, M2's and W2's (w2.md §2.13). A
/// test checks the list against the catalog's shapes.
fn is_value_type(block_type: &str) -> bool {
    matches!(
        block_type,
        "var.get"
            | "math.number"
            | "math.arithmetic"
            | "math.compare"
            | "math.random_int"
            | "math.convert"
            | "math.fn"
            | "math.abs"
            | "math.minmax"
            | "math.fn_two"
            | "math.clamp"
            | "math.random_real"
            | "logic.boolean"
            | "logic.operation"
            | "logic.not"
            | "logic.ternary"
            | "text.literal"
            | "text.char"
            | "text.join"
            | "text.length"
            | "text.letter"
            | "text.case"
            | "text.split"
            | "text.to_number"
            | "text.as_text"
            | "text.find"
            | "text.part"
            | "text.trim"
            | "text.replace"
            | "func.call"
            | "coll.item"
            | "coll.length"
            | "coll.is_empty"
            | "coll.list_of"
            | "coll.contains"
            | "coll.next"
            | "type.new"
            | "type.field"
            | "type.enum_value"
            | "error.message"
            | "file.read_line"
            | "file.exists"
    )
}

/// A statement that *leave loop* (`break`) leaves: a loop, or a switch
/// (w2.md §2.9). The Lowerer keeps a stack of them, innermost last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Breakable {
    /// A loop: *leave loop* and *skip to next round* work.
    Loop,
    /// A switch: C++'s `break` would leave it, not the loop around it.
    Switch,
}

impl<'d> Lowerer<'d> {
    fn new(
        document: &'d Document,
        module: ModuleId,
        sigs: &'d SigTable,
        unchecked: &'d BTreeSet<BlockId>,
    ) -> Self {
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
            module_globals: vec![BTreeMap::new(); document.modules.len()],
            user_types: BTreeMap::new(),
            global_items: BTreeMap::new(),
            functions: BTreeMap::new(),
            symbols: SymbolTable::default(),
            diags: Diags::default(),
            scopes: Scopes::default(),
            index: ScopeIndex::default(),
            pending: Vec::new(),
            module_index: 0,
            module,
            item: ItemCtx::Main,
            breakable: Vec::new(),
            catch_depth: 0,
            depth: 0,
            too_deep_reported: false,
            placeholders: 0,
            sigs,
            unchecked,
            checks: Vec::new(),
            headers: ItemHeaders::default(),
            slot_types: BTreeMap::new(),
            constants: BTreeMap::new(),
            in_defaults: false,
            defaulted_fields: BTreeSet::new(),
        }
    }

    /// Lowers every module.
    fn program(&mut self) -> Vec<Module> {
        let document = self.document;
        // Structs and enums come first, so that any parameter, result or
        // field may name them (w2.md §2.2), and their flags are known before
        // any type that holds them is checked or any body is lowered
        // (types.md §1.6).
        let fields = self.declare_user_types();
        self.settle_ticks();
        self.check_field_requirements(fields);
        // Globals next: their types may be user types, and every function's
        // parameters, defaults and body see them (w2.md §2.7).
        let globals = self.declare_globals();
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
        // Default values may call any function of the module, so they are
        // lowered once every signature is known.
        for (index, module) in document.modules.iter().enumerate() {
            self.enter_module(index, &module.id);
            for block in sorted_blocks(module) {
                if block.block_type == "func.define" && !block.disabled {
                    self.function_defaults(block);
                }
            }
        }
        // The globals' starting values, which may call any function and
        // read other globals, before any body: a constant's value is then
        // known wherever it is used (a `case` label).
        self.lower_globals(&globals);
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
                    "type.struct" | "type.enum" => (self.type_definition(block), true),
                    "var.global" => (self.global_items.remove(&block.id), true),
                    "program.use_header" => (self.use_header(block), true),
                    _ => {
                        self.report_parked(block);
                        (None, false)
                    }
                };
                if let Some(item) = item {
                    // A global was checked with the others (`lower_globals`).
                    let checked = matches!(item.kind, ItemKind::Global(_));
                    if !checked && self.check_nesting(&item) {
                        flow::check_item(&item, &self.symbols, &mut self.diags);
                        consts::check_item(
                            &item,
                            &self.symbols,
                            self.unchecked,
                            &mut self.diags,
                            &mut self.constants,
                        );
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

    /// Reports a parked stack (ADR-0012, 05 §5.4): an enabled top-level
    /// statement or value block, with the blocks stacked below it, is not
    /// part of the program, so it is not lowered and does not run. It gets
    /// one `B2C-W0505`, on its head, which counts the stacked blocks. A head
    /// the user disabled is not reported (the caller skips it), and neither
    /// is a block type the analyser does not lower: the catalog reports an
    /// unknown type (`B2C-E0601`).
    fn report_parked(&mut self, head: &Block) {
        let gives_value = if access::is_statement_type(&head.block_type) {
            false
        } else if is_value_type(&head.block_type) {
            true
        } else {
            return;
        };
        let location = self.loc(&head.id, Part::Whole);
        self.diags.warning(
            codes::PARKED,
            location,
            messages::parked(gives_value, head.stack.len()),
        );
    }

    /// Reports an item that nests too deeply for the later stages (unless
    /// its block nesting was reported already). Returns whether it is fine.
    fn check_nesting(&mut self, item: &Item) -> bool {
        let location = match &item.kind {
            ItemKind::Main(main) => nesting::too_deep(&main.body).map(|stmt| stmt.origin.location()),
            ItemKind::Function(function) => function
                .params
                .iter()
                .filter_map(|param| param.default.as_ref())
                .find(|value| nesting::expr_too_deep(value, 0))
                .map(|value| value.origin.location())
                .or_else(|| nesting::too_deep(&function.body).map(|stmt| stmt.origin.location())),
            // A definition's values: a global's starting value, a struct's
            // field defaults (w2.md §1.9).
            ItemKind::Global(global) => global
                .init
                .iter()
                .find(|init| nesting::expr_too_deep(init, 0))
                .map(|init| init.origin.location()),
            ItemKind::Struct(def) => def
                .defaults
                .values()
                .find(|value| nesting::expr_too_deep(value, 0))
                .map(|value| value.origin.location()),
            ItemKind::Enum(_) | ItemKind::UseHeader { .. } => None,
        };
        let Some(location) = location else {
            return true;
        };
        if !self.too_deep_reported {
            self.too_deep_reported = true;
            self.diags.error(
                codes::TOO_DEEP,
                location,
                "The values in this block are nested too deeply, together with the blocks around it, to be \
                 turned into C++. Split them up using variables, or move some of the blocks into a function.",
            );
        }
        false
    }

    fn enter_module(&mut self, index: usize, id: &ModuleId) {
        self.module_index = index;
        self.module = id.clone();
        if let Some(globals) = self.module_globals.get(index) {
            self.scopes.set_globals(globals);
        }
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
        self.scopes.reset();
        self.pending.clear();
        self.breakable.clear();
        self.catch_depth = 0;
        self.depth = 0;
        self.too_deep_reported = false;
        self.checks.clear();
        self.headers = ItemHeaders::default();
    }

    fn main(&mut self, block: &Block) -> Item {
        self.begin_item(ItemCtx::Main);
        self.record_block(&block.id);
        self.add_headers(block, true);
        // With arguments (w2.md §1.9): `ARGV` encloses the body, as a
        // function's parameters do, and shares its scope: C++ declares it
        // in the body's own block, so a top-level local of its name is a
        // second declaration there, not one that hides it.
        let with_args = self.checkbox(block, "ARGS", false);
        let args = if with_args {
            self.scopes.push(ListKey::new(&block.id, PARAMS_LIST), false);
            self.declare_main_args(block)
        } else {
            None
        };
        let (body, end) = self.body_and_end(block, "BODY", with_args);
        if with_args {
            self.scopes.pop();
        }
        // What parked blocks see (06 §6.5).
        self.index.record_main_end(end);
        Item {
            kind: ItemKind::Main(MainDef { body, args }),
            origin: self.origin(&block.id, Part::Whole),
            comment: self.comment(block),
            headers: std::mem::take(&mut self.headers),
            shared: false,
        }
    }

    /// Declares `main`'s arguments (`ARGV`), a read-only list of text.
    fn declare_main_args(&mut self, block: &Block) -> Option<SymbolId> {
        let location = self.loc(&block.id, field("ARGV"));
        let Ok(decl) = access::decl_field(block, "ARGV") else {
            self.incomplete(location, "The program's arguments need a name.");
            return None;
        };
        let ident = self.ident(&decl.name, &location, false);
        self.error_if_names_type(&decl.name, &location);
        self.warn_if_hides_definition(&decl.name, &location);
        let text = Type::String;
        let ty = Type::lib(b2c_ir::types::LibType::Vector, vec![text]).unwrap_or(Type::Error);
        let symbol = Symbol {
            name: ident,
            kind: SymbolKind::Variable { is_const: true },
            ty,
            origin: self.origin(&block.id, field("ARGV")),
        };
        if !self.insert_symbol(&decl.sym, &decl.name, symbol, location) {
            return None;
        }
        self.scopes.declare(&decl.name, &decl.sym);
        Some(decl.sym.clone())
    }

    fn function(&mut self, block: &Block) -> Option<Item> {
        let symbol = access::decl_field(block, "NAME").ok()?.sym.clone();
        let sig = self.functions.get(&symbol)?.clone();
        self.begin_item(ItemCtx::Function {
            name: sig.name.clone(),
            ret: sig.ret.clone(),
        });
        self.record_block(&block.id);
        self.add_headers(block, true);
        self.headers.decl.extend(sig.default_headers.decl.iter().copied());
        self.headers.decl.extend(sig.default_headers.body.iter().copied());
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
                default: p.default.clone(),
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
            headers: std::mem::take(&mut self.headers),
            shared: false,
        })
    }

    /// Declares a function's symbol and parameters (before any body is lowered).
    fn declare_function(&mut self, block: &Block, names: &mut BTreeMap<String, Vec<SymbolId>>) {
        let name_loc = self.loc(&block.id, field("NAME"));
        let Ok(decl) = access::decl_field(block, "NAME") else {
            self.incomplete(name_loc, "This function needs a name.");
            return;
        };
        let ident = self.ident(&decl.name, &name_loc, true);
        let ret = self.type_setting(block, "RETURNS", "void", TypeUse::Result);
        let params = self.declare_params(block, &decl.name);
        let required = self.check_default_order(&params, &decl.name);
        let others = names.get(&decl.name).cloned().unwrap_or_default();
        self.check_overloads(&others, &decl.name, &params, required, &name_loc);
        names.entry(decl.name.clone()).or_default().push(decl.sym.clone());
        let symbol = Symbol {
            name: ident,
            kind: SymbolKind::Function {
                params: params
                    .iter()
                    .filter(|p| p.declared)
                    .map(|p| p.sym.clone())
                    .collect(),
                optional: params.iter().skip(required).filter(|p| p.declared).count(),
            },
            ty: ret.clone(),
            origin: self.origin(&block.id, field("NAME")),
        };
        if self.insert_symbol(&decl.sym, &decl.name, symbol, name_loc) {
            let sig = FuncSig {
                name: decl.name.clone(),
                module: self.module_index,
                ret,
                params,
                required,
                default_headers: ItemHeaders::default(),
            };
            self.functions.insert(decl.sym.clone(), sig);
        }
    }

    /// Reports a function named like one declared before it (`others`) that
    /// a call could not tell apart from it (`B2C-E0211`, w2.md §2.8): in
    /// another module, any one of that name (functions have external
    /// linkage, and other modules' functions can't be called yet); in this
    /// module, one whose inputs give the same list of types for some number
    /// of arguments that both take, counting default values and ignoring how
    /// the inputs are passed.
    fn check_overloads(
        &mut self,
        others: &[SymbolId],
        name: &str,
        params: &[ParamSig],
        required: usize,
        location: &Location,
    ) {
        for other in others {
            let module = self.decls.get(other).map(|d| d.module);
            let message = match module {
                Some(module) if module != self.module_index => {
                    let module = self.document.modules.get(module).map(|m| quoted(&m.name));
                    format!(
                        "There is already a function called {} in module {}. Functions of different modules \
                         can't share a name yet, so rename one of them.",
                        quoted(name),
                        module.unwrap_or_default()
                    )
                }
                _ => {
                    let Some(sig) = self.functions.get(other) else {
                        continue;
                    };
                    let Some(shared) = same_inputs(params, required, &sig.params, sig.required) else {
                        continue;
                    };
                    let inputs: Vec<String> = shared
                        .iter()
                        .map(|ty| {
                            ty.to_term()
                                .map_or_else(|| String::from("?"), |term| term_cpp(&term, &self.symbols))
                        })
                        .collect();
                    if inputs.is_empty() {
                        format!(
                            "Two functions called {} can both be called with no inputs, so a call could not \
                             tell them apart.",
                            quoted(name)
                        )
                    } else {
                        format!(
                            "Two functions called {} take the same inputs ({}), so a call could not tell them \
                             apart.",
                            quoted(name),
                            inputs.join(", ")
                        )
                    }
                }
            };
            self.error_with_decl(
                codes::DUPLICATE_FUNCTION,
                location.clone(),
                message,
                other,
                "the other function",
            );
            return;
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
            let ty = match row.ty {
                Ok(term) => self.type_of_term(&term, &loc, TypeUse::Parameter),
                Err(text) => {
                    let message = format!("{} is not a type that a function input can have.", quoted(&text));
                    self.incomplete(loc.clone(), message);
                    Type::Error
                }
            };
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
            self.error_if_names_type(&row.name, &loc);
            self.warn_if_hides_definition(&row.name, &loc);
            let origin = self.origin(&block.id, part);
            let symbol = Symbol {
                name: ident,
                kind: SymbolKind::Parameter { mode },
                ty: ty.clone(),
                origin: origin.clone(),
            };
            let declared = self.insert_symbol(&row.sym, &row.name, symbol, loc);
            params.push(ParamSig {
                sym: row.sym,
                name: row.name,
                ty: if declared { ty } else { Type::Error },
                mode,
                origin,
                declared,
                row: i,
                has_default: holds_value(block, &format!("DEFAULT{i}")),
                default: None,
            });
        }
        params
    }

    /// Reports each parameter without a default value after one with a
    /// default (`B2C-E0426`, w2.md §2.8), and returns how many arguments a
    /// call must give: the parameters up to the last one without a default.
    fn check_default_order(&mut self, params: &[ParamSig], function: &str) -> usize {
        let mut first: Option<&ParamSig> = None;
        for param in params {
            if param.has_default {
                first.get_or_insert(param);
                continue;
            }
            let Some(first) = first else {
                continue;
            };
            let message = format!(
                "{} has no default value, but {} before it has one. In C++, once an input of {} has a \
                 default value, every input after it needs one too: give {} a default value, or remove \
                 the default value of {}.",
                quoted(&param.name),
                quoted(&first.name),
                quoted(function),
                quoted(&param.name),
                quoted(&first.name),
            );
            let location = param.origin.location();
            self.error_with_decl(
                codes::DEFAULT_ORDER,
                location,
                message,
                &first.sym,
                "the input with a default value",
            );
        }
        params
            .iter()
            .rposition(|p| !p.has_default)
            .map_or(0, |last| last + 1)
    }

    /// Lowers the default values of a function's parameters (w2.md §2.8),
    /// typed by their parameters (`param:NAME`). A default sees the module's
    /// definitions but not the parameters, as in C++.
    fn function_defaults(&mut self, block: &Block) {
        let Ok(decl) = access::decl_field(block, "NAME") else {
            return;
        };
        let Some(sig) = self.functions.get(&decl.sym).cloned() else {
            return;
        };
        let rows = access::param_rows(block).len();
        let any = (0..rows).any(|row| access::input(block, &format!("DEFAULT{row}")).is_some());
        if !any {
            return;
        }
        self.begin_item(ItemCtx::Function {
            name: sig.name.clone(),
            ret: sig.ret.clone(),
        });
        self.in_defaults = true;
        let mut by_row = vec![Type::Error; rows];
        for param in &sig.params {
            if let Some(ty) = by_row.get_mut(param.row) {
                *ty = param.ty.clone();
            }
        }
        let mut typed = self.typed(self.block_sig(&block.block_type));
        typed.params("NAME", by_row);
        typed.count("DEFAULT", rows);
        for row in 0..rows {
            let part = format!("DEFAULT{row}");
            self.record_input(&block.id, &part);
            self.typed_input(block, &mut typed, &part, false);
        }
        let typing = self.type_parts(block, &mut typed, None);
        let mut defaults = BTreeMap::new();
        for param in &sig.params {
            let part = format!("DEFAULT{}", param.row);
            let Some(value) = typed.take(&part) else {
                continue;
            };
            let subject = convert::Subject::Phrase {
                text: format!("The default value of {}", quoted(&param.name)),
                stores: true,
            };
            self.check_param_default_names(&value, &decl.sym, &sig, param);
            if param.mode == PassMode::Editable {
                // `int& x = …` binds the parameter to what the default
                // names, so it must be a changeable variable, as an
                // argument must (`B2C-E0307`).
                self.check_editable_argument(&value, param, &sig.name);
            } else {
                self.report_fit(&typed, &typing, &part, &value, &subject);
            }
            defaults.insert(param.sym.clone(), value);
        }
        self.in_defaults = false;
        let headers = std::mem::take(&mut self.headers);
        if let Some(sig) = self.functions.get_mut(&decl.sym) {
            for param in &mut sig.params {
                param.default = defaults.remove(&param.sym);
            }
            sig.default_headers = headers;
        }
    }

    /// What C++ looks up by name in a default value, at any depth: each
    /// global it names and each function it calls (`true`), with where. A
    /// local or a parameter is reported where it is used.
    pub(super) fn names_looked_up(&self, value: &Expr) -> Vec<(SymbolId, bool, Location)> {
        let mut found = Vec::new();
        let mut stack = vec![value];
        while let Some(expr) = stack.pop() {
            match &expr.kind {
                ExprKind::Call { function, .. } => {
                    found.push((function.clone(), true, expr.origin.location()));
                }
                ExprKind::Var(sym)
                    if matches!(
                        self.symbols.get(sym).map(|symbol| &symbol.kind),
                        Some(SymbolKind::Global { .. })
                    ) =>
                {
                    found.push((sym.clone(), false, expr.origin.location()));
                }
                _ => {}
            }
            stack.extend(flow::children(expr));
        }
        found
    }

    /// Reports what the default value of `param` names that C++ would not
    /// find as the blocks mean it (w2.md §2.8): a global or function called
    /// like an input up to this one, since C++ finds an input by its name
    /// from the input on, also in default values (g++ lets an input's own
    /// default name a global of the input's name, which the standard does
    /// not); and a call to `function` itself, which C++ declares only after
    /// its whole first line, inputs included.
    fn check_param_default_names(
        &mut self,
        value: &Expr,
        function: &SymbolId,
        sig: &FuncSig,
        param: &ParamSig,
    ) {
        for (sym, called, location) in self.names_looked_up(value) {
            let name = self.raw_name(&sym);
            if let Some(input) = sig.params.iter().find(|p| p.row <= param.row && p.name == name) {
                let (uses, other) = if called {
                    ("calls the function", "function")
                } else {
                    ("uses the global", "global")
                };
                let message = format!(
                    "This {uses} {name}, but {} has an input called {name}, and from that input on C++ finds the \
                     input by that name, also in default values. Rename the input or the {other}.",
                    quoted(&sig.name),
                    name = quoted(&name)
                );
                let input = input.sym.clone();
                self.error_with_decl(codes::NAME_HIDDEN, location, message, &input, "the input");
            } else if &sym == function {
                let message = format!(
                    "The default value of {} calls {} itself, but C++ knows {} only after the whole first line of \
                     its definition, inputs included. Use a value, a constant or another function instead.",
                    quoted(&param.name),
                    quoted(&sig.name),
                    quoted(&sig.name)
                );
                self.diags.error(codes::DECLARATION_CYCLE, location, message);
            }
        }
    }

    // --- The Lowerer context of W2 (w2.md §2.13) ---------------------------------

    /// Lowers `lower` inside a loop or a switch: the one way family modules
    /// push and pop the stack that *leave loop* and *skip to next round*
    /// read (`jump`).
    pub(super) fn within<T>(&mut self, kind: Breakable, lower: impl FnOnce(&mut Self) -> T) -> T {
        self.breakable.push(kind);
        let lowered = lower(self);
        self.breakable.pop();
        lowered
    }

    /// Lowers `lower` inside a catch part, where *throw the error again*
    /// may be used, at any depth of other blocks (w2.md §2.5).
    pub(super) fn in_catch<T>(&mut self, lower: impl FnOnce(&mut Self) -> T) -> T {
        self.catch_depth += 1;
        let lowered = lower(self);
        self.catch_depth -= 1;
        lowered
    }

    /// Reports *throw the error again* outside a catch part (`B2C-E0402`).
    pub(super) fn check_rethrow(&mut self, block: &Block) {
        if self.catch_depth == 0 {
            let location = self.loc(&block.id, Part::Whole);
            self.diags.error(
                codes::RETHROW_OUTSIDE_CATCH,
                location,
                "'throw the error again' can only be used inside a 'catch' part, where there is an error to \
                 throw again.",
            );
        }
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

    /// Adds a symbol, whose user's name is `name`, to the table; reports a
    /// duplicate symbol ID instead.
    fn insert_symbol(&mut self, sym: &SymbolId, name: &str, symbol: Symbol, location: Location) -> bool {
        if self.symbols.symbols.contains_key(sym) {
            self.diags.error(
                codes::DUPLICATE_SYMBOL_ID,
                location,
                "This block declares a symbol that another block already declares (they share an ID). \
                 The project file may be damaged: delete this block and create it again.",
            );
            return false;
        }
        if symbol.name.as_str() != name {
            // An invalid name has a placeholder in the table; the editor
            // shows what the user wrote.
            self.index.record_name(sym, name);
        }
        self.symbols.symbols.insert(sym.clone(), symbol);
        true
    }

    // --- The editor's scope index -----------------------------------------------

    /// What is visible at the current point of the lowering.
    fn point(&self) -> Point {
        Point {
            node: self.scopes.here(),
            module: self.module_index,
        }
    }

    /// Records what is visible at a block that is being lowered, or at a
    /// disabled statement of a list that is being lowered.
    fn record_block(&mut self, block: &BlockId) {
        let point = self.point();
        self.index.record_block(block, point);
    }

    /// Records what is visible at the start of a statement list, or inside a
    /// value input, of a block that is being lowered.
    fn record_input(&mut self, block: &BlockId, input: &str) {
        let point = self.point();
        self.index.record_input(block, input, point);
    }

    /// Records a disabled block that the lowering skips where an enabled one
    /// would be lowered (a statement of a list, or the value of an input):
    /// it answers for this position, and so does every block nested in it.
    /// Like a lowered block, it is recorded only within [`MAX_BLOCK_DEPTH`].
    fn record_skipped(&mut self, block: &Block) {
        let point = self.point();
        self.index.record_tree(block, point, self.depth + 1);
    }

    /// Starts lowering the starting value of a variable, or the header of a
    /// `for` loop: C++ already binds `name` there to the new declaration.
    fn push_pending(&mut self, name: &str, sym: &SymbolId, kind: PendingKind) {
        let mark = self.scopes.hide(name);
        self.pending.push(Pending {
            name: name.to_owned(),
            sym: sym.clone(),
            kind,
            mark,
        });
    }

    /// Ends what [`Self::push_pending`] started.
    fn pop_pending(&mut self) {
        if let Some(pending) = self.pending.pop() {
            self.scopes.unhide(pending.mark);
        }
    }

    /// The struct or enum of the program called `name`, with what it is for
    /// messages ("struct" or "enum").
    fn user_type_named(&self, name: &str) -> Option<(SymbolId, &'static str)> {
        let sym = self.user_types.get(name)?;
        let what = match self.symbols.get(sym).map(|symbol| &symbol.kind)? {
            SymbolKind::Struct { .. } => "struct",
            SymbolKind::Enum { .. } => "enum",
            _ => return None,
        };
        Some((sym.clone(), what))
    }

    /// Reports a local, a parameter or `main`'s arguments named like a
    /// struct or an enum of the program (`B2C-E0210`): the generated C++
    /// names user types without qualification, so where the new name
    /// exists, C++ would find it instead of the type, and every use of the
    /// type there would fail to compile.
    fn error_if_names_type(&mut self, name: &str, location: &Location) {
        let Some((other, what)) = self.user_type_named(name) else {
            return;
        };
        let message = format!(
            "{name} is the name of {article} {what}, so C++ would not find the {what} by that name where {name} \
             exists. Choose another name.",
            name = quoted(name),
            article = if what == "enum" { "an" } else { "a" },
        );
        self.error_with_decl(
            codes::DUPLICATE_NAME,
            location.clone(),
            message,
            &other,
            &format!("the {what}"),
        );
    }

    /// Warns when a local name hides a function or a global of the module
    /// (`B2C-W0501`).
    fn warn_if_hides_definition(&mut self, name: &str, location: &Location) {
        let global = self
            .module_globals
            .get(self.module_index)
            .and_then(|globals| globals.get(name))
            .cloned();
        if let Some(global) = global {
            let what = if matches!(
                self.symbols.get(&global).map(|symbol| &symbol.kind),
                Some(SymbolKind::Global { is_const: true })
            ) {
                "global constant"
            } else {
                "global variable"
            };
            let message = format!(
                "{name} has the same name as a {what}, so that {what} can't be used where {name} exists. This \
                 works, but it is easy to mix them up, so consider another name.",
                name = quoted(name)
            );
            self.warning_with_decl(
                codes::SHADOWING,
                location.clone(),
                message,
                &global,
                &format!("the {what}"),
            );
        }
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

    /// A type field as a type (types.md §2.1): a name (`int`,
    /// `std::string`), a template (`{"std::vector": "int"}`) or a user type
    /// (`{"ref": id}`); `default` when absent.
    fn type_setting(&mut self, block: &Block, name: &str, default: &str, used: TypeUse) -> Type {
        let location = self.loc(&block.id, field(name));
        let term = match block.fields.get(name) {
            None => TypeTerm::name(default).ok(),
            Some(FieldValue::Text(text)) => {
                let Ok(term) = TypeTerm::name(text) else {
                    let message = format!("{} is not a type this block can use.", quoted(text));
                    self.incomplete(location, message);
                    return Type::Error;
                };
                Some(term)
            }
            Some(value) => value.type_term(),
        };
        let Some(term) = term else {
            self.incomplete(
                location,
                "The type setting of this block is damaged. Choose a type again.",
            );
            return Type::Error;
        };
        self.type_of_term(&term, &location, used)
    }

    /// Looks a type term up (types.md §2.2): its references must name user
    /// types (`B2C-E0201`, `B2C-E0207`), and its names known types of the
    /// class `value` (`B2C-E0430` otherwise, which the catalog's own report
    /// of the field replaces), `void` only for a result. The elements of
    /// sets and priority queues and the keys of maps must be comparable
    /// (`B2C-E0313`).
    fn type_of_term(&mut self, term: &TypeTerm, location: &Location, used: TypeUse) -> Type {
        let ty = self.lookup_term(term, location, used);
        self.check_requirements(&ty, location);
        ty
    }

    /// [`Self::type_of_term`] without the requirements of sets, priority
    /// queues and maps, which a struct's fields check once every struct's
    /// flags are known.
    fn lookup_term(&mut self, term: &TypeTerm, location: &Location, used: TypeUse) -> Type {
        for id in term.refs() {
            if !self.check_type_reference(id, location) {
                return Type::Error;
            }
        }
        match Type::from_term(term) {
            Ok(Type::Void) if used == TypeUse::Result => return Type::Void,
            Ok(Type::Void) if used == TypeUse::Variable => {
                self.incomplete(
                    location.clone(),
                    "A variable can't have the type 'nothing' (void). Choose another type.",
                );
                return Type::Error;
            }
            Ok(ty) if ty.is_in(TypeClass::Value, &self.symbols) != Some(false) => return ty,
            Ok(_) | Err(_) => {}
        }
        let shown = quoted(&term_cpp(term, &self.symbols));
        let message = if used == TypeUse::Parameter {
            format!("{shown} is not a type that a function input can have.")
        } else {
            format!("{shown} is not a type this block can use.")
        };
        self.incomplete(location.clone(), message);
        Type::Error
    }

    /// Adds the catalog headers of a block that is lowered, with those of
    /// its chosen options, to the item (types.md §5.1): to `decl` for the
    /// block that is printed with the declaration (the definition's own
    /// block), to `body` for every block printed only with the definition.
    fn add_headers(&mut self, block: &Block, decl: bool) {
        let Some(sig) = self.sigs.get(&block.block_type) else {
            return;
        };
        let options =
            sig.option_headers
                .iter()
                .filter_map(|(field, options)| match block.fields.get(field) {
                    Some(FieldValue::Text(chosen)) => options.get(chosen),
                    _ => None,
                });
        let headers: Vec<Header> = sig.headers.iter().chain(options.flatten()).copied().collect();
        let part = if decl {
            &mut self.headers.decl
        } else {
            &mut self.headers.body
        };
        part.extend(headers);
    }

    /// Starts lowering a block of a statement list or a value input: its
    /// headers count, and its parts are checked unless the catalog kept it
    /// unchecked.
    fn begin_block(&mut self, block: &Block) {
        self.checks.push(!self.unchecked.contains(&block.id));
        self.add_headers(block, false);
    }

    /// Ends what [`Self::begin_block`] started.
    fn end_block(&mut self) {
        self.checks.pop();
    }

    /// Whether the parts of the block being lowered are checked: not those
    /// of a block the catalog kept unchecked (ADR-0018 §3).
    fn checking(&self) -> bool {
        self.checks.last().copied().unwrap_or(true)
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

/// The Lowerer context W2's family modules share (w2.md §2.13): the stack of
/// loops and switches *leave loop* reads, and the depth of catch parts
/// *throw the error again* reads. The blocks that push them are tested by
/// their packages.
#[cfg(test)]
mod context_tests {
    use std::collections::BTreeSet;

    use b2c_ir::sast::StmtKind;
    use b2c_ir::sig::SigTable;

    use super::user_types::tests::{document, module};
    use super::*;

    fn block(block_type: &str) -> Block {
        serde_json::from_value(serde_json::json!({"id": "b1", "type": block_type, "v": 1})).expect("block")
    }

    /// The codes and messages reported so far.
    fn reported(lowerer: &Lowerer<'_>) -> Vec<(String, String)> {
        lowerer
            .diags
            .list
            .iter()
            .map(|d| (d.code.0.clone(), d.message.clone()))
            .collect()
    }

    #[test]
    fn leave_loop_in_a_switch_inside_a_loop_is_reported() {
        let document = document();
        let sigs = SigTable::empty();
        let unchecked = BTreeSet::new();
        let mut lowerer = Lowerer::new(&document, module(), &sigs, &unchecked);
        let leave = block("control.break");
        let skip = block("control.continue");

        // Directly in a loop, and in a loop inside a switch: fine.
        lowerer.within(Breakable::Loop, |l| {
            l.jump(&leave, StmtKind::Break, "'leave loop'");
            l.within(Breakable::Switch, |l| {
                l.within(Breakable::Loop, |l| {
                    l.jump(&leave, StmtKind::Break, "'leave loop'")
                });
                // *skip to next round* reaches the loop around the switch.
                l.jump(&skip, StmtKind::Continue, "'skip to next round'");
            });
        });
        assert_eq!(reported(&lowerer), Vec::new());

        // In a switch inside a loop: C++'s `break` would leave the switch.
        lowerer.within(Breakable::Loop, |l| {
            l.within(Breakable::Switch, |l| {
                l.jump(&leave, StmtKind::Break, "'leave loop'")
            });
        });
        // In a switch with no loop around it.
        lowerer.within(Breakable::Switch, |l| {
            l.jump(&leave, StmtKind::Break, "'leave loop'")
        });
        let expected = vec![
            (
                String::from("B2C-E0401"),
                String::from(
                    "In C++ 'leave loop' here would only leave the switch, not the loop around it. To leave \
                     the loop, set a variable that the loop checks, or use 'return' in a function.",
                ),
            ),
            (
                String::from("B2C-E0401"),
                String::from("'leave loop' can only be used inside a loop."),
            ),
        ];
        assert_eq!(reported(&lowerer), expected);
        assert!(lowerer.breakable.is_empty());
    }

    #[test]
    fn throw_again_is_reported_outside_a_catch_part() {
        let document = document();
        let sigs = SigTable::empty();
        let unchecked = BTreeSet::new();
        let mut lowerer = Lowerer::new(&document, module(), &sigs, &unchecked);
        let rethrow = block("error.rethrow");
        lowerer.in_catch(|l| {
            l.check_rethrow(&rethrow);
            // At any depth of other blocks, and of catch parts.
            l.within(Breakable::Loop, |l| l.in_catch(|l| l.check_rethrow(&rethrow)));
        });
        assert_eq!(reported(&lowerer), Vec::new());
        assert_eq!(lowerer.catch_depth, 0);
        lowerer.check_rethrow(&rethrow);
        assert_eq!(
            reported(&lowerer),
            vec![(
                String::from("B2C-E0402"),
                String::from(
                    "'throw the error again' can only be used inside a 'catch' part, where there is an error \
                     to throw again."
                ),
            )]
        );
    }
}
