//! The program's files (ADR-0019; `docs/plans/m3-contracts/modules.md` §1,
//! §3, §4; `w3-modules.md` §1.7, §2.1 to §2.4): which files each module
//! gets, what they include and in which order their declarations are
//! printed. The analyser computes the [`Layout`] and reports its problems;
//! code generation only prints it.
//!
//! [`compute_layout`] starts from milestone M2's flat layout (one `.cpp` per
//! module, named by its [`FileStem`], or `module<N>` for a name the stem
//! refuses), orders each module's declarations with the declaration graph
//! (`crate::order`), and then asks three hooks what goes into headers:
//!
//! * [`shared_items`]: the items whose declarations go into their module's
//!   header. Package F turned functions on ([`FUNCTIONS_IN_HEADERS`]: a
//!   shared function's declaration, with its default values); package T
//!   turns on structs, enums, globals and constants
//!   ([`TYPES_AND_GLOBALS_IN_HEADERS`]).
//! * [`header_parts`]: the order of those declarations in the header
//!   (`header_order`), the declaration graph's; T's program-wide graph
//!   replaces it.
//! * [`includes`]: the project headers each file includes. A `.cpp`
//!   includes its own header and the header of each module whose shared
//!   functions its code calls; a header includes the header of each module
//!   whose shared functions the default values it prints call ([`Needs`]).
//!   T adds types, constants and globals, and the forward declarations.
//!
//! The headers bring the rules of w3-modules.md §2.1 to §2.4, reported here
//! for functions in a project of several modules:
//!
//! * `B2C-E0213`: a function that is not shared has the name of a shared
//!   function, of its own module or of any other (W3M-D10: program-wide, so
//!   it never appears because of an unrelated edit). The message names the
//!   cause when the module's file does include that header, with the chain
//!   of includes as related locations.
//! * `B2C-E0214`: what a shared function's header part shows (its inputs'
//!   types, its result's type, its default values) names a definition of
//!   its module that is not shared.
//! * `B2C-E0421`: functions of different modules whose default values call
//!   each other in a circle (a circle within one module is the declaration
//!   graph's).
//! * `B2C-E0422`: headers that would include each other when no such circle
//!   of definitions explains it.
//!
//! A single-module project shares nothing, so it gets M2's layout and its
//! C++ does not change. The layout also reports `B2C-E0221` for a module
//! whose name starts with `b2c` (modules.md §2), in every project.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use b2c_ir::diag::{DiagSource, Diagnostic, Location, Part};
use b2c_ir::ids::SymbolId;
use b2c_ir::layout::{Layout, ModuleFiles};
use b2c_ir::sast::{Block, Expr, ExprKind, Item, ItemKind, Module, SymbolKind, SymbolTable};
use b2c_ir::text::{FileStem, StemError};
use b2c_ir::types::Type;

use crate::codes;
use crate::messages::{Diags, quoted};
use crate::order::{Dep, Graph, Need, Node, NodeKind};

/// Whether a shared function's declaration goes into its module's header
/// (package F, w3-modules.md §1.2).
const FUNCTIONS_IN_HEADERS: bool = true;

/// Whether a shared struct, enum, global or constant goes into its module's
/// header (package T, w3-modules.md §1.3).
const TYPES_AND_GLOBALS_IN_HEADERS: bool = false;

/// The message of `B2C-E0221` (modules.md §2).
const RESERVED_MODULE_MESSAGE: &str =
    "Module names that start with `b2c` are kept for files that Blocks2Cpp writes. Rename the module.";

/// The layout of a lowered program, with its problems: the declaration
/// graph's cycles (`B2C-E0420`, `B2C-E0421`), module names that start with
/// `b2c` (`B2C-E0221`), and the rules of shared functions (`B2C-E0213`,
/// `B2C-E0214`, `B2C-E0421` across modules, `B2C-E0422`).
pub(crate) fn compute_layout(modules: &[Module], symbols: &SymbolTable, diags: &mut Diags) -> Layout {
    check_module_names(modules, diags);
    let mut layout = Layout::flat(modules, symbols);
    crate::order::order_declarations(modules, symbols, &mut layout, diags);
    for (module, files) in modules.iter().zip(&mut layout.modules) {
        let shared = shared_items(module);
        files.header_order = header_parts(files, &shared);
        files.header = !files.header_order.is_empty();
    }
    let program = ProgramView::new(modules, symbols);
    let needs = Needs::collect(&program, &layout);
    includes(&mut layout, &needs);
    if layout.multi_module() {
        check_header_parts(&program, &layout, diags);
        let explained = check_function_cycles(&program, diags);
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
/// sets only in a project of several modules) of a kind headers hold yet.
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
            ItemKind::Function(_) => FUNCTIONS_IN_HEADERS,
            ItemKind::Struct(_) | ItemKind::Enum(_) | ItemKind::Global(_) => TYPES_AND_GLOBALS_IN_HEADERS,
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

/// The project headers each file includes, each list sorted by file name. A
/// `.cpp` includes its own header, before its definitions (w3-modules.md
/// §1.7: without it a `const` constant's definition has internal linkage
/// and does not link), and the header of every module its code needs; a
/// header includes the header of every module its declarations need.
pub(crate) fn includes(layout: &mut Layout, needs: &Needs<'_>) {
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
        files.header_includes = if files.header {
            listed(
                needs
                    .header
                    .get(m)
                    .into_iter()
                    .flat_map(BTreeMap::keys)
                    .copied()
                    .collect(),
            )
        } else {
            Vec::new()
        };
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
}

/// The symbol an item defines (a function, struct, enum or global).
fn item_symbol(item: &Item) -> Option<&SymbolId> {
    match &item.kind {
        ItemKind::Function(def) => Some(&def.symbol),
        ItemKind::Struct(def) => Some(&def.symbol),
        ItemKind::Enum(def) => Some(&def.symbol),
        ItemKind::Global(def) => Some(&def.symbol),
        ItemKind::Main(_) | ItemKind::UseHeader { .. } => None,
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

/// Why a file needs another module's header: the first use of one of that
/// module's shared definitions in it.
#[derive(Debug, Clone)]
pub(crate) struct Reason<'p> {
    /// Where the use is.
    location: Location,
    /// The shared definition used.
    used: &'p SymbolId,
    /// For a header's need, the shared definition of the module whose
    /// header part (a default value) uses it.
    by: Option<&'p SymbolId>,
}

/// What each module's files need of the other modules' headers
/// (w3-modules.md §2.4): for a `.cpp`, the first reason for each; for a
/// header, the first reason of each shared function whose default values
/// need it, in order, since `B2C-E0422` goes on each of them. The first
/// reason is the one the include and the chain of `B2C-E0213` name.
#[derive(Debug, Clone, Default)]
pub(crate) struct Needs<'p> {
    /// By module index: the modules whose headers its `.cpp` needs.
    source: Vec<BTreeMap<usize, Reason<'p>>>,
    /// By module index: the modules whose headers its header needs, each
    /// with at least one reason.
    header: Vec<BTreeMap<usize, Vec<Reason<'p>>>>,
}

impl<'p> Needs<'p> {
    /// Walks every item once: a call to another module's function that goes
    /// into that module's header is a need of the file that prints the call
    /// (a function's default values are printed with its declaration, so in
    /// its header when it has one).
    fn collect(program: &ProgramView<'p>, layout: &Layout) -> Self {
        let count = program.modules.len();
        let mut needs = Needs {
            source: vec![BTreeMap::new(); count],
            header: vec![BTreeMap::new(); count],
        };
        if count < 2 {
            return needs;
        }
        for (m, module) in program.modules.iter().enumerate() {
            let in_header: BTreeSet<usize> = layout
                .modules
                .get(m)
                .map(|files| files.header_order.iter().copied().collect())
                .unwrap_or_default();
            for (index, item) in module.items.iter().enumerate() {
                let (printed, declared) = item_exprs(item);
                let (to_header, by) = match &item.kind {
                    ItemKind::Function(def) if in_header.contains(&index) => (true, Some(&def.symbol)),
                    _ => (false, None),
                };
                for expr in printed {
                    needs.walk(program, m, expr, None, false);
                }
                for expr in declared {
                    needs.walk(program, m, expr, by, to_header);
                }
            }
        }
        needs
    }

    /// Records the needs of one expression tree of module `m`.
    fn walk(
        &mut self,
        program: &ProgramView<'p>,
        m: usize,
        root: &'p Expr,
        by: Option<&'p SymbolId>,
        header: bool,
    ) {
        let mut stack = vec![root];
        while let Some(expr) = stack.pop() {
            if let ExprKind::Call { function, .. } = &expr.kind
                && let Some(def) = program.def(function)
                && def.module != m
                && program.item(def).is_some_and(goes_into_header)
            {
                let reason = Reason {
                    location: expr.origin.location(),
                    used: function,
                    by,
                };
                if header {
                    if let Some(list) = self.header.get_mut(m) {
                        let reasons = list.entry(def.module).or_default();
                        if !reasons.iter().any(|known| known.by == by) {
                            reasons.push(reason);
                        }
                    }
                } else if let Some(list) = self.source.get_mut(m) {
                    list.entry(def.module).or_insert(reason);
                }
            }
            let mut children = crate::flow::children(expr);
            children.reverse();
            stack.extend(children);
        }
    }
}

/// The expressions of an item: those printed only with its definition (a
/// body, a global's starting value, a struct's default values), and those
/// printed with its declaration (a function's default values).
fn item_exprs(item: &Item) -> (Vec<&Expr>, Vec<&Expr>) {
    match &item.kind {
        ItemKind::Main(main) => (body_exprs(&main.body), Vec::new()),
        ItemKind::Function(def) => (
            body_exprs(&def.body),
            def.params.iter().filter_map(|p| p.default.as_ref()).collect(),
        ),
        ItemKind::Global(def) => (def.init.iter().collect(), Vec::new()),
        ItemKind::Struct(def) => (def.defaults.values().collect(), Vec::new()),
        ItemKind::Enum(_) | ItemKind::UseHeader { .. } => (Vec::new(), Vec::new()),
    }
}

/// Every expression directly in a statement of a list, at any depth of
/// statements, in order.
fn body_exprs(body: &Block) -> Vec<&Expr> {
    let mut exprs = Vec::new();
    let mut lists = vec![body];
    while let Some(list) = lists.pop() {
        for stmt in &list.stmts {
            let (values, nested) = crate::nesting::parts(stmt);
            exprs.extend(values);
            lists.extend(nested.into_iter().rev());
        }
    }
    exprs
}

// --- B2C-E0214: what a shared function's header shows ------------------------

/// Reports each definition of a shared function's own module that its
/// header part names but the module does not share (`B2C-E0214`,
/// w3-modules.md §1.9): the types of its inputs and its result, and what
/// its default values call, read or build. Other modules could not use the
/// declaration. A body is printed only in the module's `.cpp`, so what it
/// names never counts; another module's definition that is not shared is
/// `B2C-E0206` where it is used.
fn check_header_parts(program: &ProgramView<'_>, layout: &Layout, diags: &mut Diags) {
    for (m, files) in layout.modules.iter().enumerate() {
        for &index in &files.header_order {
            let Some(item) = program.modules.get(m).and_then(|module| module.items.get(index)) else {
                continue;
            };
            let ItemKind::Function(def) = &item.kind else {
                continue;
            };
            let function = program.name(&def.symbol);
            let mut reported: BTreeSet<(usize, SymbolId)> = BTreeSet::new();
            let mut report =
                |diags: &mut Diags, row: usize, target: &SymbolId, location: Location, what: String| {
                    if !reported.insert((row, target.clone())) {
                        return;
                    }
                    let name = program.name(target);
                    let message =
                        format!("{function} is shared, but {what}, which is not. Share {name} too.");
                    let mut diagnostic = Diagnostic::error(
                        codes::NOT_SHARED_IN_HEADER,
                        DiagSource::Analyser,
                        location,
                        message,
                    );
                    if let Some(at) = program.location(target) {
                        diagnostic =
                            diagnostic.with_related(at, format!("{name} is defined here, not shared"));
                    }
                    diags.push(diagnostic);
                };
            let private = |id: &SymbolId| {
                program
                    .def(id)
                    .filter(|d| d.module == m)
                    .and_then(|d| program.item(d))
                    .is_some_and(|item| !item.shared)
            };
            for (row, param) in def.params.iter().enumerate() {
                let Some(symbol) = program.symbols.get(&param.symbol) else {
                    continue;
                };
                for id in user_types(&symbol.ty) {
                    if private(id) {
                        let what = if symbol.ty == Type::Named(id.clone()) {
                            format!(
                                "its input {} has the type {}",
                                quoted(symbol.name.as_str()),
                                program.name(id)
                            )
                        } else {
                            format!(
                                "the type of its input {} uses {}",
                                quoted(symbol.name.as_str()),
                                program.name(id)
                            )
                        };
                        report(diags, row, id, param.origin.location(), what);
                    }
                }
            }
            let result = Location {
                part: Part::Field {
                    name: String::from("RETURNS"),
                },
                ..item.origin.location()
            };
            for id in user_types(&def.ret) {
                if private(id) {
                    let what = if def.ret == Type::Named(id.clone()) {
                        format!("its result has the type {}", program.name(id))
                    } else {
                        format!("the type of its result uses {}", program.name(id))
                    };
                    report(diags, usize::MAX, id, result.clone(), what);
                }
            }
            for (row, param) in def.params.iter().enumerate() {
                let Some(default) = &param.default else {
                    continue;
                };
                let input = program
                    .symbols
                    .get(&param.symbol)
                    .map(|s| quoted(s.name.as_str()))
                    .unwrap_or_default();
                for (id, location) in named_in(default, program.symbols) {
                    if private(id) {
                        let what =
                            format!("the default value of its input {input} uses {}", program.name(id));
                        report(diags, row, id, location, what);
                    }
                }
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
/// functions it calls, the globals it reads and the user types of its
/// values, each with where.
fn named_in<'e>(root: &'e Expr, symbols: &SymbolTable) -> Vec<(&'e SymbolId, Location)> {
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

// --- B2C-E0421 and B2C-E0422: circles across modules -------------------------

/// Reports functions of different modules whose default values call each
/// other in a circle (`B2C-E0421`, w3-modules.md §2.2): each needs the
/// other's declaration first, which no order of headers gives. A circle
/// within one module is the declaration graph's, reported with the module,
/// so a function on one is not reported again when a wider circle across
/// modules holds it (one code per cycle). Returns the modules of each circle reported, which explain the headers
/// that would include each other because of it (`B2C-E0422` is not
/// reported again for them).
fn check_function_cycles(program: &ProgramView<'_>, diags: &mut Diags) -> Vec<BTreeSet<usize>> {
    // Every function of the program, with what its default values call.
    let mut graph = Graph::default();
    let mut node_of: BTreeMap<&SymbolId, usize> = BTreeMap::new();
    let mut places: Vec<(usize, &Item)> = Vec::new();
    for (m, module) in program.modules.iter().enumerate() {
        for (index, item) in module.items.iter().enumerate() {
            let ItemKind::Function(def) = &item.kind else {
                continue;
            };
            let Some(symbol) = program.symbols.get(&def.symbol) else {
                continue;
            };
            if node_of.contains_key(&def.symbol) {
                continue;
            }
            let node = graph.add(Node {
                kind: NodeKind::Function,
                name: symbol.name.as_str().to_owned(),
                seq: 0,
                block: item.origin.block.clone(),
                item: index,
            });
            node_of.insert(&def.symbol, node);
            places.push((m, item));
        }
    }
    let mut crossing: BTreeSet<(usize, usize)> = BTreeSet::new();
    for (node, (m, item)) in places.iter().enumerate() {
        let ItemKind::Function(def) = &item.kind else {
            continue;
        };
        let mut seen = BTreeSet::new();
        for default in def.params.iter().filter_map(|p| p.default.as_ref()) {
            for (called, location) in calls_in(default) {
                let Some(&on) = node_of.get(called) else {
                    continue;
                };
                if on == node || !seen.insert(on) {
                    continue;
                }
                if places[on].0 != *m {
                    crossing.insert((node, on));
                }
                graph.deps[node].push(Dep {
                    on,
                    need: Need::Complete,
                    via: Some((graph.nodes[on].name.clone(), location)),
                    field: false,
                });
            }
        }
    }
    if crossing.is_empty() {
        return Vec::new();
    }
    // The functions on a circle within one module, which its declaration
    // graph has reported.
    let mut local = graph.clone();
    for (node, deps) in local.deps.iter_mut().enumerate() {
        deps.retain(|dep| places[dep.on].0 == places[node].0);
    }
    let reported: BTreeSet<usize> = local.cycles().into_iter().flatten().collect();
    // Only the circles that cross a module, alone in a graph of their own.
    let mut circles = Graph::default();
    let mut skipped = BTreeSet::new();
    let mut explained = Vec::new();
    for cycle in graph.cycles() {
        let members: BTreeSet<usize> = cycle.iter().copied().collect();
        let crosses = crossing
            .iter()
            .any(|(a, b)| members.contains(a) && members.contains(b));
        if !crosses {
            continue;
        }
        explained.push(cycle.iter().map(|node| places[*node].0).collect());
        let renumbered: BTreeMap<usize, usize> = cycle
            .iter()
            .map(|&node| (node, circles.add(graph.nodes[node].clone())))
            .collect();
        skipped.extend(
            renumbered
                .iter()
                .filter(|(old, _)| reported.contains(old))
                .map(|(_, &new)| new),
        );
        for (&old, &new) in &renumbered {
            for dep in &graph.deps[old] {
                if let Some(&on) = renumbered.get(&dep.on) {
                    circles.deps[new].push(Dep { on, ..dep.clone() });
                }
            }
        }
    }
    circles.report_cycles_except(&skipped, diags);
    explained
}

/// The functions an expression tree calls, with where, in order.
fn calls_in(root: &Expr) -> Vec<(&SymbolId, Location)> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(expr) = stack.pop() {
        if let ExprKind::Call { function, .. } = &expr.kind {
            found.push((function, expr.origin.location()));
        }
        let mut children = crate::flow::children(expr);
        children.reverse();
        stack.extend(children);
    }
    found
}

/// Reports headers that would include each other (`B2C-E0422`,
/// w3-modules.md §2.2, §2.4) when no circle of definitions explains it
/// (`explained`, already `B2C-E0421`): on each shared function whose default
/// value makes its header need the next one round the circle, with the rest
/// of the circle as related locations.
fn check_header_cycles(
    program: &ProgramView<'_>,
    layout: &Layout,
    needs: &Needs<'_>,
    explained: &[BTreeSet<usize>],
    diags: &mut Diags,
) {
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
        if explained
            .iter()
            .any(|modules| modules.len() > 1 && modules.is_subset(&members))
        {
            continue;
        }
        for &a in &members {
            for (&b, reasons) in needs.header.get(a).into_iter().flatten() {
                if !members.contains(&b) {
                    continue;
                }
                for reason in reasons {
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
    let used = program.name(reason.used);
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
    // One function to move on each side only when each side's need has a
    // single reason: with two, moving one leaves the other's need.
    let single = |from: usize, to: usize| {
        needs
            .header
            .get(from)
            .and_then(|list| list.get(&to))
            .is_some_and(|reasons| reasons.len() == 1)
    };
    let fix = match back.as_slice() {
        [(_, _, only)] if single(a, b) && single(b, a) => format!(
            "Move {by} into module {}, or {} into module {}.",
            program.module_name(b),
            program.name(only.by.unwrap_or(only.used)),
            program.module_name(a)
        ),
        _ => String::from("Move one of these shared functions into another module."),
    };
    let message = format!(
        "{} would need {}, because the default value of {by} uses {used} of module {}, but {} already needs {}{way_back}. \
         Two headers can't each include the other. {fix}",
        header_name(layout, a),
        header_name(layout, b),
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
            "{} needs {} for the default value of {}",
            header_name(layout, from),
            header_name(layout, to),
            why.by.map(|by| program.name(by)).unwrap_or_default()
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

// --- B2C-E0213: a name that is not shared meets a shared one -----------------

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

/// Reports each function that is not shared but has the name of a shared
/// function (`B2C-E0213`, w3-modules.md §2.1, W3M-D10): of its own module,
/// whose `.cpp` declares both, or of any other module, since the `.cpp`
/// includes that module's header as soon as it uses anything the header
/// declares, and C++ then could not tell the two apart (`::scale` would
/// silently reach the other one). The rule is program-wide, so it never
/// depends on what the module uses; the message names the cause when the
/// `.cpp` does include the header, with the use and each include of the
/// chain as related locations.
fn check_shared_names(program: &ProgramView<'_>, layout: &Layout, needs: &Needs<'_>, diags: &mut Diags) {
    let mut shared: BTreeMap<&str, Vec<(&SymbolId, usize)>> = BTreeMap::new();
    for (m, module) in program.modules.iter().enumerate() {
        for item in module.items.iter().filter(|item| item.shared) {
            if let ItemKind::Function(def) = &item.kind
                && let Some(symbol) = program.symbols.get(&def.symbol)
            {
                shared
                    .entry(symbol.name.as_str())
                    .or_default()
                    .push((&def.symbol, m));
            }
        }
    }
    if shared.is_empty() {
        return;
    }
    for (m, module) in program.modules.iter().enumerate() {
        let mut paths = None;
        for item in module.items.iter().filter(|item| !item.shared) {
            let ItemKind::Function(def) = &item.kind else {
                continue;
            };
            let Some(symbol) = program.symbols.get(&def.symbol) else {
                continue;
            };
            let Some(list) = shared.get(symbol.name.as_str()) else {
                continue;
            };
            let paths = paths.get_or_insert_with(|| reached(layout, needs, m));
            // The module's own shared function first, then one whose header
            // the file includes, then the first one.
            let Some(&(other, owner)) = list
                .iter()
                .find(|(_, owner)| *owner == m)
                .or_else(|| list.iter().find(|(_, owner)| paths.contains_key(owner)))
                .or_else(|| list.first())
            else {
                continue;
            };
            let name = quoted(symbol.name.as_str());
            let location = symbol.origin.location();
            let diagnostic = if owner == m {
                let message = format!(
                    "{name} is also the name of a function this module shares, and C++ could not tell the two \
                     apart. Rename one of them, or share this one too."
                );
                Diagnostic::error(codes::SHARED_NAME_CLASH, DiagSource::Analyser, location, message)
            } else {
                shared_name_clash(program, layout, paths, (&name, location), (other, owner))
            };
            let diagnostic = match program.location(other) {
                Some(at) => diagnostic.with_related(at, format!("the shared {name}")),
                None => diagnostic,
            };
            diags.push(diagnostic);
        }
    }
}

/// `B2C-E0213` for a function named like another module's (`owner`'s)
/// shared function `other`: with the chain of includes that brings its
/// header into this file, if one does.
fn shared_name_clash(
    program: &ProgramView<'_>,
    layout: &Layout,
    paths: &BTreeMap<usize, Link<'_, '_>>,
    (name, location): (&str, Location),
    (_other, owner): (&SymbolId, usize),
) -> Diagnostic {
    let module = program.module_name(owner);
    let header = header_name(layout, owner);
    if !paths.contains_key(&owner) {
        let message = format!(
            "{name} is also shared by module {module}. A function that is not shared can't have the name of a \
             shared function of any module: once this file includes {header}, C++ could not tell the two \
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
                    "{} includes {} for the default value of {}",
                    header_name(layout, *from),
                    header_name(layout, *h),
                    reason.by.map(|by| program.name(by)).unwrap_or_default()
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
    fn only_shared_functions_go_into_a_header_until_package_t() {
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
        assert_eq!(shared_items(&module), [1]);
        let mut diags = Diags::default();
        // Items whose symbols the table lacks are not in the declaration
        // graph's order, so they reach no header.
        let layout = compute_layout(&[module], &SymbolTable::default(), &mut diags);
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
        includes(&mut layout, &Needs::default());
        assert_eq!(
            layout.modules[0].source_includes,
            [ModuleId::new("mod_utils").expect("id")]
        );
        layout.modules[0].header = false;
        includes(&mut layout, &Needs::default());
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
        let layout = compute_layout(&modules, &SymbolTable::default(), &mut diags);
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
