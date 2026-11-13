//! The program's files (ADR-0019; `docs/plans/m3-contracts/modules.md` §1,
//! §3, §4; `w3-modules.md` §1.7, §2.1 to §2.4): which files each module
//! gets, what they include and forward-declare, and in which order their
//! declarations are printed. The analyser computes the [`Layout`] and
//! reports its problems; code generation only prints it.
//!
//! [`compute_layout`] starts from milestone M2's flat layout (one `.cpp` per
//! module, named by its [`FileStem`], or `module<N>` for a name the stem
//! refuses), orders each module's declarations with the program's
//! declaration graph (`crate::order`), and then works out the headers:
//!
//! * [`shared_items`]: the items whose declarations go into their module's
//!   header: a shared function's declaration, with its default values; a
//!   shared struct's or enum's definition (an enum's print support declared
//!   right after it); a shared variable's or `const` constant's `extern`
//!   declaration; a shared `constexpr` constant's definition.
//! * [`header_parts`]: the order of those declarations in the header
//!   (`header_order`), the program graph's.
//! * [`includes`]: the project headers each file includes, and the types a
//!   header forward-declares instead ([`Needs`], w3-modules.md §2.4). A
//!   header includes another module's header when a header part needs a
//!   complete type or a declaration from it: a default value (a function's
//!   or a field's), a `constexpr` constant's value, a field that names it,
//!   a shared global's type. A type named only in a function's inputs and
//!   result is forward-declared. A field that names another module's struct
//!   only inside lists includes its header too (unlike modules.md §3.3): the
//!   field's `{}` initialiser makes `std::vector`'s constructor and
//!   destructor need the struct complete, so a header that only declared it
//!   would not compile alone (g++ 11 to 14, clang 18; the struct of the
//!   module's own header is complete at the end of it, so a forward
//!   declaration still serves there). A `.cpp` includes its own header, the
//!   header of each module whose shared definitions its code names, and the
//!   headers of the types its included headers forward-declare, so that
//!   every type it touches is complete.
//!
//! The headers bring the rules of w3-modules.md §2.1 to §2.4, reported here
//! in a project of several modules:
//!
//! * `B2C-E0210`: two modules share definitions with one name (two shared
//!   functions are `B2C-E0211`, reported where functions are declared).
//! * `B2C-E0213`: a definition that is not shared has the name of a shared
//!   one, of its own module or of any other (W3M-D10: program-wide, so it
//!   never appears because of an unrelated edit). The message names the
//!   cause when the module's file does include that header, with the chain
//!   of includes as related locations.
//! * `B2C-E0214`: what a shared definition's header part shows names a
//!   definition of its module that is not shared.
//! * `B2C-E0422`: headers that would include each other when no circle of
//!   definitions explains it. A circle of definitions, also across modules,
//!   is the declaration graph's (`B2C-E0420`, `B2C-E0421`).
//!
//! A single-module project shares nothing, so it gets M2's layout and its
//! C++ does not change. The layout also reports `B2C-E0221` for a module
//! whose name starts with `b2c` (modules.md §2), in every project.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use b2c_ir::diag::{DiagSource, Diagnostic, Location, Part};
use b2c_ir::ids::SymbolId;
use b2c_ir::layout::{Layout, ModuleFiles};
use b2c_ir::sast::{Block, Expr, ExprKind, Item, ItemKind, Module, Stmt, StmtKind, SymbolKind, SymbolTable};
use b2c_ir::text::{FileStem, StemError};
use b2c_ir::types::Type;

use crate::codes;
use crate::consts::Constant;
use crate::messages::{Diags, quoted};
use crate::order::item_symbol;

/// The message of `B2C-E0221` (modules.md §2).
const RESERVED_MODULE_MESSAGE: &str =
    "Module names that start with `b2c` are kept for files that Blocks2Cpp writes. Rename the module.";

/// The layout of a lowered program, with its problems: the declaration
/// graph's cycles (`B2C-E0420`, `B2C-E0421`) and `B2C-E0423`, module names
/// that start with `b2c` (`B2C-E0221`), and the rules of shared definitions
/// (`B2C-E0210`, `B2C-E0213`, `B2C-E0214`, `B2C-E0422`). `constants` holds
/// the values L10 knows (`B2C-E0423` lets another module read a constant
/// C++ sets before any code runs).
pub(crate) fn compute_layout(
    modules: &[Module],
    symbols: &SymbolTable,
    constants: &BTreeMap<SymbolId, Constant>,
    diags: &mut Diags,
) -> Layout {
    check_module_names(modules, diags);
    let mut layout = Layout::flat(modules, symbols);
    let explained = crate::order::order_declarations(modules, symbols, constants, &mut layout, diags);
    for (module, files) in modules.iter().zip(&mut layout.modules) {
        let shared = shared_items(module);
        files.header_order = header_parts(files, &shared);
        files.header = !files.header_order.is_empty();
    }
    let program = ProgramView::new(modules, symbols);
    let needs = Needs::collect(&program, &layout);
    includes(&mut layout, &needs, symbols);
    if layout.multi_module() {
        check_header_parts(&program, &layout, diags);
        check_header_cycles(&program, &layout, &needs, &explained, diags);
        check_shared_names(&program, &layout, &needs, diags);
    }
    layout
}

/// Reports each module whose name starts with `b2c` (`B2C-E0221`; the
/// loader's pattern allows no capital letter): the loader accepts it, so that the project opens and the
/// module can be renamed, but it never becomes a file name. Its file is
/// `module<N>.cpp` until then ([`Layout::flat`]).
fn check_module_names(modules: &[Module], diags: &mut Diags) {
    for module in modules {
        if FileStem::new(&module.name) == Err(StemError::Reserved) {
            let location = Location {
                module: Some(module.id.clone()),
                block: None,
                part: Part::Whole,
            };
            diags.error(codes::RESERVED_MODULE_NAME, location, RESERVED_MODULE_MESSAGE);
        }
    }
}

/// The items of a module whose declarations go into its header, by index
/// into `Module::items`: the shared ones (`Item::shared`, which the analyser
/// sets only in a project of several modules).
pub(crate) fn shared_items(module: &Module) -> Vec<usize> {
    module
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| goes_into_header(item))
        .map(|(index, _)| index)
        .collect()
}

/// Whether an item's declaration goes into its module's header.
fn goes_into_header(item: &Item) -> bool {
    item.shared
        && match &item.kind {
            ItemKind::Function(_) | ItemKind::Struct(_) | ItemKind::Enum(_) | ItemKind::Global(_) => true,
            // `main` and *use header* are never shared.
            ItemKind::Main(_) | ItemKind::UseHeader { .. } => false,
        }
}

/// The header's declarations in printing order (`header_order`): the shared
/// items in the order the declaration graph gives the module's `.cpp`.
pub(crate) fn header_parts(files: &ModuleFiles, shared: &[usize]) -> Vec<usize> {
    files
        .source_order
        .iter()
        .copied()
        .filter(|index| shared.contains(index))
        .collect()
}

/// The project headers each file includes, each list sorted by file name,
/// and the other modules' types each header forward-declares, sorted by
/// name (no type is in a namespace yet). A `.cpp` includes its own header,
/// before its definitions (w3-modules.md §1.7: without it a `const`
/// constant's definition has internal linkage and does not link), and the
/// header of every module its code needs ([`Needs::source`], which holds
/// the headers of the types its included headers forward-declare too); a
/// header includes the header of every module its declarations need
/// complete, and forward-declares the types of the other modules it names
/// only where a declaration is enough.
pub(crate) fn includes(layout: &mut Layout, needs: &Needs<'_>, symbols: &SymbolTable) {
    let headers: Vec<bool> = layout.modules.iter().map(|files| files.header).collect();
    let stems: Vec<FileStem> = layout.modules.iter().map(|files| files.stem.clone()).collect();
    let ids: Vec<_> = layout.modules.iter().map(|files| files.module.clone()).collect();
    let listed = |modules: BTreeSet<usize>| -> Vec<_> {
        let mut modules: Vec<usize> = modules
            .into_iter()
            .filter(|n| headers.get(*n).copied().unwrap_or(false))
            .collect();
        modules.sort_by(|a, b| stems[*a].cmp(&stems[*b]).then(a.cmp(b)));
        modules.into_iter().map(|n| ids[n].clone()).collect()
    };
    for (m, files) in layout.modules.iter_mut().enumerate() {
        let mut source: BTreeSet<usize> = needs
            .source
            .get(m)
            .into_iter()
            .flat_map(BTreeMap::keys)
            .copied()
            .collect();
        if files.header {
            source.insert(m);
        }
        files.source_includes = listed(source);
        if files.header {
            let included: BTreeSet<usize> = needs
                .header
                .get(m)
                .into_iter()
                .flat_map(BTreeMap::keys)
                .copied()
                .collect();
            let mut forward: Vec<(&str, &SymbolId)> = needs
                .forward
                .get(m)
                .into_iter()
                .flatten()
                .filter(|(_, (module, _))| !included.contains(module))
                .filter_map(|(id, _)| Some((symbols.get(id)?.name.as_str(), *id)))
                .collect();
            forward.sort();
            files.forward = forward.into_iter().map(|(_, id)| id.clone()).collect();
            files.header_includes = listed(included);
        } else {
            files.header_includes = Vec::new();
            files.forward = Vec::new();
        }
    }
}

// --- The program's definitions and what each file needs ---------------------

/// Where a definition is: its module and item, by index.
#[derive(Debug, Clone, Copy)]
struct Def {
    module: usize,
    item: usize,
}

/// The program's modules with each definition's place.
struct ProgramView<'p> {
    modules: &'p [Module],
    symbols: &'p SymbolTable,
    /// Every function, struct, enum and global item, by symbol (the first
    /// item for a symbol two items name, which a damaged file can have).
    defs: BTreeMap<&'p SymbolId, Def>,
}

impl<'p> ProgramView<'p> {
    fn new(modules: &'p [Module], symbols: &'p SymbolTable) -> Self {
        let mut defs = BTreeMap::new();
        for (module, items) in modules.iter().map(|m| &m.items).enumerate() {
            for (item, entry) in items.iter().enumerate() {
                if let Some(symbol) = item_symbol(entry) {
                    defs.entry(symbol).or_insert(Def { module, item });
                }
            }
        }
        ProgramView {
            modules,
            symbols,
            defs,
        }
    }

    fn item(&self, def: Def) -> Option<&'p Item> {
        self.modules.get(def.module)?.items.get(def.item)
    }

    /// The definition of a symbol, if an item of the program defines it.
    fn def(&self, symbol: &SymbolId) -> Option<Def> {
        self.defs.get(symbol).copied()
    }

    /// A symbol's name for messages, in backticks.
    fn name(&self, symbol: &SymbolId) -> String {
        self.symbols
            .get(symbol)
            .map_or_else(|| String::from("an unknown name"), |s| quoted(s.name.as_str()))
    }

    /// A module's name for messages, in backticks.
    fn module_name(&self, module: usize) -> String {
        self.modules
            .get(module)
            .map(|m| quoted(&m.name))
            .unwrap_or_default()
    }

    /// Where a symbol is declared (its name field), for related locations.
    fn location(&self, symbol: &SymbolId) -> Option<Location> {
        self.symbols.get(symbol).map(|s| s.origin.location())
    }

    /// The index of the module a symbol is declared in.
    fn module_of(&self, symbol: &SymbolId) -> Option<usize> {
        let module = &self.symbols.get(symbol)?.origin.module;
        self.modules.iter().position(|m| m.id == *module)
    }

    /// What a definition is, for messages: `function`, `struct`, `enum`,
    /// `global variable` or `global constant`.
    fn kind_word(&self, symbol: &SymbolId) -> &'static str {
        match self.symbols.get(symbol).map(|s| &s.kind) {
            Some(SymbolKind::Function { .. }) => "function",
            Some(SymbolKind::Struct { .. }) => "struct",
            Some(SymbolKind::Enum { .. }) => "enum",
            Some(SymbolKind::Global { is_const: true }) => "global constant",
            Some(SymbolKind::Global { is_const: false }) => "global variable",
            _ => "definition",
        }
    }
}

/// A header's name for messages: `` `<stem>.hpp` ``.
fn header_name(layout: &Layout, module: usize) -> String {
    layout
        .modules
        .get(module)
        .map(|files| quoted(&format!("{}.hpp", files.stem.as_str())))
        .unwrap_or_default()
}

/// Which header part of a shared definition makes a need, for messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Why {
    /// A function's default value.
    DefaultValue,
    /// A struct's field, which holds the type.
    Field,
    /// A struct's field's default value.
    FieldDefault,
    /// A `constexpr` constant's value.
    Value,
    /// A shared global's type.
    GlobalType,
    /// A function's inputs or result (a forward declaration is enough).
    Signature,
}

/// Why a file needs another module's header: the first use of one of that
/// module's shared definitions in it.
#[derive(Debug, Clone)]
pub(crate) struct Reason<'p> {
    /// Where the use is.
    location: Location,
    /// The shared definition used.
    used: &'p SymbolId,
    /// For a header's need, the shared definition of the module whose
    /// header part uses it.
    by: Option<&'p SymbolId>,
    /// Which part of `by` uses it.
    why: Why,
}

impl Reason<'_> {
    /// What makes the need, for messages: `` the default value of `f` uses
    /// `g` ``, `` `Shape` holds `Palette` ``.
    fn cause(&self, program: &ProgramView<'_>) -> String {
        let by = self.by.map(|by| program.name(by)).unwrap_or_default();
        let used = program.name(self.used);
        match self.why {
            Why::DefaultValue => format!("the default value of {by} uses {used}"),
            Why::Field => format!("{by} holds {used}"),
            Why::FieldDefault => format!("a default value of {by} uses {used}"),
            Why::Value => format!("the value of {by} uses {used}"),
            Why::GlobalType => format!("the type of {by} uses {used}"),
            Why::Signature => format!("{by} names {used}"),
        }
    }

    /// The part that makes the need, for related locations: `the default
    /// value of `f``, `the fields of `Shape``.
    fn part(&self, program: &ProgramView<'_>) -> String {
        let by = self.by.map(|by| program.name(by)).unwrap_or_default();
        match self.why {
            Why::DefaultValue => format!("the default value of {by}"),
            Why::Field => format!("the fields of {by}"),
            Why::FieldDefault => format!("the default values of {by}'s fields"),
            Why::Value => format!("the value of {by}"),
            Why::GlobalType => format!("the type of {by}"),
            Why::Signature => format!("the inputs and result of {by}"),
        }
    }
}

/// What each module's files need of the other modules' headers
/// (w3-modules.md §2.4): for a `.cpp`, the first reason for each; for a
/// header, the first reason of each shared definition whose header part
/// needs it, in order, since `B2C-E0422` goes on each of them, and the
/// types it may forward-declare. The first reason is the one the include
/// and the chain of `B2C-E0213` name.
#[derive(Debug, Clone, Default)]
pub(crate) struct Needs<'p> {
    /// By module index: the modules whose headers its `.cpp` needs.
    source: Vec<BTreeMap<usize, Reason<'p>>>,
    /// By module index: the modules whose headers its header needs, each
    /// with at least one reason.
    header: Vec<BTreeMap<usize, Vec<Reason<'p>>>>,
    /// By module index: the other modules' types its header names only
    /// where a declaration is enough, each with its module and first reason.
    forward: Vec<BTreeMap<&'p SymbolId, (usize, Reason<'p>)>>,
}

/// How a header part names a type: complete, or where a forward
/// declaration is enough.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Want {
    Complete,
    Forward,
}

/// What one item prints in its module's header and in its `.cpp`: the
/// types it names (each with how the header needs it) and the expressions
/// and statements it prints.
#[derive(Default)]
struct ItemParts<'p> {
    /// The header's types, with how they are needed, where and why.
    header_types: Vec<(&'p Type, Want, Location, Why)>,
    /// The header's expressions, why each is printed there.
    header_exprs: Vec<(&'p Expr, Why)>,
    /// The `.cpp`'s types, each with where.
    source_types: Vec<(&'p Type, Location)>,
    /// The `.cpp`'s expressions.
    source_exprs: Vec<&'p Expr>,
    /// The `.cpp`'s statement lists.
    source_bodies: Vec<&'p Block>,
}

impl<'p> ItemParts<'p> {
    /// Splits an item between its header (when it goes into one) and its
    /// `.cpp` (modules.md §3.2, w3-modules.md §1.7).
    fn of(item: &'p Item, symbols: &'p SymbolTable, in_header: bool) -> Self {
        let mut parts = ItemParts::default();
        let ty_of = |id: &SymbolId| symbols.get(id).map(|s| (&s.ty, s.origin.location()));
        match &item.kind {
            ItemKind::Main(main) => parts.source_bodies.push(&main.body),
            ItemKind::Function(def) => {
                let signature = def
                    .params
                    .iter()
                    .filter_map(|p| ty_of(&p.symbol))
                    .chain(std::iter::once((&def.ret, item.origin.location())));
                let defaults = def.params.iter().filter_map(|p| p.default.as_ref());
                for (ty, location) in signature {
                    if in_header {
                        parts
                            .header_types
                            .push((ty, Want::Forward, location.clone(), Why::Signature));
                    }
                    parts.source_types.push((ty, location));
                }
                if in_header {
                    parts
                        .header_exprs
                        .extend(defaults.map(|e| (e, Why::DefaultValue)));
                } else {
                    parts.source_exprs.extend(defaults);
                }
                parts.source_bodies.push(&def.body);
            }
            ItemKind::Struct(def) => {
                let Some(SymbolKind::Struct { fields, .. }) = symbols.get(&def.symbol).map(|s| &s.kind)
                else {
                    return parts;
                };
                for field in fields {
                    let Some((ty, location)) = ty_of(field) else {
                        continue;
                    };
                    if in_header {
                        parts
                            .header_types
                            .push((ty, Want::Complete, location, Why::Field));
                    } else {
                        parts.source_types.push((ty, location));
                    }
                }
                if in_header {
                    parts
                        .header_exprs
                        .extend(def.defaults.values().map(|e| (e, Why::FieldDefault)));
                } else {
                    parts.source_exprs.extend(def.defaults.values());
                }
            }
            ItemKind::Global(def) => {
                let Some((ty, location)) = ty_of(&def.symbol) else {
                    return parts;
                };
                match (in_header, def.constexpr) {
                    (true, true) => {
                        parts
                            .header_types
                            .push((ty, Want::Complete, location, Why::GlobalType));
                        parts
                            .header_exprs
                            .extend(def.init.iter().map(|e| (e, Why::Value)));
                    }
                    (true, false) => {
                        parts
                            .header_types
                            .push((ty, Want::Complete, location.clone(), Why::GlobalType));
                        parts.source_types.push((ty, location));
                        parts.source_exprs.extend(def.init.iter());
                    }
                    (false, _) => {
                        parts.source_types.push((ty, location));
                        parts.source_exprs.extend(def.init.iter());
                    }
                }
            }
            ItemKind::Enum(_) | ItemKind::UseHeader { .. } => {}
        }
        parts
    }
}

/// The symbols a statement changes or names by ID, not through an
/// expression: assignment targets, a collection changed, a file written.
fn stmt_symbols(stmt: &Stmt) -> Vec<&SymbolId> {
    match &stmt.kind {
        StmtKind::Assign { target, .. }
        | StmtKind::CompoundAssign { target, .. }
        | StmtKind::Ask { target, .. }
        | StmtKind::SetField { target, .. }
        | StmtKind::Flip { target } => vec![target],
        StmtKind::CollectionOp { collection, .. } => vec![collection],
        StmtKind::WriteFile { file, .. } => vec![file],
        _ => Vec::new(),
    }
}

impl<'p> Needs<'p> {
    /// Walks every item once: a use of another module's shared definition
    /// is a need of the file that prints it (an item's header part is
    /// printed in its module's header when it is shared, the rest in its
    /// `.cpp`). Then each `.cpp` also needs the header of every type that a
    /// header it includes forward-declares.
    fn collect(program: &ProgramView<'p>, layout: &Layout) -> Self {
        let count = program.modules.len();
        let mut needs = Needs {
            source: vec![BTreeMap::new(); count],
            header: vec![BTreeMap::new(); count],
            forward: vec![BTreeMap::new(); count],
        };
        if count < 2 {
            return needs;
        }
        let symbols = program.symbols;
        for (m, module) in program.modules.iter().enumerate() {
            let in_header: BTreeSet<usize> = layout
                .modules
                .get(m)
                .map(|files| files.header_order.iter().copied().collect())
                .unwrap_or_default();
            for (index, item) in module.items.iter().enumerate() {
                let shared = in_header.contains(&index);
                let by = shared.then(|| item_symbol(item)).flatten();
                let parts = ItemParts::of(item, symbols, shared);
                for (ty, want, location, why) in &parts.header_types {
                    for id in user_types(ty) {
                        let place = Place {
                            m,
                            by,
                            why: *why,
                            location,
                        };
                        needs.header_type(program, id, *want, &place);
                    }
                }
                for (expr, why) in &parts.header_exprs {
                    needs.walk(program, m, expr, Some((by, *why)));
                }
                for (ty, location) in &parts.source_types {
                    for id in user_types(ty) {
                        needs.source_use(program, m, id, location);
                    }
                }
                for expr in &parts.source_exprs {
                    needs.walk(program, m, expr, None);
                }
                for body in parts.source_bodies {
                    for stmt in body_stmts(body) {
                        let location = stmt.origin.location();
                        for id in stmt_symbols(stmt) {
                            needs.source_use(program, m, id, &location);
                        }
                        for expr in crate::nesting::parts(stmt).0 {
                            needs.walk(program, m, expr, None);
                        }
                    }
                }
            }
        }
        // The locals' types: variables, loop items and parameters, which a
        // module's `.cpp` declares.
        for (id, symbol) in &symbols.symbols {
            let local = matches!(
                symbol.kind,
                SymbolKind::Variable { .. } | SymbolKind::Parameter { .. } | SymbolKind::LoopVariable
            );
            if local && let Some(m) = program.module_of(id) {
                for used in user_types(&symbol.ty) {
                    needs.source_use(program, m, used, &symbol.origin.location());
                }
            }
        }
        needs.close_sources(layout);
        needs
    }

    /// Records a `.cpp`'s use of the definition `used`, when it is another
    /// module's and goes into that module's header.
    fn source_use(&mut self, program: &ProgramView<'p>, m: usize, used: &'p SymbolId, location: &Location) {
        let Some(def) = program.def(used) else {
            return;
        };
        if def.module == m || !program.item(def).is_some_and(goes_into_header) {
            return;
        }
        if let Some(list) = self.source.get_mut(m) {
            list.entry(def.module).or_insert(Reason {
                location: location.clone(),
                used,
                by: None,
                why: Why::DefaultValue,
            });
        }
    }

    /// Records a header part's use of the definition `used`: an include, or
    /// a forward declaration when one is enough.
    fn header_type(
        &mut self,
        program: &ProgramView<'p>,
        used: &'p SymbolId,
        want: Want,
        place: &Place<'_, 'p>,
    ) {
        let Some(def) = program.def(used) else {
            return;
        };
        if def.module == place.m || !program.item(def).is_some_and(goes_into_header) {
            return;
        }
        let reason = Reason {
            location: place.location.clone(),
            used,
            by: place.by,
            why: place.why,
        };
        match want {
            Want::Complete => self.header_need(place.m, def.module, reason),
            Want::Forward => {
                if let Some(list) = self.forward.get_mut(place.m) {
                    list.entry(used).or_insert((def.module, reason));
                }
            }
        }
    }

    /// Records that module `m`'s header needs module `n`'s: the reason is
    /// kept when it is the first of its shared definition.
    fn header_need(&mut self, m: usize, n: usize, reason: Reason<'p>) {
        if let Some(list) = self.header.get_mut(m) {
            let reasons = list.entry(n).or_default();
            if !reasons.iter().any(|known| known.by == reason.by) {
                reasons.push(reason);
            }
        }
    }

    /// Records the needs of one expression tree of module `m`, printed in
    /// the header for `in_header`'s shared definition and part, or else in
    /// the `.cpp`: each function it calls, each global it reads, each enum
    /// value's enum and each user type its values have.
    fn walk(
        &mut self,
        program: &ProgramView<'p>,
        m: usize,
        root: &'p Expr,
        in_header: Option<(Option<&'p SymbolId>, Why)>,
    ) {
        for (used, location) in named_in(root, program.symbols) {
            match in_header {
                Some((by, why)) => {
                    let place = Place {
                        m,
                        by,
                        why,
                        location: &location,
                    };
                    self.header_type(program, used, Want::Complete, &place);
                }
                None => self.source_use(program, m, used, &location),
            }
        }
    }

    /// Adds to each `.cpp`'s needs the header of every type that a header
    /// it includes (its own, or one it needs, directly or through other
    /// headers) forward-declares, with the reason of that forward
    /// declaration, until nothing is added.
    fn close_sources(&mut self, layout: &Layout) {
        for m in 0..self.source.len() {
            let mut queue: VecDeque<usize> = self.source[m].keys().copied().collect();
            if layout.modules.get(m).is_some_and(|files| files.header) {
                queue.push_back(m);
            }
            let mut seen: BTreeSet<usize> = queue.iter().copied().collect();
            while let Some(h) = queue.pop_front() {
                let next: Vec<usize> = self.header[h].keys().copied().collect();
                for n in next {
                    if seen.insert(n) {
                        queue.push_back(n);
                    }
                }
                let forwarded: Vec<(usize, Reason<'p>)> = self.forward[h].values().cloned().collect();
                for (n, reason) in forwarded {
                    if n != m && !self.source[m].contains_key(&n) {
                        self.source[m].insert(n, reason);
                    }
                    if seen.insert(n) {
                        queue.push_back(n);
                    }
                }
            }
        }
    }
}

/// Where a header part names something: its module, its shared
/// definition, which part and where.
struct Place<'l, 'p> {
    m: usize,
    by: Option<&'p SymbolId>,
    why: Why,
    location: &'l Location,
}

/// Every statement of a list, at any depth of statements, in order.
fn body_stmts(body: &Block) -> Vec<&Stmt> {
    let mut stmts = Vec::new();
    let mut lists = vec![body];
    while let Some(list) = lists.pop() {
        for stmt in &list.stmts {
            stmts.push(stmt);
            lists.extend(crate::nesting::parts(stmt).1.into_iter().rev());
        }
    }
    stmts
}

// --- B2C-E0214: what a shared definition's header shows ----------------------

/// Reports each definition of a shared definition's own module that its
/// header part names but the module does not share (`B2C-E0214`,
/// w3-modules.md §1.9): a function's inputs' and result's types and its
/// default values; a struct's fields' types and default values; a shared
/// global's type. Other modules could not use the declaration. A body, a
/// variable's starting value and a `const` constant's value are printed only
/// in the module's `.cpp`, so what they name never counts, and a constant
/// whose value names something not shared is printed `extern const` (the
/// analyser does not make it `constexpr`); another module's definition that
/// is not shared is `B2C-E0206` where it is used.
fn check_header_parts(program: &ProgramView<'_>, layout: &Layout, diags: &mut Diags) {
    for (m, files) in layout.modules.iter().enumerate() {
        for &index in &files.header_order {
            let Some(item) = program.modules.get(m).and_then(|module| module.items.get(index)) else {
                continue;
            };
            let Some(owner) = item_symbol(item) else {
                continue;
            };
            let mut check = HeaderCheck {
                program,
                m,
                owner: program.name(owner),
                reported: BTreeSet::new(),
                diags: &mut *diags,
            };
            match &item.kind {
                ItemKind::Function(def) => check.function(def, item),
                ItemKind::Struct(def) => check.structure(def),
                ItemKind::Global(def) => check.global(&def.symbol),
                ItemKind::Enum(_) | ItemKind::Main(_) | ItemKind::UseHeader { .. } => {}
            }
        }
    }
}

/// The `B2C-E0214` checks of one shared definition's header part.
struct HeaderCheck<'a, 'p> {
    program: &'a ProgramView<'p>,
    /// Its module.
    m: usize,
    /// Its name, quoted.
    owner: String,
    /// What was reported, by part (a row, or `usize::MAX` for the result
    /// or the type) and definition.
    reported: BTreeSet<(usize, SymbolId)>,
    diags: &'a mut Diags,
}

impl HeaderCheck<'_, '_> {
    /// Whether `id` is a definition of the module that it does not share.
    fn private(&self, id: &SymbolId) -> bool {
        self.program
            .def(id)
            .filter(|d| d.module == self.m)
            .and_then(|d| self.program.item(d))
            .is_some_and(|item| !item.shared)
    }

    fn report(&mut self, row: usize, target: &SymbolId, location: Location, what: &str) {
        if !self.reported.insert((row, target.clone())) {
            return;
        }
        let name = self.program.name(target);
        let message = format!(
            "{} is shared, but {what}, which is not. Share {name} too.",
            self.owner
        );
        let mut diagnostic = Diagnostic::error(
            codes::NOT_SHARED_IN_HEADER,
            DiagSource::Analyser,
            location,
            message,
        );
        if let Some(at) = self.program.location(target) {
            diagnostic = diagnostic.with_related(at, format!("{name} is defined here, not shared"));
        }
        self.diags.push(diagnostic);
    }

    /// The types a part's type names: `has the type` for the type itself,
    /// `uses` for one inside it.
    fn types(&mut self, row: usize, ty: &Type, location: &Location, subject: &str) {
        for id in user_types(ty) {
            if self.private(id) {
                let name = self.program.name(id);
                let what = if *ty == Type::Named(id.clone()) {
                    format!("{subject} has the type {name}")
                } else {
                    format!("the type of {subject} uses {name}")
                };
                self.report(row, id, location.clone(), &what);
            }
        }
    }

    /// The definitions a default value names.
    fn value(&mut self, row: usize, value: &Expr, subject: &str) {
        for (id, location) in named_in(value, self.program.symbols) {
            if self.private(id) {
                let what = format!("the default value of {subject} uses {}", self.program.name(id));
                self.report(row, id, location, &what);
            }
        }
    }

    /// A shared function: its inputs' types, its result's type, its default
    /// values.
    fn function(&mut self, def: &b2c_ir::sast::FunctionDef, item: &Item) {
        let symbols = self.program.symbols;
        for (row, param) in def.params.iter().enumerate() {
            let Some(symbol) = symbols.get(&param.symbol) else {
                continue;
            };
            let subject = format!("its input {}", quoted(symbol.name.as_str()));
            self.types(row, &symbol.ty, &param.origin.location(), &subject);
        }
        let result = Location {
            part: Part::Field {
                name: String::from("RETURNS"),
            },
            ..item.origin.location()
        };
        for id in user_types(&def.ret) {
            if self.private(id) {
                let name = self.program.name(id);
                let what = if def.ret == Type::Named(id.clone()) {
                    format!("its result has the type {name}")
                } else {
                    format!("the type of its result uses {name}")
                };
                self.report(usize::MAX, id, result.clone(), &what);
            }
        }
        for (row, param) in def.params.iter().enumerate() {
            let Some(default) = &param.default else {
                continue;
            };
            let input = symbols
                .get(&param.symbol)
                .map(|s| quoted(s.name.as_str()))
                .unwrap_or_default();
            self.value(row, default, &format!("its input {input}"));
        }
    }

    /// A shared struct: its fields' types and default values.
    fn structure(&mut self, def: &b2c_ir::sast::StructDef) {
        let symbols = self.program.symbols;
        let Some(SymbolKind::Struct { fields, .. }) = symbols.get(&def.symbol).map(|s| &s.kind) else {
            return;
        };
        for (row, field) in fields.iter().enumerate() {
            let Some(symbol) = symbols.get(field) else {
                continue;
            };
            let subject = format!("its field {}", quoted(symbol.name.as_str()));
            self.types(row, &symbol.ty, &symbol.origin.location(), &subject);
            if let Some(value) = def.defaults.get(field) {
                self.value(row, value, &subject);
            }
        }
    }

    /// A shared global: its type.
    fn global(&mut self, symbol: &SymbolId) {
        let Some(found) = self.program.symbols.get(symbol) else {
            return;
        };
        let location = Location {
            part: Part::Field {
                name: String::from("TYPE"),
            },
            ..found.origin.location()
        };
        for id in user_types(&found.ty) {
            if self.private(id) {
                let name = self.program.name(id);
                let what = if found.ty == Type::Named(id.clone()) {
                    format!("its type is {name}")
                } else {
                    format!("its type uses {name}")
                };
                self.report(usize::MAX, id, location.clone(), &what);
            }
        }
    }
}

/// The user types a type names, at any depth, each once, in order.
fn user_types(ty: &Type) -> Vec<&SymbolId> {
    let mut found: Vec<&SymbolId> = Vec::new();
    let mut stack = vec![ty];
    while let Some(ty) = stack.pop() {
        match ty {
            Type::Named(id) if !found.contains(&id) => found.push(id),
            Type::Lib(app) => stack.extend(app.args().iter().rev()),
            _ => {}
        }
    }
    found
}

/// What an expression tree names that a declaration printing it needs: the
/// functions it calls, the globals it reads, each enum value's enum and the
/// user types of its values, each with where.
fn named_in<'e>(root: &'e Expr, symbols: &'e SymbolTable) -> Vec<(&'e SymbolId, Location)> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(expr) = stack.pop() {
        match &expr.kind {
            ExprKind::Call { function, .. } => found.push((function, expr.origin.location())),
            ExprKind::Var(sym)
                if matches!(symbols.get(sym).map(|s| &s.kind), Some(SymbolKind::Global { .. })) =>
            {
                found.push((sym, expr.origin.location()));
            }
            ExprKind::Enumerator(value) => {
                if let Some(SymbolKind::Enumerator { owner }) = symbols.get(value).map(|s| &s.kind) {
                    found.push((owner, expr.origin.location()));
                }
            }
            _ => {}
        }
        for id in user_types(&expr.ty) {
            found.push((id, expr.origin.location()));
        }
        let mut children = crate::flow::children(expr);
        children.reverse();
        stack.extend(children);
    }
    found
}

// --- B2C-E0422: headers that would include each other -----------------------

/// Reports headers that would include each other (`B2C-E0422`,
/// w3-modules.md §2.2, §2.4) when no circle of definitions explains it: on
/// each shared definition whose header part makes its header need the next
/// one round the circle, with the rest of the circle as related locations.
/// A need between two definitions of one circle of `explained` (the
/// declaration graph's circles that join modules, already `B2C-E0420` or
/// `B2C-E0421`) is not reported; every other need between two headers of a
/// circle closes a circle that no circle of definitions explains.
fn check_header_cycles(
    program: &ProgramView<'_>,
    layout: &Layout,
    needs: &Needs<'_>,
    explained: &[BTreeSet<&SymbolId>],
    diags: &mut Diags,
) {
    let is_explained = |reason: &Reason<'_>| {
        reason.by.is_some_and(|by| {
            explained
                .iter()
                .any(|circle| circle.contains(by) && circle.contains(reason.used))
        })
    };
    // Which headers each header reaches through the includes (at most 256
    // modules, so a walk from each is cheap).
    let count = program.modules.len();
    let reach: Vec<BTreeSet<usize>> = (0..count)
        .map(|start| {
            let mut seen = BTreeSet::new();
            let mut queue: VecDeque<usize> = needs
                .header
                .get(start)
                .into_iter()
                .flat_map(BTreeMap::keys)
                .copied()
                .collect();
            while let Some(m) = queue.pop_front() {
                if seen.insert(m) {
                    queue.extend(needs.header.get(m).into_iter().flat_map(BTreeMap::keys).copied());
                }
            }
            seen
        })
        .collect();
    let mut done: BTreeSet<usize> = BTreeSet::new();
    for start in 0..count {
        if done.contains(&start) || !reach[start].contains(&start) {
            continue;
        }
        // The headers on a circle with `start`.
        let members: BTreeSet<usize> = reach[start]
            .iter()
            .copied()
            .filter(|m| reach[*m].contains(&start))
            .collect();
        done.extend(members.iter().copied());
        for &a in &members {
            for (&b, reasons) in needs.header.get(a).into_iter().flatten() {
                if !members.contains(&b) {
                    continue;
                }
                for reason in reasons.iter().filter(|reason| !is_explained(reason)) {
                    report_header_cycle(program, layout, needs, &members, (a, b, reason), diags);
                }
            }
        }
    }
}

/// `B2C-E0422` on the need of `a`'s header on `b`'s, which closes a circle
/// of headers through `members`: the way back from `b` to `a` is the
/// shortest one, shown in the message and as related locations.
fn report_header_cycle(
    program: &ProgramView<'_>,
    layout: &Layout,
    needs: &Needs<'_>,
    members: &BTreeSet<usize>,
    (a, b, reason): (usize, usize, &Reason<'_>),
    diags: &mut Diags,
) {
    // Breadth first from `b` back to `a`, inside the circle.
    let mut came: BTreeMap<usize, (usize, &Reason<'_>)> = BTreeMap::new();
    let mut queue = VecDeque::from([b]);
    while let Some(m) = queue.pop_front() {
        if m == a {
            break;
        }
        for (&n, whys) in needs.header.get(m).into_iter().flatten() {
            let Some(why) = whys.first() else {
                continue;
            };
            if members.contains(&n) && n != b && !came.contains_key(&n) {
                came.insert(n, (m, why));
                queue.push_back(n);
            }
        }
    }
    let mut back: Vec<(usize, usize, &Reason<'_>)> = Vec::new();
    let mut at = a;
    while at != b {
        let Some(&(from, why)) = came.get(&at) else {
            break;
        };
        back.push((from, at, why));
        at = from;
    }
    back.reverse();
    let by = reason.by.map(|by| program.name(by)).unwrap_or_default();
    let through: Vec<String> = back
        .iter()
        .skip(1)
        .map(|(from, _, _)| header_name(layout, *from))
        .collect();
    let way_back = if through.is_empty() {
        String::new()
    } else {
        format!(" through {}", listing(&through))
    };
    // One definition to move on each side only when each side's need has a
    // single reason: with two, moving one leaves the other's need.
    let single = |from: usize, to: usize| {
        needs
            .header
            .get(from)
            .and_then(|list| list.get(&to))
            .is_some_and(|reasons| reasons.len() == 1)
    };
    let functions = std::iter::once(reason)
        .chain(back.iter().map(|(_, _, why)| *why))
        .all(|why| why.why == Why::DefaultValue);
    let fix = match back.as_slice() {
        [(_, _, only)] if single(a, b) && single(b, a) => format!(
            "Move {by} into module {}, or {} into module {}.",
            program.module_name(b),
            program.name(only.by.unwrap_or(only.used)),
            program.module_name(a)
        ),
        _ if functions => String::from("Move one of these shared functions into another module."),
        _ => String::from("Move one of these shared definitions into another module."),
    };
    let message = format!(
        "{} would need {}, because {} of module {}, but {} already needs {}{way_back}. Two headers can't each \
         include the other. {fix}",
        header_name(layout, a),
        header_name(layout, b),
        reason.cause(program),
        program.module_name(b),
        header_name(layout, b),
        header_name(layout, a),
    );
    let mut diagnostic = Diagnostic::error(
        codes::HEADER_CYCLE,
        DiagSource::Analyser,
        reason.location.clone(),
        message,
    );
    for (from, to, why) in back {
        let note = format!(
            "{} needs {} for {}",
            header_name(layout, from),
            header_name(layout, to),
            why.part(program)
        );
        diagnostic = diagnostic.with_related(why.location.clone(), note);
    }
    diags.push(diagnostic);
}

/// Names joined for a sentence: `a`, `a and b`, `a, b and c`.
fn listing(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

// --- B2C-E0210 and B2C-E0213: names of shared definitions --------------------

/// How a module's `.cpp` comes to include a header ([`reached`]).
#[derive(Debug, Clone, Copy)]
enum Link<'n, 'p> {
    /// It is the module's own header.
    Own,
    /// The `.cpp` uses a definition the header declares.
    Uses(&'n Reason<'p>),
    /// A header the `.cpp` includes (by module) includes it.
    Through(usize, &'n Reason<'p>),
}

/// The headers module `m`'s `.cpp` includes, directly or through other
/// headers, each with how it is first reached (breadth first, so by a
/// shortest chain).
fn reached<'n, 'p>(layout: &Layout, needs: &'n Needs<'p>, m: usize) -> BTreeMap<usize, Link<'n, 'p>> {
    let mut found: BTreeMap<usize, Link<'n, 'p>> = BTreeMap::new();
    let mut queue = VecDeque::new();
    if layout.modules.get(m).is_some_and(|files| files.header) {
        found.insert(m, Link::Own);
        queue.push_back(m);
    }
    for (&n, reason) in needs.source.get(m).into_iter().flatten() {
        if let std::collections::btree_map::Entry::Vacant(entry) = found.entry(n) {
            entry.insert(Link::Uses(reason));
            queue.push_back(n);
        }
    }
    while let Some(h) = queue.pop_front() {
        for (&n, reasons) in needs.header.get(h).into_iter().flatten() {
            let Some(reason) = reasons.first() else {
                continue;
            };
            if let std::collections::btree_map::Entry::Vacant(entry) = found.entry(n) {
                entry.insert(Link::Through(h, reason));
                queue.push_back(n);
            }
        }
    }
    found
}

/// A shared definition of the program: its symbol, its module and whether
/// it is a function.
#[derive(Debug, Clone, Copy)]
struct SharedDef<'p> {
    symbol: &'p SymbolId,
    module: usize,
    function: bool,
}

/// Reports the names of shared definitions that C++ could not tell apart
/// (w3-modules.md §2.1): two modules sharing definitions with one name
/// (`B2C-E0210`; two shared functions are `B2C-E0211`, reported with the
/// functions), and a definition that is not shared but has the name of a
/// shared one (`B2C-E0213`, W3M-D10): of its own module, whose `.cpp`
/// declares both (only two functions get here: any other pair of one module
/// is `B2C-E0210` where it is declared), or of any other module, since the
/// `.cpp` includes that module's header as soon as it uses anything the
/// header declares, and C++ then could not tell the two apart (`::scale`
/// would silently reach the other one). The rule is program-wide, so it
/// never depends on what the module uses; the message names the cause when
/// the `.cpp` does include the header, with the use and each include of the
/// chain as related locations.
fn check_shared_names(program: &ProgramView<'_>, layout: &Layout, needs: &Needs<'_>, diags: &mut Diags) {
    let mut shared: BTreeMap<&str, Vec<SharedDef<'_>>> = BTreeMap::new();
    for (m, module) in program.modules.iter().enumerate() {
        for item in module.items.iter().filter(|item| item.shared) {
            if let Some(symbol) = item_symbol(item)
                && let Some(found) = program.symbols.get(symbol)
            {
                shared.entry(found.name.as_str()).or_default().push(SharedDef {
                    symbol,
                    module: m,
                    function: matches!(item.kind, ItemKind::Function(_)),
                });
            }
        }
    }
    if shared.is_empty() {
        return;
    }
    check_shared_twice(program, &shared, diags);
    for (m, module) in program.modules.iter().enumerate() {
        let mut paths = None;
        for item in module.items.iter().filter(|item| !item.shared) {
            let Some(symbol_id) = item_symbol(item) else {
                continue;
            };
            let Some(symbol) = program.symbols.get(symbol_id) else {
                continue;
            };
            let Some(list) = shared.get(symbol.name.as_str()) else {
                continue;
            };
            let function = matches!(item.kind, ItemKind::Function(_));
            let paths = paths.get_or_insert_with(|| reached(layout, needs, m));
            // The module's own shared function first (another definition of
            // the module with that name is `B2C-E0210`), then one whose
            // header the file includes, then the first one of another module.
            let own = list
                .iter()
                .find(|def| def.module == m && def.function && function);
            let others = || list.iter().filter(|def| def.module != m);
            let Some(other) = own
                .or_else(|| others().find(|def| paths.contains_key(&def.module)))
                .or_else(|| others().next())
                .copied()
            else {
                continue;
            };
            let name = quoted(symbol.name.as_str());
            let location = symbol.origin.location();
            let diagnostic = if other.module == m {
                let message = format!(
                    "{name} is also the name of a function this module shares, and C++ could not tell the two \
                     apart. Rename one of them, or share this one too."
                );
                Diagnostic::error(codes::SHARED_NAME_CLASH, DiagSource::Analyser, location, message)
            } else {
                let kinds = (program.kind_word(symbol_id), program.kind_word(other.symbol));
                shared_name_clash(program, layout, paths, (&name, location, kinds), other.module)
            };
            let diagnostic = match program.location(other.symbol) {
                Some(at) => diagnostic.with_related(at, format!("the shared {name}")),
                None => diagnostic,
            };
            diags.push(diagnostic);
        }
    }
}

/// `B2C-E0210` on each shared definition with the name of a shared
/// definition of an earlier module, unless both are functions
/// (`B2C-E0211`): the headers would declare one name twice.
fn check_shared_twice(
    program: &ProgramView<'_>,
    shared: &BTreeMap<&str, Vec<SharedDef<'_>>>,
    diags: &mut Diags,
) {
    for (name, list) in shared {
        for (at, def) in list.iter().enumerate() {
            let Some(first) = list[..at]
                .iter()
                .find(|other| other.module != def.module && !(other.function && def.function))
            else {
                continue;
            };
            let Some(location) = program.location(def.symbol) else {
                continue;
            };
            let message = format!(
                "{} is already shared by module {}. Two modules can't share definitions with one name, so \
                 rename one of them.",
                quoted(name),
                program.module_name(first.module)
            );
            let mut diagnostic =
                Diagnostic::error(codes::DUPLICATE_NAME, DiagSource::Analyser, location, message);
            if let Some(other) = program.location(first.symbol) {
                diagnostic = diagnostic.with_related(other, format!("the other shared {}", quoted(name)));
            }
            diags.push(diagnostic);
        }
    }
}

/// `B2C-E0213` for a definition named like another module's (`owner`'s)
/// shared definition, the kinds of both given: with the chain of includes
/// that brings its header into this file, if one does.
fn shared_name_clash(
    program: &ProgramView<'_>,
    layout: &Layout,
    paths: &BTreeMap<usize, Link<'_, '_>>,
    (name, location, (kind, other_kind)): (&str, Location, (&str, &str)),
    owner: usize,
) -> Diagnostic {
    let module = program.module_name(owner);
    let header = header_name(layout, owner);
    if !paths.contains_key(&owner) {
        let message = format!(
            "{name} is also shared by module {module}. A {kind} that is not shared can't have the name of a \
             shared {other_kind} of any module: once this file includes {header}, C++ could not tell the two \
             apart. Rename this {name}."
        );
        return Diagnostic::error(codes::SHARED_NAME_CLASH, DiagSource::Analyser, location, message);
    }
    // The chain, from the first header the `.cpp` includes to `owner`'s.
    let mut chain: Vec<(usize, Link<'_, '_>)> = Vec::new();
    let mut at = owner;
    while let Some(&link) = paths.get(&at) {
        chain.push((at, link));
        match link {
            Link::Through(from, _) if chain.len() <= paths.len() => at = from,
            _ => break,
        }
    }
    chain.reverse();
    let through: Vec<String> = chain[..chain.len() - 1]
        .iter()
        .map(|(h, link)| match link {
            Link::Own => format!("its own header {}", header_name(layout, *h)),
            _ => header_name(layout, *h),
        })
        .collect();
    let how = match chain.first() {
        Some((_, Link::Uses(reason))) if through.is_empty() => {
            format!("because it uses {}", program.name(reason.used))
        }
        Some((_, Link::Uses(reason))) => format!(
            "through {}, which it includes because it uses {}",
            listing(&through),
            program.name(reason.used)
        ),
        _ => format!("through {}", listing(&through)),
    };
    let message = format!(
        "{name} is also shared by module {module}, whose header {header} this file includes {how}. Rename this \
         {name}."
    );
    let mut diagnostic = Diagnostic::error(codes::SHARED_NAME_CLASH, DiagSource::Analyser, location, message);
    for (h, link) in &chain {
        match link {
            Link::Own => {}
            Link::Uses(reason) => {
                let note = format!(
                    "this file uses {} of module {} here",
                    program.name(reason.used),
                    program.module_name(*h)
                );
                diagnostic = diagnostic.with_related(reason.location.clone(), note);
            }
            Link::Through(from, reason) => {
                let note = format!(
                    "{} includes {} for {}",
                    header_name(layout, *from),
                    header_name(layout, *h),
                    reason.part(program)
                );
                diagnostic = diagnostic.with_related(reason.location.clone(), note);
            }
        }
    }
    diagnostic
}

#[cfg(test)]
mod tests {
    use b2c_ir::headers::ItemHeaders;
    use b2c_ir::ids::{BlockId, ModuleId, SymbolId};
    use b2c_ir::sast::{Block, EnumDef, FunctionDef, MainDef, Origin};
    use b2c_ir::types::Type;

    use super::*;

    fn item(kind: ItemKind, shared: bool) -> Item {
        Item {
            kind,
            origin: Origin::whole(ModuleId::new("m").expect("id"), BlockId::new("b").expect("id")),
            comment: None,
            headers: ItemHeaders::default(),
            shared,
            namespace: None,
        }
    }

    fn function(shared: bool) -> Item {
        item(
            ItemKind::Function(FunctionDef {
                symbol: SymbolId::new("f").expect("id"),
                params: Vec::new(),
                ret: Type::Void,
                body: Block::default(),
            }),
            shared,
        )
    }

    fn module(name: &str, items: Vec<Item>) -> Module {
        Module {
            id: ModuleId::new(&format!("mod_{name}")).expect("id"),
            name: name.to_owned(),
            items,
        }
    }

    #[test]
    fn shared_definitions_go_into_a_header() {
        let main = item(
            ItemKind::Main(MainDef {
                body: Block::default(),
                args: None,
            }),
            true,
        );
        let colour = item(
            ItemKind::Enum(EnumDef {
                symbol: SymbolId::new("e").expect("id"),
            }),
            true,
        );
        let module = module("utils", vec![main, function(true), function(false), colour]);
        assert_eq!(shared_items(&module), [1, 3]);
        let mut diags = Diags::default();
        // Items whose symbols the table lacks are not in the declaration
        // graph's order, so they reach no header.
        let layout = compute_layout(&[module], &SymbolTable::default(), &BTreeMap::new(), &mut diags);
        assert!(diags.list.is_empty());
        let files = &layout.modules[0];
        assert!(!files.header);
        assert!(files.header_order.is_empty() && files.source_includes.is_empty());
    }

    #[test]
    fn a_header_lists_the_shared_items_in_graph_order_and_its_cpp_includes_it() {
        let mut layout = Layout::flat(&[module("utils", Vec::new())], &SymbolTable::default());
        let files = &mut layout.modules[0];
        files.source_order = vec![3, 0, 2, 1];
        files.header_order = header_parts(files, &[1, 3]);
        assert_eq!(files.header_order, [3, 1]);
        files.header = true;
        includes(&mut layout, &Needs::default(), &SymbolTable::default());
        assert_eq!(
            layout.modules[0].source_includes,
            [ModuleId::new("mod_utils").expect("id")]
        );
        layout.modules[0].header = false;
        includes(&mut layout, &Needs::default(), &SymbolTable::default());
        assert!(layout.modules[0].source_includes.is_empty());
    }

    #[test]
    fn module_names_starting_with_b2c_are_reported_and_numbered() {
        let modules = [
            module("main", Vec::new()),
            module("b2c_tools", Vec::new()),
            module("b2cx", Vec::new()),
            module("xb2c", Vec::new()),
        ];
        let mut diags = Diags::default();
        let layout = compute_layout(&modules, &SymbolTable::default(), &BTreeMap::new(), &mut diags);
        let reported: Vec<(&str, Option<&str>, &str)> = diags
            .list
            .iter()
            .map(|d| {
                (
                    d.code.0.as_str(),
                    d.primary.module.as_ref().map(ModuleId::as_str),
                    d.message.as_str(),
                )
            })
            .collect();
        assert_eq!(
            reported,
            [
                (
                    codes::RESERVED_MODULE_NAME,
                    Some("mod_b2c_tools"),
                    RESERVED_MODULE_MESSAGE
                ),
                (
                    codes::RESERVED_MODULE_NAME,
                    Some("mod_b2cx"),
                    RESERVED_MODULE_MESSAGE
                ),
            ]
        );
        assert!(diags.list.iter().all(|d| d.primary.block.is_none()));
        let stems: Vec<&str> = layout.modules.iter().map(|files| files.stem.as_str()).collect();
        assert_eq!(stems, ["main", "module2", "module3", "xb2c"]);
    }
}
