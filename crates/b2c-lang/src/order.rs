//! The declaration graph (docs/plans/m3-contracts/modules.md §4, ADR-0019;
//! w2.md §2.2, §2.7, §2.8; w3-modules.md §2.2): in which order each file's
//! declarations are printed, and the cycles C++ cannot order (`B2C-E0420`,
//! `B2C-E0421`), now over the whole program.
//!
//! Nodes are the program's type definitions (structs and enums), function
//! declarations and globals, each with its module and whether it is
//! shared; `main` is not one. An edge from `a` to `b` says that `b` comes
//! before `a`, and may join two modules:
//!
//! * **complete**: `a` holds `b` by value, so `b`'s definition must be
//!   before `a`'s: a field of type `b`, or `b` inside a container other than
//!   a list (the standard allows an element type that is not complete yet
//!   only in `std::vector`), or `b` anywhere in a field of a struct that is
//!   comparable or printable, whose generated operators compare or print the
//!   field. An enum is always needed complete. A default value (a
//!   parameter's, w2.md §2.8, or a field's) needs complete every user type
//!   its values have, and the declaration of every function it calls.
//! * **forward**: `a` names the struct `b` only inside lists (`std::vector`,
//!   at any depth), or a function names it in its parameters or result: a
//!   forward declaration (`struct b;`) is enough, which code generation
//!   prints when `b`'s definition is not already before `a`. Such an edge is
//!   kept where it can be, so that a definition comes first whenever no
//!   cycle needs the forward declaration, and never closes a cycle.
//! * **initialised**: a global needs every global its starting value reads
//!   when it is set, directly, through the functions it calls (followed
//!   through every call) and through the default member initialisers of
//!   the structs it builds (modules.md §4.1). What running each function
//!   and building each struct reads is summed up once per program over the
//!   strongly connected components of the call graph ([`Reach`]), so the
//!   walk stays linear in calls and uses. A global set before the program
//!   starts from another module's global that C++ does not set before any
//!   code runs is `B2C-E0423` (modules.md §4.4).
//!
//! An edge to another module's definition is met by an include or a forward
//! declaration (`crate::layout`), never by printing the definition again,
//! so each file's order is the order of the graph restricted to its own
//! declarations. When several declarations are ready, those with the
//! visibility of the last one printed come first, starting with those that
//! are not shared (so that they stay in one anonymous namespace), then 06
//! §6.7's rule: constants, types, functions, then variables, each sorted by
//! name, then block ID; functions with one name keep the flat layout's
//! order (their parameters' types). A program with only functions prints
//! exactly as before, and so does every single-module program.
//!
//! A cycle of complete edges through struct fields is `B2C-E0420`, reported
//! at each struct of the cycle; any other cycle is `B2C-E0421`. Each cycle
//! is reported once, also when it joins modules. Every walk is iterative.

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::diag::{DiagSource, Diagnostic, Location, Part};
use b2c_ir::ids::{BlockId, SymbolId};
use b2c_ir::layout::Layout;
use b2c_ir::sast::{Block, Expr, ExprKind, GlobalDef, Item, ItemKind, Module, SymbolKind, SymbolTable};
use b2c_ir::types::{LibType, Type};

use crate::codes;
use crate::consts::Constant;
use crate::messages::{Diags, article, quoted};

/// How a declaration needs another one (modules.md §4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Need {
    /// The other one's definition before it.
    Complete,
    /// A forward declaration of the other one, or its definition, before it.
    Forward,
}

/// What kind of declaration a node is, in the order 06 §6.7 prints
/// declarations that are ready together.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum NodeKind {
    /// A global constant (w2.md §2.7).
    Constant,
    /// A struct.
    Struct,
    /// An enum.
    Enum,
    /// A function's declaration.
    Function,
    /// A global variable, or any other declaration.
    Other,
}

/// One declaration of the graph.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    /// Its kind.
    pub(crate) kind: NodeKind,
    /// The name it is sorted and reported by.
    pub(crate) name: String,
    /// Its place among the declarations of its kind with the same name
    /// (the flat layout's order of overloads), the next key of the order.
    pub(crate) seq: usize,
    /// Its defining block, the last key of the order.
    pub(crate) block: BlockId,
    /// The index of its item in its module.
    pub(crate) item: usize,
    /// The index of its module.
    pub(crate) module: usize,
    /// Whether its module shares it (ADR-0019): it is printed in the
    /// module's header, and the order keeps runs of one visibility.
    pub(crate) shared: bool,
}

/// One need of a node: the node it needs, how, and the part of the
/// declaration that needs it (a struct's field), for messages.
#[derive(Debug, Clone)]
pub(crate) struct Dep {
    /// The node needed.
    pub(crate) on: usize,
    /// How.
    pub(crate) need: Need,
    /// What needs it, by name, with its location: a struct's field, or
    /// the node itself for any other need.
    pub(crate) via: Option<(String, Location)>,
    /// Whether a struct's field needs it (by value, `B2C-E0420`), rather
    /// than a default value or a declaration's own parts.
    pub(crate) field: bool,
}

/// The declarations of one module and what each needs.
#[derive(Debug, Clone, Default)]
pub(crate) struct Graph {
    /// The nodes.
    pub(crate) nodes: Vec<Node>,
    /// What each node needs, by node index.
    pub(crate) deps: Vec<Vec<Dep>>,
}

impl Graph {
    /// Adds a node and returns its index.
    pub(crate) fn add(&mut self, node: Node) -> usize {
        self.nodes.push(node);
        self.deps.push(Vec::new());
        self.nodes.len() - 1
    }

    /// The sort key of a node among those that are ready together.
    fn key(&self, node: usize) -> (u8, &str, usize, &BlockId) {
        let node = &self.nodes[node];
        let rank = match node.kind {
            NodeKind::Constant => 0,
            NodeKind::Struct | NodeKind::Enum => 1,
            NodeKind::Function => 2,
            NodeKind::Other => 3,
        };
        (rank, node.name.as_str(), node.seq, &node.block)
    }

    /// The nodes in printing order: each after what it needs complete, and
    /// after what it names in lists where no cycle prevents it; ties by kind,
    /// name and block. The nodes of a cycle of complete edges (reported by
    /// [`Graph::cycles`]) come in key order where the cycle is met.
    pub(crate) fn order(&self) -> Vec<usize> {
        let count = self.nodes.len();
        // The cycle of complete edges each node is in, if any.
        let mut cycle_of = vec![usize::MAX; count];
        for (index, cycle) in self.cycles().iter().enumerate() {
            for &node in cycle {
                cycle_of[node] = index;
            }
        }
        // A node is ready in tier 0 when every need is met, in tier 1 when
        // every need but its forward ones is, in tier 2 when every need but
        // its forward ones and those inside its own cycle is; any node is
        // ready in tier 3. Each step places the node first in key order of
        // the first tier that has one. Instead of rescanning every node for
        // each step, each node counts its needs not met yet for tiers 0 to 2
        // (one per need, a need of itself not counted), and each tier keeps
        // its ready nodes in key order.
        let in_cycle =
            |node: usize, on: usize| cycle_of[node] != usize::MAX && cycle_of[on] == cycle_of[node];
        let counted = |node: usize, dep: &Dep| {
            [
                true,
                dep.need != Need::Forward,
                dep.need != Need::Forward && !in_cycle(node, dep.on),
            ]
        };
        let mut unmet = vec![[0usize; 3]; count];
        let mut needed_by: Vec<Vec<(usize, [bool; 3])>> = vec![Vec::new(); count];
        for (node, deps) in self.deps.iter().enumerate() {
            for dep in deps {
                if dep.on == node {
                    continue;
                }
                let tiers = counted(node, dep);
                for (tier, &counts) in tiers.iter().enumerate() {
                    if counts {
                        unmet[node][tier] += 1;
                    }
                }
                needed_by[dep.on].push((node, tiers));
            }
        }
        // Each tier keeps its ready nodes by visibility (not shared, then
        // shared): the first tier that has one gives a node of the
        // visibility last printed when it has one, starting with those that
        // are not shared (modules.md §4.2).
        let key = |node: usize| (self.key(node), node);
        let shown = |node: usize| usize::from(self.nodes[node].shared);
        let mut ready: [[BTreeSet<_>; 2]; 4] = Default::default();
        for (node, counts) in unmet.iter().enumerate() {
            for (tier, &left) in counts.iter().enumerate() {
                if left == 0 {
                    ready[tier][shown(node)].insert(key(node));
                }
            }
            ready[3][shown(node)].insert(key(node));
        }
        let mut out = Vec::with_capacity(count);
        let mut last = 0;
        while let Some(&(_, next)) = ready
            .iter()
            .find(|tier| tier.iter().any(|set| !set.is_empty()))
            .and_then(|tier| tier[last].first().or_else(|| tier[1 - last].first()))
        {
            last = shown(next);
            for tier in &mut ready {
                tier[last].remove(&key(next));
            }
            out.push(next);
            for &(node, tiers) in &needed_by[next] {
                let visibility = shown(node);
                for (tier, &counts) in tiers.iter().enumerate() {
                    if counts {
                        unmet[node][tier] -= 1;
                        if unmet[node][tier] == 0 && ready[3][visibility].contains(&key(node)) {
                            ready[tier][visibility].insert(key(node));
                        }
                    }
                }
            }
        }
        out
    }

    /// The cycles of complete edges: each strongly connected component of
    /// those edges with more than one node, or with a node that needs
    /// itself, in node order.
    pub(crate) fn cycles(&self) -> Vec<Vec<usize>> {
        let count = self.nodes.len();
        let successors: Vec<Vec<usize>> = (0..count)
            .map(|node| {
                self.deps[node]
                    .iter()
                    .filter(|dep| dep.need == Need::Complete)
                    .map(|dep| dep.on)
                    .collect()
            })
            .collect();
        let mut components: Vec<Vec<usize>> = components(&successors)
            .into_iter()
            .filter_map(|mut component| {
                component.sort_unstable();
                let looped = component.len() > 1 || successors[component[0]].contains(&component[0]);
                looped.then_some(component)
            })
            .collect();
        components.sort();
        components
    }

    /// The graph of the declarations of module `module` alone, in node
    /// order, with the needs between them: what an include or a forward
    /// declaration meets is left out (w3-modules.md §2.2).
    fn restricted(&self, module: usize) -> Graph {
        let mut graph = Graph::default();
        let mut renumbered: BTreeMap<usize, usize> = BTreeMap::new();
        for (node, found) in self.nodes.iter().enumerate() {
            if found.module == module {
                renumbered.insert(node, graph.add(found.clone()));
            }
        }
        for (&old, &new) in &renumbered {
            for dep in &self.deps[old] {
                if let Some(&on) = renumbered.get(&dep.on) {
                    graph.deps[new].push(Dep { on, ..dep.clone() });
                }
            }
        }
        graph
    }

    /// A shortest path of complete edges inside `cycle` from `start` back to
    /// itself: the deps taken, in order.
    fn path_back(&self, start: usize, cycle: &BTreeSet<usize>) -> Vec<&Dep> {
        // Breadth first; `came` keeps the dep that first reached each node.
        let mut came: BTreeMap<usize, (usize, &Dep)> = BTreeMap::new();
        let mut queue = std::collections::VecDeque::from([start]);
        while let Some(node) = queue.pop_front() {
            for dep in &self.deps[node] {
                if dep.need != Need::Complete || !cycle.contains(&dep.on) {
                    continue;
                }
                if dep.on == start {
                    let mut path = vec![dep];
                    let mut at = node;
                    while at != start {
                        let Some(&(from, step)) = came.get(&at) else {
                            break;
                        };
                        path.push(step);
                        at = from;
                    }
                    path.reverse();
                    return path;
                }
                if let std::collections::btree_map::Entry::Vacant(entry) = came.entry(dep.on) {
                    entry.insert((node, dep));
                    queue.push_back(dep.on);
                }
            }
        }
        Vec::new()
    }

    /// Reports every cycle: `B2C-E0420` at each struct of a cycle of fields,
    /// on the field that leads on round it; `B2C-E0421` at each declaration
    /// of any other cycle. A message shows at most [`SHOWN_STEPS`] steps of
    /// a long cycle.
    pub(crate) fn report_cycles(&self, diags: &mut Diags) {
        self.report_cycles_except(&BTreeSet::new(), diags);
    }

    /// [`Graph::report_cycles`], with nothing reported at the nodes of
    /// `skip` (reported elsewhere already); the paths round each cycle still
    /// go through them.
    pub(crate) fn report_cycles_except(&self, skip: &BTreeSet<usize>, diags: &mut Diags) {
        for cycle in self.cycles() {
            let members: BTreeSet<usize> = cycle.iter().copied().collect();
            // One walk round the cycle serves every node on it; a node off
            // that walk gets its own.
            let start = cycle.first().copied();
            let round = start
                .map(|first| self.path_back(first, &members))
                .unwrap_or_default();
            let structs = cycle
                .iter()
                .all(|node| self.nodes[*node].kind == NodeKind::Struct)
                && round.iter().all(|dep| dep.field);
            let on_round: BTreeMap<usize, usize> =
                round.iter().enumerate().map(|(at, dep)| (dep.on, at)).collect();
            for &node in cycle.iter().filter(|node| !skip.contains(node)) {
                let path: Vec<&Dep> = match on_round.get(&node) {
                    _ if Some(node) == start => round.clone(),
                    Some(&at) => round[at + 1..]
                        .iter()
                        .chain(&round[..=at])
                        .take(SHOWN_STEPS + 1)
                        .copied()
                        .collect(),
                    None => self.path_back(node, &members),
                };
                let Some((_, location)) = path.first().and_then(|first| first.via.as_ref()) else {
                    continue;
                };
                let (code, message) = if structs {
                    (codes::STRUCT_CYCLE, self.struct_cycle(node, &path))
                } else {
                    (codes::DECLARATION_CYCLE, self.declaration_cycle(node, &path))
                };
                diags.error(code, location.clone(), message);
            }
        }
    }

    /// `B2C-E0420`'s message for the struct `node`, round `path`.
    fn struct_cycle(&self, node: usize, path: &[&Dep]) -> String {
        let name = |node: usize| quoted(&self.nodes[node].name);
        let holds = |dep: &Dep| {
            let held = &self.nodes[dep.on].name;
            let field = dep.via.as_ref().map_or_else(String::new, |(field, _)| {
                format!(" in its field {}", quoted(field))
            });
            format!("holds {} {}{field}", article(held), quoted(held))
        };
        let mut chain = name(node);
        for (step, dep) in path.iter().take(SHOWN_STEPS).enumerate() {
            chain.push_str(if step == 0 { " " } else { ", which " });
            chain.push_str(&holds(dep));
        }
        if path.len() > SHOWN_STEPS {
            chain.push_str(", and so on back to ");
            chain.push_str(&name(node));
        }
        let fix = if path.len() == 1 {
            "Make the field a list (a list keeps its items elsewhere), or remove it."
        } else {
            "Make one of these fields a list (a list keeps its items elsewhere), or remove it."
        };
        format!(
            "{chain}, so each {} would have to hold a whole copy of itself, which C++ does not allow. {fix}",
            name(node)
        )
    }

    /// `B2C-E0421`'s message for `node`, round `path`.
    fn declaration_cycle(&self, node: usize, path: &[&Dep]) -> String {
        if let [only] = path
            && only.on == node
        {
            // A global set from its own value, directly or through a
            // function it calls.
            return format!(
                "{} needs its own value to be set first, which C++ cannot do. Change it so that it no longer \
                 needs itself.",
                quoted(&self.nodes[node].name)
            );
        }
        let mut names = vec![quoted(&self.nodes[node].name)];
        names.extend(
            path.iter()
                .take(SHOWN_STEPS)
                .map(|dep| quoted(&self.nodes[dep.on].name)),
        );
        if path.len() > SHOWN_STEPS {
            names.push(String::from("… back to ") + &quoted(&self.nodes[node].name));
        }
        format!(
            "These definitions each need the next one first, in a circle that C++ cannot put in order: {}. \
             Change one of them so that it no longer needs the next.",
            names.join(" needs ")
        )
    }
}

/// How many steps of a cycle a message shows.
const SHOWN_STEPS: usize = 4;

/// The strongly connected components of a graph given by each node's
/// successors, each one once a component it reaches is done (the reverse
/// of a topological order), by Tarjan's algorithm, iteratively.
fn components(successors: &[Vec<usize>]) -> Vec<Vec<usize>> {
    let count = successors.len();
    let mut index = vec![usize::MAX; count];
    let mut low = vec![0; count];
    let mut on_stack = vec![false; count];
    let mut stack = Vec::new();
    let mut next_index = 0;
    let mut components = Vec::new();
    for root in 0..count {
        if index[root] != usize::MAX {
            continue;
        }
        // Each frame: the node and how many of its successors are done.
        let mut frames: Vec<(usize, usize)> = vec![(root, 0)];
        index[root] = next_index;
        low[root] = next_index;
        next_index += 1;
        stack.push(root);
        on_stack[root] = true;
        while let Some(&mut (node, ref mut done)) = frames.last_mut() {
            if let Some(&next) = successors[node].get(*done) {
                *done += 1;
                if index[next] == usize::MAX {
                    index[next] = next_index;
                    low[next] = next_index;
                    next_index += 1;
                    stack.push(next);
                    on_stack[next] = true;
                    frames.push((next, 0));
                } else if on_stack[next] {
                    low[node] = low[node].min(index[next]);
                }
                continue;
            }
            frames.pop();
            if let Some(&(parent, _)) = frames.last() {
                low[parent] = low[parent].min(low[node]);
            }
            if low[node] == index[node] {
                let mut component = Vec::new();
                while let Some(member) = stack.pop() {
                    on_stack[member] = false;
                    component.push(member);
                    if member == node {
                        break;
                    }
                }
                components.push(component);
            }
        }
    }
    components
}

/// Where a type stands inside a field's type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Depth {
    /// The field's type itself.
    Top,
    /// Inside lists only (`std::vector`, at any depth).
    Lists,
    /// Inside some other container.
    Other,
}

/// The user types that a field's type names, each with how the field needs
/// it: complete, or forward when it is a struct named only inside lists.
fn named_types(ty: &Type, symbols: &SymbolTable, out: &mut Vec<(SymbolId, Need)>) {
    let mut stack = vec![(ty, Depth::Top)];
    while let Some((ty, depth)) = stack.pop() {
        match ty {
            Type::Named(id) => {
                let is_struct = matches!(
                    symbols.get(id).map(|symbol| &symbol.kind),
                    Some(SymbolKind::Struct { .. })
                );
                let need = if depth == Depth::Lists && is_struct {
                    Need::Forward
                } else {
                    Need::Complete
                };
                out.push((id.clone(), need));
            }
            Type::Lib(app) => {
                let inner = if app.ty() == LibType::Vector && depth != Depth::Other {
                    Depth::Lists
                } else {
                    Depth::Other
                };
                stack.extend(app.args().iter().map(|arg| (arg, inner)));
            }
            _ => {}
        }
    }
}

/// What the values of a declaration use: the functions they call, the
/// symbols they name and the user types their values have.
#[derive(Debug, Default)]
struct Uses<'p> {
    calls: BTreeSet<&'p SymbolId>,
    symbols: BTreeSet<&'p SymbolId>,
    types: BTreeSet<&'p SymbolId>,
}

impl<'p> Uses<'p> {
    /// Adds what `exprs` and everything inside them use.
    fn exprs(&mut self, exprs: impl IntoIterator<Item = &'p Expr>) {
        let mut stack: Vec<&'p Expr> = exprs.into_iter().collect();
        while let Some(expr) = stack.pop() {
            match &expr.kind {
                ExprKind::Call { function, .. } => {
                    self.calls.insert(function);
                }
                ExprKind::Var(sym) => {
                    self.symbols.insert(sym);
                }
                _ => {}
            }
            let mut named = Vec::new();
            user_types_in(&expr.ty, &mut named);
            self.types.extend(named);
            stack.extend(crate::flow::children(expr));
        }
    }
}

/// The user types a type names, at any depth.
fn user_types_in<'t>(ty: &'t Type, out: &mut Vec<&'t SymbolId>) {
    let mut stack = vec![ty];
    while let Some(ty) = stack.pop() {
        match ty {
            Type::Named(id) => out.push(id),
            Type::Lib(app) => stack.extend(app.args()),
            _ => {}
        }
    }
}

/// The declarations of the program as one graph: one node per struct, enum,
/// function and global item whose symbol is in the table (the first item for
/// a symbol two items name), with the needs of each struct's fields on the
/// other types, those of the functions' signatures, those of the default
/// values of functions and fields, and those of the globals (the
/// *initialised* edges among them). Edges join modules where the program
/// does; `B2C-E0423` is reported here for an *initialised* edge to another
/// module's global that C++ does not set before any code runs.
pub(crate) fn program_graph(
    modules: &[Module],
    symbols: &SymbolTable,
    constants: &BTreeMap<SymbolId, Constant>,
    diags: &mut Diags,
) -> Graph {
    let mut graph = Graph::default();
    let mut by_symbol: BTreeMap<&SymbolId, usize> = BTreeMap::new();
    let mut by_item: BTreeMap<(usize, usize), usize> = BTreeMap::new();
    for (m, module) in modules.iter().enumerate() {
        // Functions keep the flat layout's order among those with one name.
        let flat = function_seq(module, symbols);
        for (index, item) in module.items.iter().enumerate() {
            let (symbol, kind) = match &item.kind {
                ItemKind::Struct(def) => (&def.symbol, NodeKind::Struct),
                ItemKind::Enum(def) => (&def.symbol, NodeKind::Enum),
                ItemKind::Function(def) => (&def.symbol, NodeKind::Function),
                ItemKind::Global(def) => {
                    let constant = matches!(
                        symbols.get(&def.symbol).map(|symbol| &symbol.kind),
                        Some(SymbolKind::Global { is_const: true })
                    );
                    let kind = if constant {
                        NodeKind::Constant
                    } else {
                        NodeKind::Other
                    };
                    (&def.symbol, kind)
                }
                _ => continue,
            };
            let Some(found) = symbols.get(symbol) else {
                continue;
            };
            let node = graph.add(Node {
                kind,
                name: found.name.as_str().to_owned(),
                seq: flat.get(&index).copied().unwrap_or(0),
                block: item.origin.block.clone(),
                item: index,
                module: m,
                shared: item.shared,
            });
            by_symbol.entry(symbol).or_insert(node);
            by_item.insert((m, index), node);
        }
    }
    for (symbol, &node) in &by_symbol {
        match symbols.get(symbol).map(|symbol| &symbol.kind) {
            Some(SymbolKind::Struct {
                fields,
                comparable,
                printable,
            }) => field_needs(
                &mut graph,
                &by_symbol,
                symbols,
                node,
                fields,
                *comparable || *printable,
            ),
            Some(SymbolKind::Function { params, .. }) => {
                let location = graph.nodes[node].location(modules);
                signature_needs(&mut graph, &by_symbol, symbols, node, symbol, params, &location);
            }
            _ => {}
        }
    }
    // Default values, a parameter's and a field's: the declaration of each
    // function they call and of each global they name, every user type
    // their values have.
    let reach = Reach::new(modules, symbols);
    for (m, module) in modules.iter().enumerate() {
        for (index, item) in module.items.iter().enumerate() {
            let Some(&node) = by_item.get(&(m, index)) else {
                continue;
            };
            let mut uses = Uses::default();
            match &item.kind {
                ItemKind::Function(def) => uses.exprs(def.params.iter().filter_map(|p| p.default.as_ref())),
                ItemKind::Struct(def) => uses.exprs(def.defaults.values()),
                ItemKind::Global(def) => {
                    let place = GlobalPlace {
                        modules,
                        symbols,
                        constants,
                        node,
                        location: item.origin.location(),
                    };
                    global_needs(&mut graph, &by_symbol, &reach, &place, def, diags);
                    continue;
                }
                _ => continue,
            }
            let location = item.origin.location();
            for id in uses.calls.iter().chain(&uses.symbols).chain(&uses.types) {
                if let Some(&on) = by_symbol.get(id)
                    && on != node
                {
                    graph.need(node, on, Need::Complete, location.clone());
                }
            }
        }
    }
    graph
}

/// The needs of a function's declaration on the types of its parameters and
/// result: a forward declaration of a struct is enough, an enum is needed
/// complete.
fn signature_needs(
    graph: &mut Graph,
    by_symbol: &BTreeMap<&SymbolId, usize>,
    symbols: &SymbolTable,
    node: usize,
    function: &SymbolId,
    params: &[SymbolId],
    location: &Location,
) {
    let ret = symbols.get(function).map(|s| &s.ty);
    let mut named = Vec::new();
    for ty in params
        .iter()
        .filter_map(|p| symbols.get(p))
        .map(|p| &p.ty)
        .chain(ret)
    {
        user_types_in(ty, &mut named);
    }
    for id in named {
        let Some(&on) = by_symbol.get(id) else {
            continue;
        };
        let need = if graph.nodes[on].kind == NodeKind::Struct {
            Need::Forward
        } else {
            Need::Complete
        };
        graph.need(node, on, need, location.clone());
    }
}

/// What running each function, and building each struct, of the program
/// reads (modules.md §4.1, *initialised*): a function's body and the
/// default values of its parameters, a struct's default member
/// initialisers, and, through every call and every struct built (directly
/// or as a field), what those read in turn. The sums are worked out once,
/// over the strongly connected components of the graph of calls and
/// structs built, each component after those it reaches, so a program's
/// globals cost its calls and uses once, not once per global.
struct Reach<'p> {
    /// Each function's and struct's place in `reads`.
    place: BTreeMap<&'p SymbolId, usize>,
    /// The globals that running each function, or building each struct,
    /// reads, transitively.
    reads: Vec<BTreeSet<&'p SymbolId>>,
    /// How many steps the sums took: one per need followed and one per
    /// global added, which stays linear in calls and uses while the sums
    /// are small (a test counts them).
    #[cfg_attr(not(test), allow(dead_code, reason = "counted by a test"))]
    steps: usize,
}

impl<'p> Reach<'p> {
    fn new(modules: &'p [Module], symbols: &'p SymbolTable) -> Self {
        let mut place: BTreeMap<&SymbolId, usize> = BTreeMap::new();
        let mut uses: Vec<Uses<'p>> = Vec::new();
        let mut built: Vec<Vec<&'p SymbolId>> = Vec::new();
        for item in modules.iter().flat_map(|module| &module.items) {
            let (symbol, found) = match &item.kind {
                ItemKind::Function(def) => {
                    let mut found = Uses::default();
                    found.exprs(def.params.iter().filter_map(|p| p.default.as_ref()));
                    found.exprs(body_exprs(&def.body));
                    (&def.symbol, found)
                }
                ItemKind::Struct(def) => {
                    let mut found = Uses::default();
                    found.exprs(def.defaults.values());
                    (&def.symbol, found)
                }
                _ => continue,
            };
            if place.contains_key(symbol) {
                continue;
            }
            // A struct built also builds the structs its fields hold.
            let mut fields = Vec::new();
            if let Some(SymbolKind::Struct { fields: list, .. }) = symbols.get(symbol).map(|s| &s.kind) {
                for field in list.iter().filter_map(|field| symbols.get(field)) {
                    user_types_in(&field.ty, &mut fields);
                }
            }
            place.insert(symbol, uses.len());
            uses.push(found);
            built.push(fields);
        }
        let mut steps = 0;
        let successors: Vec<Vec<usize>> = uses
            .iter()
            .zip(&built)
            .map(|(found, fields)| {
                let mut next: Vec<usize> = found
                    .calls
                    .iter()
                    .chain(&found.types)
                    .chain(fields)
                    .filter_map(|id| place.get(*id).copied())
                    .collect();
                next.sort_unstable();
                next.dedup();
                next
            })
            .collect();
        let mut reads: Vec<BTreeSet<&SymbolId>> = vec![BTreeSet::new(); uses.len()];
        // Each component comes after every component it reaches.
        for component in components(&successors) {
            let mut sum: BTreeSet<&SymbolId> = BTreeSet::new();
            for &member in &component {
                for id in uses[member]
                    .symbols
                    .iter()
                    .copied()
                    .filter(|id| is_global(id, symbols))
                {
                    steps += 1;
                    sum.insert(id);
                }
                for &next in &successors[member] {
                    steps += 1;
                    if !component.contains(&next) {
                        for id in &reads[next] {
                            steps += 1;
                            sum.insert(id);
                        }
                    }
                }
            }
            for &member in &component {
                reads[member].clone_from(&sum);
            }
        }
        Reach { place, reads, steps }
    }

    /// The globals that setting a value from `start` reads: directly, and
    /// what running each function it calls and building each struct it
    /// builds reads. `built` names more types built with the value (a
    /// global's own type, whose default member initialisers run when it is
    /// set). The walk is conservative: a function that reads a global on any
    /// path counts.
    fn reads(
        &self,
        start: &Uses<'p>,
        built: &[&'p SymbolId],
        symbols: &SymbolTable,
    ) -> BTreeSet<&'p SymbolId> {
        let mut read: BTreeSet<&SymbolId> = start
            .symbols
            .iter()
            .copied()
            .filter(|id| is_global(id, symbols))
            .collect();
        for id in start.calls.iter().chain(&start.types).chain(built) {
            if let Some(&at) = self.place.get(*id) {
                read.extend(self.reads[at].iter().copied());
            }
        }
        read
    }
}

/// Whether a symbol is a global variable or constant.
fn is_global(id: &SymbolId, symbols: &SymbolTable) -> bool {
    matches!(
        symbols.get(id).map(|symbol| &symbol.kind),
        Some(SymbolKind::Global { .. })
    )
}

/// A global's place in the program, for [`global_needs`].
struct GlobalPlace<'p> {
    modules: &'p [Module],
    symbols: &'p SymbolTable,
    constants: &'p BTreeMap<SymbolId, Constant>,
    /// Its node.
    node: usize,
    /// Its block.
    location: Location,
}

/// The needs of a global (modules.md §4.1): its type's definitions
/// (complete), the declaration of each function its starting value calls
/// and of each user type its values have, and the definition of every
/// global the value reads when it is set (*initialised*), itself included:
/// a global set from its own value is a circle (`B2C-E0421`). A global of
/// another module read so is `B2C-E0423`, unless C++ sets it before any
/// code runs ([`set_before_any_code`]).
fn global_needs(
    graph: &mut Graph,
    by_symbol: &BTreeMap<&SymbolId, usize>,
    reach: &Reach<'_>,
    place: &GlobalPlace<'_>,
    def: &GlobalDef,
    diags: &mut Diags,
) {
    let symbols = place.symbols;
    let node = place.node;
    let mut uses = Uses::default();
    uses.exprs(def.init.iter());
    let mut named = Vec::new();
    if let Some(symbol) = symbols.get(&def.symbol) {
        user_types_in(&symbol.ty, &mut named);
    }
    let globals = reach.reads(&uses, &named, symbols);
    let mut needed: BTreeSet<usize> = BTreeSet::new();
    for id in named
        .iter()
        .copied()
        .chain(uses.calls.iter().copied())
        .chain(uses.types.iter().copied())
        .chain(globals.iter().copied())
    {
        if let Some(&on) = by_symbol.get(id)
            && needed.insert(on)
        {
            graph.need(node, on, Need::Complete, place.location.clone());
        }
    }
    let module = graph.nodes[node].module;
    for id in globals {
        let Some(&on) = by_symbol.get(id) else {
            continue;
        };
        if graph.nodes[on].module != module && !set_before_any_code(id, symbols, place.constants) {
            report_initialisation(graph, place, (node, on), id, diags);
        }
    }
}

/// Whether C++ sets a global before any code runs, so that another
/// module's global may be set from it (modules.md §4.4): a constant of a
/// type L10 evaluates (`bool`, `char`, `int`, `double`) whose value L10
/// proves a constant expression, printed `constexpr` or not.
fn set_before_any_code(
    id: &SymbolId,
    symbols: &SymbolTable,
    constants: &BTreeMap<SymbolId, Constant>,
) -> bool {
    symbols.get(id).is_some_and(|symbol| {
        matches!(symbol.kind, SymbolKind::Global { is_const: true })
            && matches!(symbol.ty, Type::Bool | Type::Char | Type::Int | Type::Double)
    }) && constants
        .get(id)
        .is_some_and(|constant| constant.constant_expression)
}

/// `B2C-E0423` on the global of node `node`, set from the global `read`
/// (node `on`) of another module before the program starts.
fn report_initialisation(
    graph: &Graph,
    place: &GlobalPlace<'_>,
    (node, on): (usize, usize),
    read: &SymbolId,
    diags: &mut Diags,
) {
    let name = quoted(&graph.nodes[node].name);
    let other = quoted(&graph.nodes[on].name);
    let module = place
        .modules
        .get(graph.nodes[on].module)
        .map(|module| quoted(&module.name))
        .unwrap_or_default();
    let constant = matches!(
        place.symbols.get(read).map(|symbol| &symbol.kind),
        Some(SymbolKind::Global { is_const: true })
    );
    let fix = if constant {
        format!("Set {name} at the start of 'when program starts' instead.")
    } else {
        format!(
            "Set {name} at the start of 'when program starts', or make {other} a constant with a fixed value."
        )
    };
    let message = format!(
        "{name} is set from {other} of module {module} before the program starts, and C++ does not say which \
         of the two is set first. {fix}"
    );
    let location = Location {
        part: Part::Input {
            name: String::from("VALUE"),
        },
        ..place.location.clone()
    };
    let mut diagnostic = Diagnostic::error(
        codes::INIT_ACROSS_MODULES,
        DiagSource::Analyser,
        location,
        message,
    );
    if let Some(symbol) = place.symbols.get(read) {
        diagnostic = diagnostic.with_related(
            symbol.origin.location(),
            format!("{other} is defined here, in module {module}"),
        );
    }
    diags.push(diagnostic);
}

/// Every expression of a statement list, at any depth.
fn body_exprs(body: &Block) -> Vec<&Expr> {
    let mut exprs = Vec::new();
    let mut lists = vec![body];
    while let Some(list) = lists.pop() {
        for stmt in &list.stmts {
            let (values, nested) = crate::nesting::parts(stmt);
            exprs.extend(values);
            lists.extend(nested);
        }
    }
    exprs
}

impl Graph {
    /// Adds a need of `node` on `on`, from the declaration at `location`
    /// (not from one of its fields).
    fn need(&mut self, node: usize, on: usize, need: Need, location: Location) {
        let name = self.nodes[on].name.clone();
        self.deps[node].push(Dep {
            on,
            need,
            via: Some((name, location)),
            field: false,
        });
    }
}

impl Node {
    /// Where the node's declaration is, for messages.
    fn location(&self, modules: &[Module]) -> Location {
        modules
            .get(self.module)
            .and_then(|module| module.items.get(self.item))
            .map_or_else(Location::project, |item| item.origin.location())
    }
}

/// Each function item's place in the flat layout's order of the module.
fn function_seq(module: &Module, symbols: &SymbolTable) -> BTreeMap<usize, usize> {
    let flat = b2c_ir::layout::Layout::flat(std::slice::from_ref(module), symbols);
    flat.modules
        .first()
        .map(|files| {
            files
                .source_order
                .iter()
                .enumerate()
                .map(|(seq, item)| (*item, seq))
                .collect()
        })
        .unwrap_or_default()
}

/// The needs of a struct's fields on the other types of the module.
fn field_needs(
    graph: &mut Graph,
    by_symbol: &BTreeMap<&SymbolId, usize>,
    symbols: &SymbolTable,
    node: usize,
    fields: &[SymbolId],
    operators: bool,
) {
    for field in fields {
        let Some(field) = symbols.get(field) else {
            continue;
        };
        let mut named = Vec::new();
        named_types(&field.ty, symbols, &mut named);
        for (id, need) in named {
            let Some(&on) = by_symbol.get(&id) else {
                continue;
            };
            // The generated operators of a comparable or printable struct
            // compare or print the field, which needs the type complete.
            let need = if operators { Need::Complete } else { need };
            graph.deps[node].push(Dep {
                on,
                need,
                via: Some((field.name.as_str().to_owned(), field.origin.location())),
                field: true,
            });
        }
    }
}

/// Puts each module's declarations in the program graph's order (its
/// types, functions and globals), restricted to the module (w3-modules.md
/// §2.2), and reports the cycles, each once, and `B2C-E0423`. Returns the
/// symbols of each cycle that joins modules: a header's need between two of
/// them is explained by it (`B2C-E0422` is not reported for it).
pub(crate) fn order_declarations<'p>(
    modules: &'p [Module],
    symbols: &'p SymbolTable,
    constants: &BTreeMap<SymbolId, Constant>,
    layout: &mut Layout,
    diags: &mut Diags,
) -> Vec<BTreeSet<&'p SymbolId>> {
    let graph = program_graph(modules, symbols, constants, diags);
    graph.report_cycles(diags);
    for (m, files) in layout.modules.iter_mut().enumerate() {
        let own = graph.restricted(m);
        files.source_order = own.order().into_iter().map(|node| own.nodes[node].item).collect();
    }
    graph
        .cycles()
        .into_iter()
        .filter(|cycle| {
            cycle
                .iter()
                .any(|&node| graph.nodes[node].module != graph.nodes[cycle[0]].module)
        })
        .map(|cycle| {
            cycle
                .iter()
                .filter_map(|&node| {
                    let found = &graph.nodes[node];
                    item_symbol(modules.get(found.module)?.items.get(found.item)?)
                })
                .collect()
        })
        .collect()
}

/// The symbol an item defines (a function, struct, enum or global).
pub(crate) fn item_symbol(item: &Item) -> Option<&SymbolId> {
    match &item.kind {
        ItemKind::Function(def) => Some(&def.symbol),
        ItemKind::Struct(def) => Some(&def.symbol),
        ItemKind::Enum(def) => Some(&def.symbol),
        ItemKind::Global(def) => Some(&def.symbol),
        ItemKind::Main(_) | ItemKind::UseHeader { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use b2c_ir::diag::Part;
    use b2c_ir::ids::ModuleId;

    use super::*;

    fn node(kind: NodeKind, name: &str) -> Node {
        Node {
            kind,
            name: name.to_owned(),
            seq: 0,
            block: BlockId::new(&format!("b_{name}")).expect("id"),
            item: 0,
            module: 0,
            shared: false,
        }
    }

    fn location(field: &str) -> Location {
        Location {
            module: Some(ModuleId::new("m").expect("id")),
            block: Some(BlockId::new("b").expect("id")),
            part: Part::Field {
                name: field.to_owned(),
            },
        }
    }

    fn need(graph: &mut Graph, from: usize, on: usize, need: Need, field: &str) {
        graph.deps[from].push(Dep {
            on,
            need,
            via: Some((field.to_owned(), location(field))),
            field: true,
        });
    }

    fn names(graph: &Graph, order: &[usize]) -> Vec<String> {
        order.iter().map(|node| graph.nodes[*node].name.clone()).collect()
    }

    #[test]
    fn a_type_comes_after_what_it_holds_and_ties_go_by_name() {
        let mut graph = Graph::default();
        let student = graph.add(node(NodeKind::Struct, "Student"));
        let address = graph.add(node(NodeKind::Struct, "Address"));
        let colour = graph.add(node(NodeKind::Enum, "Colour"));
        let zone = graph.add(node(NodeKind::Struct, "Zone"));
        need(&mut graph, student, zone, Need::Complete, "zone");
        need(&mut graph, zone, colour, Need::Complete, "colour");
        need(&mut graph, address, student, Need::Forward, "people");
        assert_eq!(
            names(&graph, &graph.order()),
            ["Colour", "Zone", "Student", "Address"]
        );
        assert!(graph.cycles().is_empty());
    }

    #[test]
    fn lists_of_each_other_need_a_forward_declaration_only() {
        let mut graph = Graph::default();
        let a = graph.add(node(NodeKind::Struct, "A"));
        let b = graph.add(node(NodeKind::Struct, "B"));
        let tree = graph.add(node(NodeKind::Struct, "Tree"));
        need(&mut graph, a, b, Need::Forward, "bs");
        need(&mut graph, b, a, Need::Forward, "as");
        need(&mut graph, tree, tree, Need::Forward, "children");
        // No cycle of complete edges. `Tree` needs nothing but itself, so it
        // comes first; `A` and `B` then need a forward declaration of one of
        // them, and go by name.
        assert!(graph.cycles().is_empty());
        assert_eq!(names(&graph, &graph.order()), ["Tree", "A", "B"]);
        let mut diags = Diags::default();
        graph.report_cycles(&mut diags);
        assert!(diags.list.is_empty());
    }

    #[test]
    fn structs_that_hold_each_other_are_reported_at_each() {
        let mut graph = Graph::default();
        let player = graph.add(node(NodeKind::Struct, "Player"));
        let team = graph.add(node(NodeKind::Struct, "Team"));
        let node_ = graph.add(node(NodeKind::Struct, "Node"));
        let other = graph.add(node(NodeKind::Struct, "Other"));
        need(&mut graph, player, team, Need::Complete, "team");
        need(&mut graph, team, player, Need::Complete, "captain");
        need(&mut graph, node_, node_, Need::Complete, "next");
        need(&mut graph, other, player, Need::Complete, "player");
        assert_eq!(graph.cycles(), [vec![player, team], vec![node_]]);
        let mut diags = Diags::default();
        graph.report_cycles(&mut diags);
        let found: Vec<(&str, &str, &Part)> = diags
            .list
            .iter()
            .map(|d| (d.code.0.as_str(), d.message.as_str(), &d.primary.part))
            .collect();
        assert_eq!(
            found,
            [
                (
                    codes::STRUCT_CYCLE,
                    "`Player` holds a `Team` in its field `team`, which holds a `Player` in its field `captain`, \
                     so each `Player` would have to hold a whole copy of itself, which C++ does not allow. Make \
                     one of these fields a list (a list keeps its items elsewhere), or remove it.",
                    &location("team").part,
                ),
                (
                    codes::STRUCT_CYCLE,
                    "`Team` holds a `Player` in its field `captain`, which holds a `Team` in its field `team`, \
                     so each `Team` would have to hold a whole copy of itself, which C++ does not allow. Make \
                     one of these fields a list (a list keeps its items elsewhere), or remove it.",
                    &location("captain").part,
                ),
                (
                    codes::STRUCT_CYCLE,
                    "`Node` holds a `Node` in its field `next`, so each `Node` would have to hold a whole copy \
                     of itself, which C++ does not allow. Make the field a list (a list keeps its items \
                     elsewhere), or remove it.",
                    &location("next").part,
                ),
            ]
        );
        // Every node is still printed: a cycle is entered at its first name,
        // after which `Other` and `Team` are ready.
        assert_eq!(names(&graph, &graph.order()), ["Node", "Player", "Other", "Team"]);
    }

    #[test]
    fn any_other_cycle_is_a_declaration_cycle() {
        let mut graph = Graph::default();
        let a = graph.add(node(NodeKind::Other, "a"));
        let b = graph.add(node(NodeKind::Other, "b"));
        let c = graph.add(node(NodeKind::Other, "c"));
        need(&mut graph, a, b, Need::Complete, "a");
        need(&mut graph, b, c, Need::Complete, "b");
        need(&mut graph, c, a, Need::Complete, "c");
        let mut diags = Diags::default();
        graph.report_cycles(&mut diags);
        let found: Vec<(&str, &str)> = diags
            .list
            .iter()
            .map(|d| (d.code.0.as_str(), d.message.as_str()))
            .collect();
        assert_eq!(found.len(), 3);
        assert_eq!(
            found[0],
            (
                codes::DECLARATION_CYCLE,
                "These definitions each need the next one first, in a circle that C++ cannot put in order: `a` \
                 needs `b` needs `c` needs `a`. Change one of them so that it no longer needs the next."
            )
        );
    }

    #[test]
    fn long_chains_are_walked_without_recursion() {
        let mut graph = Graph::default();
        let count = 20_000;
        for i in 0..count {
            graph.add(node(NodeKind::Struct, &format!("S{i:05}")));
        }
        for i in 1..count {
            need(&mut graph, i - 1, i, Need::Complete, "next");
        }
        need(&mut graph, count - 1, 0, Need::Complete, "first");
        assert_eq!(graph.cycles().len(), 1);
        assert_eq!(graph.cycles()[0].len(), count);
        let mut diags = Diags::default();
        graph.report_cycles(&mut diags);
        assert_eq!(diags.list.len(), count);
    }

    /// The order as it was first written: each step rescans every node
    /// for the first one in key order of the first tier that has one.
    fn rescanning_order(graph: &Graph) -> Vec<usize> {
        let count = graph.nodes.len();
        let mut cycle_of = vec![usize::MAX; count];
        for (index, cycle) in graph.cycles().iter().enumerate() {
            for &node in cycle {
                cycle_of[node] = index;
            }
        }
        let mut placed = vec![false; count];
        let mut out = Vec::new();
        let ready = |placed: &[bool], node: usize, soft: bool, cycle: bool| {
            graph.deps[node].iter().all(|dep| {
                dep.on == node
                    || placed[dep.on]
                    || (!soft && dep.need == Need::Forward)
                    || (cycle && cycle_of[node] != usize::MAX && cycle_of[dep.on] == cycle_of[node])
            })
        };
        // The visibility last printed first, then the other one.
        let mut last = false;
        let first_of = |placed: &[bool], shared: bool, tier: Option<(bool, bool)>| {
            (0..count)
                .filter(|node| {
                    !placed[*node]
                        && graph.nodes[*node].shared == shared
                        && tier.is_none_or(|(soft, cycle)| ready(placed, *node, soft, cycle))
                })
                .min_by(|a, b| graph.key(*a).cmp(&graph.key(*b)))
        };
        while out.len() < count {
            let first = |tier: Option<(bool, bool)>| {
                first_of(&placed, last, tier).or_else(|| first_of(&placed, !last, tier))
            };
            let next = first(Some((true, false)))
                .or_else(|| first(Some((false, false))))
                .or_else(|| first(Some((false, true))))
                .or_else(|| first(None))
                .expect("a node left");
            last = graph.nodes[next].shared;
            placed[next] = true;
            out.push(next);
        }
        out
    }

    proptest::proptest! {
        /// Counting the needs not met gives the order rescanning gives, on
        /// graphs with forward needs, cycles, needs of a node itself, equal
        /// names and nodes of both visibilities.
        #[test]
        fn counting_needs_orders_as_rescanning_does(
            kinds in proptest::collection::vec((0u8..4, proptest::bool::ANY), 1..24),
            edges in proptest::collection::vec((0usize..24, 0usize..24, proptest::bool::ANY), 0..60),
        ) {
            let mut graph = Graph::default();
            for (index, (kind, shared)) in kinds.iter().enumerate() {
                let kind = match kind {
                    0 => NodeKind::Constant,
                    1 => NodeKind::Struct,
                    2 => NodeKind::Function,
                    _ => NodeKind::Other,
                };
                // Few names, so that keys tie on the name and the block.
                let mut added = node(kind, &format!("n{}", index % 5));
                added.block = BlockId::new(&format!("b{}", index % 7)).expect("id");
                added.shared = *shared;
                graph.add(added);
            }
            let count = kinds.len();
            for (from, on, forward) in edges {
                let need = if forward { Need::Forward } else { Need::Complete };
                need_of(&mut graph, from % count, on % count, need);
            }
            proptest::prop_assert_eq!(graph.order(), rescanning_order(&graph));
        }
    }

    fn need_of(graph: &mut Graph, from: usize, on: usize, need: Need) {
        graph.deps[from].push(Dep {
            on,
            need,
            via: None,
            field: false,
        });
    }

    /// The summary of what running each function reads (modules.md §4.1,
    /// w3-modules.md §5.2 T): 256 modules each share a function that calls
    /// the previous module's, and 4,000 globals are set from the last one,
    /// which reaches through every module. Walking the calls from each
    /// global would take 4,000 × 256 steps; the summary takes one step per
    /// call and use, and each global then looks up its function's sum once.
    #[test]
    fn the_summary_counts_each_call_and_use_once() {
        use serde_json::json;
        let modules = 256_usize;
        let globals_per_module = 4_000 / modules + 1;
        let call = |sym: String| json!({"expr": [{"ref": sym}, {"op": "("}, {"op": ")"}]});
        let blocks: Vec<serde_json::Value> = (0..modules)
            .map(|m| {
                let value = if m == 0 {
                    json!({"expr": [{"num": "1"}]})
                } else {
                    json!({"expr": [{"ref": format!("s_f{}", m - 1)}, {"op": "("}, {"op": ")"}, {"op": "+"},
                                    {"num": "1"}]})
                };
                let mut blocks = vec![json!({
                    "id": format!("fn_{m}"), "type": "func.define", "v": 1,
                    "fields": {"NAME": {"sym": format!("s_f{m}"), "name": format!("f{m}")}, "RETURNS": "int",
                               "SHARED": true},
                    "extra": {"params": []},
                    "statements": {"BODY": [{"id": format!("ret_{m}"), "type": "func.return", "v": 1,
                                             "inputs": {"VALUE": value}}]}
                })];
                for g in 0..globals_per_module {
                    blocks.push(json!({
                        "id": format!("gl_{m}_{g}"), "type": "var.global", "v": 1,
                        "fields": {"KIND": "variable", "TYPE": "int",
                                   "NAME": {"sym": format!("s_g{m}_{g}"), "name": format!("g{m}_{g}")}},
                        "inputs": {"VALUE": call(format!("s_f{}", modules - 1))}
                    }));
                }
                if m == 0 {
                    blocks.push(
                        json!({"id": "main", "type": "program.main", "v": 1, "statements": {"BODY": []}}),
                    );
                }
                json!({"id": format!("mod_{m}"), "name": format!("m{m}"), "workspace": {"blocks": blocks}})
            })
            .collect();
        let document: b2c_model::Document = serde_json::from_value(json!({
            "format": "blocks2cpp/project",
            "formatVersion": 1,
            "generator": {"app": "0.1.0", "catalog": "1.0.0"},
            "project": {"id": "prj_test", "name": "Test", "language": {"standard": "c++20"}},
            "modules": blocks
        }))
        .expect("document");
        let analysis = crate::analyze_with(&document, b2c_catalog::core_catalog().sigs());
        assert!(
            analysis
                .diagnostics
                .iter()
                .all(|d| d.severity != b2c_ir::Severity::Error),
            "{:?}",
            analysis.diagnostics.first()
        );
        let program = &analysis.program;
        let globals = program
            .modules
            .iter()
            .flat_map(|module| &module.items)
            .filter(|item| matches!(item.kind, ItemKind::Global(_)))
            .count();
        assert!(globals >= 4_000, "{globals}");
        // The calls and uses of the functions' bodies.
        let mut uses = Uses::default();
        for item in program.modules.iter().flat_map(|module| &module.items) {
            if let ItemKind::Function(def) = &item.kind {
                uses.exprs(body_exprs(&def.body));
            }
        }
        let calls_and_uses = uses.calls.len() + uses.symbols.len();
        let reach = Reach::new(&program.modules, &program.symbols);
        assert!(
            reach.steps <= calls_and_uses + modules,
            "{} steps for {calls_and_uses} calls and uses",
            reach.steps
        );
        // Each global's lookup of what its value reads costs its one call.
        let last = SymbolId::new(&format!("s_f{}", modules - 1)).expect("id");
        let start = Uses {
            calls: BTreeSet::from([&last]),
            ..Uses::default()
        };
        assert!(reach.reads(&start, &[], &program.symbols).is_empty());
    }

    /// Ordering 40,000 declarations, a chain of them needing the next one,
    /// takes about linear time; rescanning every node for each one took
    /// minutes.
    #[test]
    fn many_declarations_are_ordered_in_about_linear_time() {
        let mut graph = Graph::default();
        let count = 40_000;
        for index in 0..count {
            graph.add(node(NodeKind::Other, &format!("g{index:05}")));
        }
        for index in 0..count - 1 {
            if index % 2 == 0 {
                need_of(&mut graph, index, index + 1, Need::Complete);
            }
        }
        let started = std::time::Instant::now();
        let order = graph.order();
        let took = started.elapsed();
        assert_eq!(order.len(), count);
        assert!(took < std::time::Duration::from_secs(10), "{took:?}");
    }
}
