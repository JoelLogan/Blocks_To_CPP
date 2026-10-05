//! The editor's questions about an analysis (spec §6.5): which symbols are
//! visible at a block (the scope query), every symbol of the program, and the
//! static type of each value block.
//!
//! The scope query answers from an index that the lowering records as it
//! walks, from the same scope stack it resolves names with ([`Scopes`]), so a
//! dropdown never offers a symbol that the analyser would reject at that
//! point. For each lowered block, and for each statement list it lowers, the
//! index keeps one node of the scope stack's [`Trail`]: the index grows with
//! the number of blocks and declarations, never with their product.
//!
//! [`Scopes`]: crate::scope::Scopes

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::diag::Part;
use b2c_ir::ids::{BlockId, SymbolId};
use b2c_ir::sast::{Expr, ItemKind, Program, Stmt};
use b2c_ir::scope_info::SymbolInfo;
use b2c_ir::types::Type;

use crate::Analysis;
use crate::scope::{NodeId, Trail};

/// What was visible at a recorded position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Point {
    /// The scope stack's trail node there (`None`: no local symbol).
    pub(crate) node: Option<NodeId>,
    /// Index of the module being lowered, whose functions are visible.
    pub(crate) module: usize,
}

/// The positions recorded for one block.
#[derive(Debug, Clone, PartialEq, Default)]
struct BlockPoints {
    /// At the block itself.
    at: Option<Point>,
    /// At the start of each statement list the block owns, and at each value
    /// input where less is visible than at the block (inside the starting
    /// value of a variable, or the header of a `for` loop, the name being
    /// declared hides any outer symbol with that name), by input name.
    inputs: BTreeMap<String, Point>,
}

/// Everything the scope query needs, recorded while lowering.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct ScopeIndex {
    /// The scope stack's trail, shared by every recorded position.
    trail: Trail,
    /// Recorded positions, by block.
    blocks: BTreeMap<BlockId, BlockPoints>,
    /// The functions that calls in each module can use, by module index:
    /// `(name, symbol)` in symbol order.
    functions: Vec<Vec<(String, SymbolId)>>,
    /// The user's name of every symbol in the table, when it differs from the
    /// C++ name there (an invalid name gets a placeholder in the table).
    names: BTreeMap<SymbolId, String>,
}

impl ScopeIndex {
    /// Records what is visible at a block. A block that is lowered twice (a
    /// damaged file reusing a block ID) keeps its first position.
    pub(crate) fn record_block(&mut self, block: &BlockId, point: Point) {
        let points = self.blocks.entry(block.clone()).or_default();
        points.at.get_or_insert(point);
    }

    /// Records what is visible at the start of a statement list, or inside a
    /// value input, of a block (the first position wins, as above).
    pub(crate) fn record_input(&mut self, block: &BlockId, input: &str, point: Point) {
        let points = self.blocks.entry(block.clone()).or_default();
        if !points.inputs.contains_key(input) {
            points.inputs.insert(input.to_owned(), point);
        }
    }

    /// Remembers the user's name of a symbol whose C++ name differs from it.
    pub(crate) fn record_name(&mut self, sym: &SymbolId, name: &str) {
        self.names.insert(sym.clone(), name.to_owned());
    }

    /// Completes the index with the scope stack's trail and the functions of
    /// each module (`(module index, name, symbol)`, in any order).
    pub(crate) fn finish<'a>(
        &mut self,
        trail: Trail,
        modules: usize,
        functions: impl IntoIterator<Item = (usize, &'a str, &'a SymbolId)>,
    ) {
        self.trail = trail;
        self.functions = vec![Vec::new(); modules];
        for (module, name, sym) in functions {
            if let Some(list) = self.functions.get_mut(module) {
                list.push((name.to_owned(), sym.clone()));
            }
        }
        for list in &mut self.functions {
            list.sort_by(|a, b| a.1.cmp(&b.1));
        }
    }

    /// The position a query asks about: the statement list or value input
    /// when one was recorded under that name, the block itself otherwise.
    fn point(&self, block: &BlockId, input: Option<&str>) -> Option<Point> {
        let points = self.blocks.get(block)?;
        input
            .and_then(|name| points.inputs.get(name))
            .or(points.at.as_ref())
            .copied()
    }

    /// The symbols visible at a position, innermost first, then the
    /// functions of its module. A name declared again in an inner list hides
    /// the outer symbol (C++ would find the inner one, and the analyser
    /// reports a reference to the outer one), and a local name hides a
    /// function with that name.
    fn visible(&self, point: Point) -> Vec<&SymbolId> {
        let mut names: BTreeSet<&str> = BTreeSet::new();
        let mut found = Vec::new();
        let mut node = point.node;
        while let Some(index) = node {
            let Some(step) = self.trail.get(index) else {
                break;
            };
            if names.insert(step.name.as_str())
                && let Some(sym) = &step.sym
            {
                found.push(sym);
            }
            // Parents always come first; the filter keeps a corrupted index
            // from looping.
            node = step.parent.filter(|parent| *parent < index);
        }
        for (name, sym) in self.functions.get(point.module).into_iter().flatten() {
            if !names.contains(name.as_str()) {
                found.push(sym);
            }
        }
        found
    }
}

impl Analysis {
    /// The symbols that blocks may refer to at `block` (spec §6.5): what the
    /// editor's dropdowns, the dynamic Variables category and pasting offer.
    ///
    /// * With `input` `None`, or the name of a value input or field of the
    ///   block, the result is what is visible **at the block**: the variables
    ///   declared before it in the same and the enclosing statement lists, the
    ///   counter of each `for` loop it is inside, the parameters of the
    ///   function it is inside, and every function of its module (definition
    ///   order never matters). Inside the starting value of a `create`
    ///   block, or the start, end and step of a `for` loop, the variable
    ///   being created and any outer variable with its name are left out.
    /// * With the name of one of the block's statement inputs (`BODY`, `DO0`,
    ///   `ELSE`, …), the result is what is visible at the **start of that
    ///   list**, including the counter or parameters the block itself declares.
    ///
    /// Exactly the symbols that the analyser accepts a reference to at that
    /// point are listed: a symbol that a newer declaration with the same name
    /// hides is left out, and so are declarations in disabled blocks,
    /// functions of other modules and functions hidden by a local name. A
    /// block ID that the document does not have, and a block the analyser
    /// does not reach (disabled or inside a disabled block, not attached to
    /// `when program starts` or a function, or nested too deeply), give an
    /// empty list.
    ///
    /// The result is sorted by name (byte order), then by ID.
    pub fn symbols_in_scope(&self, block: &BlockId, input: Option<&str>) -> Vec<SymbolInfo> {
        match self.index.point(block, input) {
            Some(point) => self.infos(self.index.visible(point)),
            None => Vec::new(),
        }
    }

    /// Every symbol of the program (those of disabled and unattached blocks
    /// are not part of it), sorted by name (byte order), then by ID.
    pub fn symbol_infos(&self) -> Vec<SymbolInfo> {
        self.infos(self.program.symbols.symbols.keys().collect())
    }

    /// The static type of each value block of the program, by block ID: the
    /// type of the expression that the block became ([`Type::Error`] when it
    /// has an error). Expression slots are not blocks and are not listed;
    /// blocks outside the program (disabled, unattached, or in a second
    /// `when program starts`) are not either.
    pub fn block_types(&self) -> BTreeMap<BlockId, Type> {
        block_types(&self.program)
    }

    /// The records of these symbols (each once, unknown ones skipped), sorted.
    fn infos(&self, ids: Vec<&SymbolId>) -> Vec<SymbolInfo> {
        let mut seen = BTreeSet::new();
        let mut infos: Vec<SymbolInfo> = ids
            .into_iter()
            .filter(|id| seen.insert(*id))
            .filter_map(|id| {
                let symbol = self.program.symbols.get(id)?;
                let name = self
                    .index
                    .names
                    .get(id)
                    .map_or_else(|| symbol.name.as_str(), String::as_str);
                Some(SymbolInfo::new(id.clone(), name, symbol))
            })
            .collect();
        infos.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        infos
    }
}

/// The type of every value block in a program (see [`Analysis::block_types`]).
///
/// The walk is iterative, so a program of any depth is safe. A block whose
/// lowering made several nested expressions (an `and` of three conditions is
/// two `&&`) is typed by the outermost one, which the walk meets first. The
/// expression that a statement block itself became (a call whose result is
/// ignored) belongs to a statement, not to a value block, and is skipped.
fn block_types(program: &Program) -> BTreeMap<BlockId, Type> {
    let mut types = BTreeMap::new();
    let mut stmts: Vec<&Stmt> = Vec::new();
    for item in program.modules.iter().flat_map(|module| &module.items) {
        let body = match &item.kind {
            ItemKind::Main(main) => &main.body,
            ItemKind::Function(function) => &function.body,
        };
        stmts.extend(body.stmts.iter().rev());
    }
    let mut exprs: Vec<&Expr> = Vec::new();
    while let Some(stmt) = stmts.pop() {
        let (values, bodies) = crate::nesting::parts(stmt);
        for body in bodies.into_iter().rev() {
            stmts.extend(body.stmts.iter().rev());
        }
        exprs.extend(values.into_iter().rev());
        while let Some(expr) = exprs.pop() {
            if expr.origin.part == Part::Whole && expr.origin.block != stmt.origin.block {
                types
                    .entry(expr.origin.block.clone())
                    .or_insert_with(|| expr.ty.clone());
            }
            exprs.extend(crate::flow::children(expr).into_iter().rev());
        }
    }
    types
}

#[cfg(test)]
mod tests {
    use b2c_ir::ids::ModuleId;
    use b2c_ir::sast::{Block, Origin, StmtKind};

    use super::*;
    use crate::scope::TrailNode;

    fn block(id: &str) -> BlockId {
        BlockId::new(id).expect("id")
    }

    fn sym(id: &str) -> SymbolId {
        SymbolId::new(id).expect("id")
    }

    fn node(parent: Option<NodeId>, name: &str, symbol: Option<&str>) -> TrailNode {
        TrailNode {
            parent,
            name: name.to_owned(),
            sym: symbol.map(sym),
        }
    }

    fn ids(found: &[&SymbolId]) -> Vec<String> {
        found.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn inner_names_hide_outer_ones_and_functions() {
        let mut index = ScopeIndex::default();
        let trail = vec![
            node(None, "x", Some("x_outer")),
            node(Some(0), "y", Some("y")),
            node(Some(1), "x", Some("x_inner")),
            node(Some(1), "f", None),
        ];
        let functions = [(0, "f", &sym("f")), (0, "g", &sym("g")), (1, "h", &sym("h"))];
        index.finish(trail, 2, functions);
        let at = |node, module| index.visible(Point { node, module });
        assert_eq!(ids(&at(None, 0)), ["f", "g"]);
        assert_eq!(ids(&at(None, 1)), ["h"]);
        assert_eq!(ids(&at(None, 7)), Vec::<String>::new(), "an unknown module");
        assert_eq!(ids(&at(Some(1), 0)), ["y", "x_outer", "f", "g"]);
        assert_eq!(ids(&at(Some(2), 0)), ["x_inner", "y", "f", "g"]);
        assert_eq!(
            ids(&at(Some(3), 0)),
            ["y", "x_outer", "g"],
            "a hidden name hides f"
        );
        assert_eq!(ids(&at(Some(99), 0)), ["f", "g"], "a node outside the trail");
    }

    #[test]
    fn a_corrupted_trail_cannot_loop() {
        let mut index = ScopeIndex::default();
        let trail = vec![node(Some(1), "a", Some("a")), node(Some(0), "b", Some("b"))];
        index.finish(trail, 0, []);
        let found = index.visible(Point {
            node: Some(1),
            module: 0,
        });
        assert_eq!(ids(&found), ["b", "a"]);
    }

    #[test]
    fn the_first_recording_wins_and_inputs_fall_back_to_the_block() {
        let mut index = ScopeIndex::default();
        let first = Point {
            node: Some(1),
            module: 0,
        };
        let second = Point {
            node: None,
            module: 3,
        };
        index.record_block(&block("b"), first);
        index.record_block(&block("b"), second);
        index.record_input(&block("b"), "BODY", second);
        index.record_input(&block("b"), "BODY", first);
        index.record_input(&block("only_list"), "BODY", first);
        assert_eq!(index.point(&block("b"), None), Some(first));
        assert_eq!(index.point(&block("b"), Some("BODY")), Some(second));
        assert_eq!(index.point(&block("b"), Some("VALUE")), Some(first));
        assert_eq!(index.point(&block("only_list"), Some("BODY")), Some(first));
        assert_eq!(index.point(&block("only_list"), None), None);
        assert_eq!(index.point(&block("nowhere"), Some("BODY")), None);
    }

    #[test]
    fn the_index_grows_with_the_blocks_not_with_their_product() {
        let count = 400;
        let body: Vec<serde_json::Value> = (0..count)
            .map(|i| {
                serde_json::json!({"id": format!("d{i}"), "type": "var.declare", "v": 1,
                    "fields": {"TYPE": "int", "NAME": {"sym": format!("s{i}"), "name": format!("v{i}")}}})
            })
            .collect();
        let document: b2c_model::Document = serde_json::from_value(serde_json::json!({
            "format": "blocks2cpp/project", "formatVersion": 1,
            "generator": {"app": "0.1.0", "catalog": "1.0.0"},
            "project": {"id": "p", "name": "T", "language": {"standard": "c++20"}},
            "modules": [{"id": "m", "name": "main", "workspace": {"blocks": [
                {"id": "main", "type": "program.main", "v": 1, "statements": {"BODY": body}}
            ]}}]
        }))
        .expect("document");
        let analysis = crate::analyze(&document);
        // One trail node per declaration and one position per block (each
        // `create` also records its VALUE input, with one hidden name).
        assert_eq!(analysis.index.trail.len(), 2 * count);
        assert_eq!(analysis.index.blocks.len(), count + 1);
        let last = analysis.symbols_in_scope(&block(&format!("d{}", count - 1)), None);
        assert_eq!(last.len(), count - 1);
        assert!(analysis.index.names.is_empty(), "every name is valid");
    }

    #[test]
    fn deep_programs_are_typed_without_recursion() {
        let module = ModuleId::new("m").expect("id");
        let leaf = Expr {
            kind: b2c_ir::sast::ExprKind::Bool(true),
            ty: Type::Bool,
            origin: Origin::whole(module.clone(), block("leaf")),
        };
        let mut body = Block {
            stmts: vec![Stmt {
                kind: StmtKind::Eval { expr: leaf },
                origin: Origin::whole(module.clone(), block("eval")),
                comment: None,
            }],
        };
        // Far deeper than any recursive walk could go on a test thread's stack.
        for _ in 0..100_000 {
            body = Block {
                stmts: vec![Stmt {
                    kind: StmtKind::Forever { body },
                    origin: Origin::whole(module.clone(), block("loop")),
                    comment: None,
                }],
            };
        }
        let program = Program {
            standard: b2c_ir::sast::CppStandard::Cpp20,
            modules: vec![b2c_ir::sast::Module {
                id: module.clone(),
                name: String::from("main"),
                items: vec![b2c_ir::sast::Item {
                    kind: ItemKind::Main(b2c_ir::sast::MainDef { body }),
                    origin: Origin::whole(module, block("main")),
                    comment: None,
                }],
            }],
            symbols: b2c_ir::sast::SymbolTable::default(),
        };
        let types = block_types(&program);
        assert_eq!(types.len(), 1);
        assert_eq!(types.get(&block("leaf")), Some(&Type::Bool));
        // Dropping the tree as a whole would recurse; take it apart level by
        // level instead (the analyser itself never builds such a tree).
        let mut stmts: Vec<Stmt> = program
            .modules
            .into_iter()
            .flat_map(|module| module.items)
            .flat_map(|item| match item.kind {
                ItemKind::Main(main) => main.body.stmts,
                ItemKind::Function(function) => function.body.stmts,
            })
            .collect();
        while let Some(stmt) = stmts.pop() {
            if let StmtKind::Forever { body } = stmt.kind {
                stmts.extend(body.stmts);
            }
        }
    }
}
