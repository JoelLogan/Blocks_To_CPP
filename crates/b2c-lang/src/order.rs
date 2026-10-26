//! The declaration graph (docs/plans/m3-contracts/modules.md §4, ADR-0019;
//! w2.md §2.2): in which order a module's declarations are printed, and the
//! cycles C++ cannot order (`B2C-E0420`, `B2C-E0421`).
//!
//! Nodes are a module's type definitions (structs and enums) and function
//! declarations; `main` is not one. An edge from `a` to `b` says that `b`
//! comes before `a`:
//!
//! * **complete**: `a` holds `b` by value, so `b`'s definition must be
//!   before `a`'s: a field of type `b`, or `b` inside a container other than
//!   a list (the standard allows an element type that is not complete yet
//!   only in `std::vector`), or `b` anywhere in a field of a struct that is
//!   comparable or printable, whose generated operators compare or print the
//!   field. An enum is always needed complete.
//! * **forward**: `a` names the struct `b` only inside lists (`std::vector`,
//!   at any depth): a forward declaration (`struct b;`) is enough, which
//!   code generation prints when `b`'s definition is not already before `a`.
//!   Such an edge is kept where it can be, so that a definition comes first
//!   whenever no cycle needs the forward declaration, and never closes a
//!   cycle.
//!
//! Functions need nothing before their declarations until default arguments
//! arrive (package C2 adds them, with globals and their `initialised`
//! edges). When several declarations are ready, types come before functions
//! (06 §6.7 step 3), each sorted by name, then block ID; functions keep the
//! flat layout's order, so a program with only functions prints exactly as
//! before.
//!
//! A cycle of complete edges through struct fields is `B2C-E0420`, reported
//! at each struct of the cycle; any other cycle (C2's globals) is
//! `B2C-E0421`. Every walk is iterative.

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::diag::Location;
use b2c_ir::ids::{BlockId, SymbolId};
use b2c_ir::layout::Layout;
use b2c_ir::sast::{ItemKind, Module, SymbolKind, SymbolTable};
use b2c_ir::types::{LibType, Type};

use crate::codes;
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
    /// A struct.
    Struct,
    /// An enum.
    Enum,
    /// Any other declaration (C2's globals, for instance).
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "W2's globals join the graph with package C2")
    )]
    Other,
}

/// One declaration of the graph.
#[derive(Debug, Clone)]
pub(crate) struct Node {
    /// Its kind.
    pub(crate) kind: NodeKind,
    /// The name it is sorted and reported by.
    pub(crate) name: String,
    /// Its defining block, the last key of the order.
    pub(crate) block: BlockId,
    /// The index of its item in its module.
    pub(crate) item: usize,
}

/// One need of a node: the node it needs, how, and the part of the
/// declaration that needs it (a struct's field), for messages.
#[derive(Debug, Clone)]
pub(crate) struct Dep {
    /// The node needed.
    pub(crate) on: usize,
    /// How.
    pub(crate) need: Need,
    /// The field that needs it, by name, with its location.
    pub(crate) via: Option<(String, Location)>,
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
    fn key(&self, node: usize) -> (u8, &str, &BlockId) {
        let node = &self.nodes[node];
        let rank = match node.kind {
            NodeKind::Struct | NodeKind::Enum => 0,
            NodeKind::Other => 1,
        };
        (rank, node.name.as_str(), &node.block)
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
        let mut placed = vec![false; count];
        let mut out = Vec::with_capacity(count);
        // Whether every need of `node` is met: its complete ones, its forward
        // ones too with `soft`, all but those inside its own cycle with
        // `cycle`.
        let ready = |placed: &[bool], node: usize, soft: bool, cycle: bool| {
            self.deps[node].iter().all(|dep| {
                dep.on == node
                    || placed[dep.on]
                    || (!soft && dep.need == Need::Forward)
                    || (cycle && cycle_of[node] != usize::MAX && cycle_of[dep.on] == cycle_of[node])
            })
        };
        while out.len() < count {
            let first = |soft: bool, cycle: bool| {
                (0..count)
                    .filter(|node| !placed[*node] && ready(&placed, *node, soft, cycle))
                    .min_by(|a, b| self.key(*a).cmp(&self.key(*b)))
            };
            let Some(next) = first(true, false)
                .or_else(|| first(false, false))
                .or_else(|| first(false, true))
                .or_else(|| {
                    (0..count)
                        .filter(|node| !placed[*node])
                        .min_by(|a, b| self.key(*a).cmp(&self.key(*b)))
                })
            else {
                break;
            };
            placed[next] = true;
            out.push(next);
        }
        out
    }

    /// The cycles of complete edges: each strongly connected component of
    /// those edges with more than one node, or with a node that needs
    /// itself, in node order (Tarjan's algorithm, iteratively).
    pub(crate) fn cycles(&self) -> Vec<Vec<usize>> {
        let count = self.nodes.len();
        let complete = |node: usize| {
            self.deps[node]
                .iter()
                .filter(|dep| dep.need == Need::Complete)
                .map(|dep| dep.on)
                .collect::<Vec<usize>>()
        };
        let successors: Vec<Vec<usize>> = (0..count).map(complete).collect();
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
                    component.sort_unstable();
                    let looped = component.len() > 1 || successors[node].contains(&node);
                    if looped {
                        components.push(component);
                    }
                }
            }
        }
        components.sort();
        components
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
        for cycle in self.cycles() {
            let members: BTreeSet<usize> = cycle.iter().copied().collect();
            let structs = cycle
                .iter()
                .all(|node| self.nodes[*node].kind == NodeKind::Struct);
            // One walk round the cycle serves every node on it; a node off
            // that walk gets its own.
            let start = cycle.first().copied();
            let round = start
                .map(|first| self.path_back(first, &members))
                .unwrap_or_default();
            let on_round: BTreeMap<usize, usize> =
                round.iter().enumerate().map(|(at, dep)| (dep.on, at)).collect();
            for &node in &cycle {
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

/// The type definitions of a module as a graph: one node per struct and enum
/// item whose symbol is in the table, with the needs of each struct's fields
/// on the other types of the module.
pub(crate) fn module_graph(module: &Module, symbols: &SymbolTable) -> Graph {
    let mut graph = Graph::default();
    let mut by_symbol: BTreeMap<&SymbolId, usize> = BTreeMap::new();
    for (index, item) in module.items.iter().enumerate() {
        let (symbol, kind) = match &item.kind {
            ItemKind::Struct(def) => (&def.symbol, NodeKind::Struct),
            ItemKind::Enum(def) => (&def.symbol, NodeKind::Enum),
            _ => continue,
        };
        let Some(found) = symbols.get(symbol) else {
            continue;
        };
        let node = graph.add(Node {
            kind,
            name: found.name.as_str().to_owned(),
            block: item.origin.block.clone(),
            item: index,
        });
        by_symbol.entry(symbol).or_insert(node);
    }
    for (symbol, &node) in &by_symbol {
        let Some(SymbolKind::Struct {
            fields,
            comparable,
            printable,
        }) = symbols.get(symbol).map(|symbol| &symbol.kind)
        else {
            continue;
        };
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
                let need = if *comparable || *printable {
                    Need::Complete
                } else {
                    need
                };
                graph.deps[node].push(Dep {
                    on,
                    need,
                    via: Some((field.name.as_str().to_owned(), field.origin.location())),
                });
            }
        }
    }
    graph
}

/// Puts each module's declarations in the declaration graph's order: its
/// types first, in [`Graph::order`], then its functions as `layout` already
/// lists them. Reports the cycles.
pub(crate) fn order_declarations(
    modules: &[Module],
    symbols: &SymbolTable,
    layout: &mut Layout,
    diags: &mut Diags,
) {
    for (module, files) in modules.iter().zip(&mut layout.modules) {
        let graph = module_graph(module, symbols);
        graph.report_cycles(diags);
        let mut order: Vec<usize> = graph
            .order()
            .into_iter()
            .map(|node| graph.nodes[node].item)
            .collect();
        order.append(&mut files.source_order);
        files.source_order = order;
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
            block: BlockId::new(&format!("b_{name}")).expect("id"),
            item: 0,
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
}
