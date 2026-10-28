//! The editor's questions about an analysis (spec §6.5): which symbols are
//! visible at a block (the scope query), every symbol of the program, and the
//! static type of each value block.
//!
//! The scope query answers from an index that the lowering records as it
//! walks, from the same scope stack it resolves names with ([`Scopes`]), so a
//! dropdown on an attached block never offers a symbol that is out of scope
//! there. For each lowered block, and for each statement list it lowers, the
//! index keeps one node of the scope stack's [`Trail`]: the index grows with
//! the number of blocks and declarations, never with their product.
//!
//! Blocks the analyser does not lower answer too (06 §6.5), each with one
//! position of the same trail:
//!
//! * a disabled block in a list or input that is lowered answers for its
//!   position, like an enabled block there, and every block nested in it
//!   answers like it ([`ScopeIndex::record_tree`], called by the lowering);
//! * a *parked* block (a top-level statement, reporter or predicate, the
//!   blocks stacked below a top-level block, and every block nested in them,
//!   ADR-0012) answers with the parked scope of its module: what is visible
//!   at the end of the module's `main` body, or the module's functions alone
//!   when it has no `main` ([`ScopeIndex::record_unlowered`]);
//! * a definition that is not lowered (a disabled one, or one too damaged to
//!   lower), and every block in it, answers with the module's functions.
//!
//! Each of these walks is iterative and stops at [`MAX_BLOCK_DEPTH`], like the
//! lowering, so a block nested too deeply anywhere gives no answer.
//!
//! The answer also lists the fields of each struct and the enumerators of
//! each enum it lists, with their owner (types.md §2.4): blocks refer to
//! them through their type, and a paste binds them through it. No name
//! hides them, and they hide no name.
//!
//! [`Scopes`]: crate::scope::Scopes

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::diag::Part;
use b2c_ir::ids::{BlockId, ModuleId, SymbolId};
use b2c_ir::sast::{Expr, ItemKind, Program, Stmt, SymbolKind};
use b2c_ir::scope_info::{SymbolInfo, SymbolInfoKind};
use b2c_ir::types::Type;
use b2c_model::{Block, Document, Input};

use crate::Analysis;
use crate::collect::MAX_BLOCK_DEPTH;
use crate::scope::{NodeId, Trail};

/// The top-level blocks that are definitions, which are never parked
/// (ADR-0012): the lowering lowers them as items, or skips them as a whole.
/// W2's are among them (w2.md §2.13); a test checks the list against the
/// catalog.
const DEFINITION_TYPES: [&str; 6] = [
    "program.main",
    "func.define",
    "type.struct",
    "type.enum",
    "var.global",
    "program.use_header",
];

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
    /// What is visible at the end of the body of each module's `main` (the
    /// first one lowered there), by module index.
    main_ends: BTreeMap<usize, Point>,
    /// The variables a block declares for one of its own statement lists
    /// only: a catch part's error (w2.md §2.5) and a *for each* item (a copy
    /// or an editable one is a variable), which are not visible after the
    /// block ([`Analysis::scope_after`]).
    part_variables: BTreeSet<SymbolId>,
}

impl ScopeIndex {
    /// Records what is visible at a block that is lowered, or at a block the
    /// lowering skips that answers for a position (see the module docs). A
    /// block recorded twice (a damaged file reusing a block ID) keeps its
    /// first position.
    pub(crate) fn record_block(&mut self, block: &BlockId, point: Point) {
        let points = self.blocks.entry(block.clone()).or_default();
        points.at.get_or_insert(point);
    }

    /// Records a variable that a block declares for one of its own
    /// statement lists only (a catch part's error, a *for each* item), so
    /// that it is not visible after the block.
    pub(crate) fn record_part_variable(&mut self, sym: &SymbolId) {
        self.part_variables.insert(sym.clone());
    }

    /// Records what is visible at the end of a `main` body, the parked scope
    /// of its module (the first `main` lowered in a module wins).
    pub(crate) fn record_main_end(&mut self, end: Point) {
        self.main_ends.entry(end.module).or_insert(end);
    }

    /// The parked scope of a module: what is visible at the end of its
    /// `main` body, or, without one, its functions alone.
    fn parked_point(&self, module: usize) -> Point {
        self.main_ends
            .get(&module)
            .copied()
            .unwrap_or(Point { node: None, module })
    }

    /// Records `point` for `root`, a block at nesting `level` (a top-level
    /// definition is at level 0, a statement of its body at level 1, and each
    /// statement list or value input one more), and for every block nested
    /// in it: its statement lists and value inputs, not its `stack`. Blocks
    /// deeper than [`MAX_BLOCK_DEPTH`] are left out, as the lowering leaves
    /// them out. The walk is iterative, and returns how many blocks it
    /// visited (each at most once).
    pub(crate) fn record_tree(&mut self, root: &Block, point: Point, level: usize) -> usize {
        let mut visited = 0;
        let mut pending: Vec<(&Block, usize)> = vec![(root, level)];
        while let Some((block, level)) = pending.pop() {
            if level > MAX_BLOCK_DEPTH {
                continue;
            }
            visited += 1;
            self.record_block(&block.id, point);
            pending.extend(
                block
                    .statements
                    .values()
                    .flatten()
                    .map(|nested| (nested, level + 1)),
            );
            pending.extend(block.inputs.values().filter_map(|input| match input {
                Input::Block(nested) => Some((&*nested.block, level + 1)),
                Input::Expr(_) => None,
            }));
        }
        visited
    }

    /// Records the blocks of a document that the lowering never reached as a
    /// whole: each top-level block that is not a definition, with its
    /// `stack`, and the blocks stacked below a definition, answer with the
    /// parked scope of their module; a definition the lowering skipped
    /// (disabled, or too damaged to lower) answers, with everything in it,
    /// with the module's functions. Called once the lowering has recorded
    /// every `main` end. Returns how many blocks it visited.
    pub(crate) fn record_unlowered(&mut self, document: &Document) -> usize {
        let mut visited = 0;
        for (module, workspace) in document.modules.iter().map(|m| &m.workspace).enumerate() {
            let parked = self.parked_point(module);
            for top in &workspace.blocks {
                if !DEFINITION_TYPES.contains(&top.block_type.as_str()) {
                    // A parked head is placed like a statement of a body.
                    visited += self.record_tree(top, parked, 1);
                } else if self.blocks.get(&top.id).is_none_or(|points| points.at.is_none()) {
                    // A definition the lowering skipped: its module's functions.
                    visited += self.record_tree(top, Point { node: None, module }, 0);
                }
                for stacked in &top.stack {
                    visited += self.record_tree(stacked, parked, 1);
                }
            }
        }
        visited
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
    /// For an attached block, exactly the symbols that the analyser accepts a
    /// reference to at that point are listed: a symbol that a newer
    /// declaration with the same name hides is left out, and so are
    /// declarations in disabled blocks, functions of other modules and
    /// functions hidden by a local name.
    ///
    /// With a struct or an enum, the answer lists its fields or enumerators
    /// too (`field` and `enumerator` records, with their `owner`): blocks
    /// refer to them through their type, so no local name hides them, and
    /// they hide nothing.
    ///
    /// Blocks the analyser does not lower answer too, with any `input`:
    ///
    /// * A disabled block in a statement list or value input the analyser
    ///   reaches answers for its position, like an enabled block there (its
    ///   own dropdowns, and a paste directly after it): a disabled
    ///   declaration declares nothing, so the same symbols are visible at it
    ///   and after it. Every block nested in it answers like it.
    /// * A parked block (a top-level statement, reporter or predicate, a
    ///   block in a parked stack, or one nested in them; ADR-0012) answers
    ///   with the **parked scope** of its module ([`Self::parked_scope`]).
    ///   Declarations inside parked blocks declare nothing.
    /// * A definition the analyser skips (a disabled `when program starts`
    ///   or function), and every block in it, answers with the module's
    ///   functions.
    ///
    /// A block ID that the document does not have, and a block nested too
    /// deeply, give an empty list ([`Self::scope_at`] tells them apart from a
    /// block that sees nothing).
    ///
    /// The result is sorted by name (byte order), then by ID.
    pub fn symbols_in_scope(&self, block: &BlockId, input: Option<&str>) -> Vec<SymbolInfo> {
        self.scope_at(block, input).unwrap_or_default()
    }

    /// [`Self::symbols_in_scope`], or `None` when the analysis has no answer
    /// for the block: the document does not have it (for the editor: it was
    /// added after this analysis), or it is nested too deeply. `Some` with an
    /// empty list means that nothing is visible there.
    pub fn scope_at(&self, block: &BlockId, input: Option<&str>) -> Option<Vec<SymbolInfo>> {
        let point = self.index.point(block, input)?;
        Some(self.infos(self.with_members(self.index.visible(point))))
    }

    /// What is visible **directly after** a block in its statement list,
    /// where a block inserted after it would be: what is visible at the
    /// block, plus the variable the block creates if the analyser accepted
    /// that declaration (it hides any other symbol with its name). A
    /// disabled or parked `create` declares nothing, and a `for` loop's
    /// counter, a *for each* item and a catch part's error are visible only
    /// inside their block, so for those it is what is visible at the block.
    /// `None` when the analysis has no answer for the block
    /// ([`Self::scope_at`]). Sorted like [`Self::symbols_in_scope`].
    pub fn scope_after(&self, block: &BlockId) -> Option<Vec<SymbolInfo>> {
        let mut visible = self.scope_at(block, None)?;
        let created = self
            .program
            .symbols
            .symbols
            .iter()
            .find(|(id, symbol)| {
                symbol.origin.block == *block
                    && matches!(symbol.kind, SymbolKind::Variable { .. })
                    && !self.index.part_variables.contains(*id)
            })
            .map(|(id, _)| id);
        if let Some(own) = created.and_then(|id| self.infos(vec![id]).pop()) {
            // The new variable hides what has its name, but not the members
            // of a type, which are reached through the type.
            visible.retain(|info| info.name != own.name || is_member(info));
            visible.push(own);
            visible.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
        }
        Some(visible)
    }

    /// The **parked scope** of a module (06 §6.5): what a block placed on
    /// its canvas sees. That is what is visible at the end of the module's
    /// `when program starts` body (the first one there, if it has several),
    /// or, when the module has none that the analyser lowers, the module's
    /// functions alone. Every parked block of the module answers with it,
    /// and a paste onto the canvas binds references with it. Empty for a
    /// module the document does not have. Sorted like
    /// [`Self::symbols_in_scope`].
    pub fn parked_scope(&self, module: &ModuleId) -> Vec<SymbolInfo> {
        match self.program.modules.iter().position(|m| m.id == *module) {
            Some(index) => self.infos(self.with_members(self.index.visible(self.index.parked_point(index)))),
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

    /// These symbols, each struct and enum among them followed by its fields
    /// or enumerators: those of its members that the table has as members of
    /// it (types.md §2.4).
    fn with_members<'a>(&'a self, ids: Vec<&'a SymbolId>) -> Vec<&'a SymbolId> {
        let symbols = &self.program.symbols;
        let mut out = Vec::with_capacity(ids.len());
        for id in ids {
            out.push(id);
            let members = match symbols.get(id).map(|symbol| &symbol.kind) {
                Some(SymbolKind::Struct { fields, .. }) => fields.as_slice(),
                Some(SymbolKind::Enum { values }) => values.as_slice(),
                _ => &[],
            };
            out.extend(members.iter().filter(|member| {
                matches!(
                    symbols.get(member).map(|symbol| &symbol.kind),
                    Some(SymbolKind::Field { owner } | SymbolKind::Enumerator { owner }) if owner == id
                )
            }));
        }
        out
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

/// A position of the scope index ([`ScopeIndexParts`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopePoint {
    /// The trail node there (`None`: no local symbol).
    pub node: Option<usize>,
    /// The index of the module whose functions are visible.
    pub module: usize,
}

impl From<Point> for ScopePoint {
    fn from(point: Point) -> Self {
        Self {
            node: point.node,
            module: point.module,
        }
    }
}

/// The positions recorded for one block ([`ScopeIndexParts::blocks`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeBlock<'a> {
    /// The block.
    pub id: &'a BlockId,
    /// The position at the block, if one was recorded there.
    pub at: Option<ScopePoint>,
    /// The position of each statement list or value input recorded under
    /// its own name, sorted by name.
    pub inputs: Vec<(&'a str, ScopePoint)>,
}

/// A read-only view of the scope index behind [`Analysis::scope_at`], for an
/// encoder that answers the same questions elsewhere (the editor's preview
/// worker, docs/plans/m3-contracts/worker.md §4). A lookup over these parts
/// that walks the trail as [`Analysis::scope_at`] does (each name counted
/// once, a parent followed only when it is smaller than its node, then the
/// module's functions whose names were not seen, then each struct's and
/// enum's [`Self::members`]) gives exactly its answers.
#[derive(Debug, Clone, Copy)]
pub struct ScopeIndexParts<'a> {
    analysis: &'a Analysis,
}

impl<'a> ScopeIndexParts<'a> {
    /// The trail, in order: each node's parent (always a smaller index in an
    /// index the lowering made), the name C++ sees, and the symbol it names
    /// (`None` for a hidden name).
    pub fn trail(self) -> impl Iterator<Item = (Option<usize>, &'a str, Option<&'a SymbolId>)> + 'a {
        self.analysis
            .index
            .trail
            .iter()
            .map(|node| (node.parent, node.name.as_str(), node.sym.as_ref()))
    }

    /// Every recorded block, sorted by ID, with its recorded positions.
    pub fn blocks(self) -> impl Iterator<Item = ScopeBlock<'a>> + 'a {
        self.analysis.index.blocks.iter().map(|(id, points)| ScopeBlock {
            id,
            at: points.at.map(ScopePoint::from),
            inputs: points
                .inputs
                .iter()
                .map(|(name, point)| (name.as_str(), ScopePoint::from(*point)))
                .collect(),
        })
    }

    /// The modules, by module index: what [`Analysis::parked_scope`] is asked
    /// about by ID.
    pub fn modules(self) -> impl Iterator<Item = &'a ModuleId> + 'a {
        self.analysis.program.modules.iter().map(|module| &module.id)
    }

    /// The functions that calls in each module can use, by module index:
    /// `(C++ name, symbol)`, in symbol order.
    pub fn functions(self) -> impl Iterator<Item = &'a [(String, SymbolId)]> + 'a {
        self.analysis.index.functions.iter().map(Vec::as_slice)
    }

    /// The parked point of each module, by module index (the modules of
    /// [`Self::modules`]): the trail node at the end of its first lowered
    /// `main` body, or `None` when its parked scope is its functions alone.
    pub fn parked(self) -> impl Iterator<Item = Option<usize>> + 'a {
        let index = &self.analysis.index;
        (0..self.analysis.program.modules.len()).map(move |module| index.parked_point(module).node)
    }

    /// Each struct and enum of the table with the members a scope answer
    /// lists after it, in that order (its fields or enumerators whose owner
    /// it is); sorted by the owner's ID, those without members left out.
    pub fn members(self) -> impl Iterator<Item = (&'a SymbolId, Vec<&'a SymbolId>)> + 'a {
        let analysis = self.analysis;
        analysis.program.symbols.symbols.keys().filter_map(move |id| {
            let members = analysis.with_members(vec![id]);
            (members.len() > 1).then(|| (id, members[1..].to_vec()))
        })
    }

    /// The variable each block creates that [`Analysis::scope_after`] adds
    /// after it, sorted by block ID (blocks that create none are left out).
    pub fn created(self) -> Vec<(&'a BlockId, &'a SymbolId)> {
        let analysis = self.analysis;
        let mut created: BTreeMap<&'a BlockId, &'a SymbolId> = BTreeMap::new();
        for (id, symbol) in &analysis.program.symbols.symbols {
            if matches!(symbol.kind, SymbolKind::Variable { .. })
                && !analysis.index.part_variables.contains(id)
            {
                // The first in ID order, as `scope_after` finds it.
                created.entry(&symbol.origin.block).or_insert(id);
            }
        }
        created.into_iter().collect()
    }
}

impl Analysis {
    /// The scope index behind [`Self::scope_at`], [`Self::scope_after`] and
    /// [`Self::parked_scope`], read-only, for an encoder (see
    /// [`ScopeIndexParts`]).
    pub fn scope_index_parts(&self) -> ScopeIndexParts<'_> {
        ScopeIndexParts { analysis: self }
    }
}

/// Whether a record is a field or an enumerator: a member of a user type.
fn is_member(info: &SymbolInfo) -> bool {
    matches!(
        info.kind,
        SymbolInfoKind::Field { .. } | SymbolInfoKind::Enumerator { .. }
    )
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
        let (body, values): (Option<&b2c_ir::sast::Block>, Vec<&Expr>) = match &item.kind {
            ItemKind::Main(main) => (Some(&main.body), Vec::new()),
            // A function's default values (W2) belong to its own block.
            ItemKind::Function(function) => (
                Some(&function.body),
                function
                    .params
                    .iter()
                    .filter_map(|p| p.default.as_ref())
                    .collect(),
            ),
            // A definition's values (W2): a global's starting value, a
            // struct's field defaults.
            ItemKind::Global(global) => (None, global.init.iter().collect()),
            ItemKind::Struct(def) => (None, def.defaults.values().collect()),
            ItemKind::Enum(_) | ItemKind::UseHeader { .. } => (None, Vec::new()),
        };
        if let Some(body) = body {
            stmts.extend(body.stmts.iter().rev());
        }
        record_types(&item.origin.block, values, &mut types);
    }
    while let Some(stmt) = stmts.pop() {
        let (values, bodies) = crate::nesting::parts(stmt);
        for body in bodies.into_iter().rev() {
            stmts.extend(body.stmts.iter().rev());
        }
        record_types(&stmt.origin.block, values, &mut types);
    }
    types
}

/// Records the type of every value block in `values` and their operands,
/// except the expressions the block `owner` itself became.
fn record_types<'p>(owner: &BlockId, values: Vec<&'p Expr>, types: &mut BTreeMap<BlockId, Type>) {
    let mut exprs: Vec<&'p Expr> = values.into_iter().rev().collect();
    while let Some(expr) = exprs.pop() {
        if expr.origin.part == Part::Whole && &expr.origin.block != owner {
            types
                .entry(expr.origin.block.clone())
                .or_insert_with(|| expr.ty.clone());
        }
        exprs.extend(crate::flow::children(expr).into_iter().rev());
    }
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

    /// The definitions are exactly the catalog's top-level blocks (its hat
    /// and definitions), W2's among them (w2.md §2.13, §5.3).
    #[test]
    fn the_definitions_are_the_catalog_s_top_level_blocks() {
        let top_level: std::collections::BTreeSet<&str> = b2c_catalog::core_catalog()
            .blocks
            .values()
            .filter(|def| def.shape.is_top_level())
            .map(|def| def.id.as_str())
            .collect();
        assert_eq!(top_level, DEFINITION_TYPES.into_iter().collect());
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
        let analysis = crate::analyze_with(&document, b2c_catalog::core_catalog().sigs());
        // One trail node per declaration and one position per block (each
        // `create` also records its VALUE input, with one hidden name).
        assert_eq!(analysis.index.trail.len(), 2 * count);
        assert_eq!(analysis.index.blocks.len(), count + 1);
        let last = analysis.symbols_in_scope(&block(&format!("d{}", count - 1)), None);
        assert_eq!(last.len(), count - 1);
        assert!(analysis.index.names.is_empty(), "every name is valid");
    }

    #[test]
    fn the_index_stays_linear_on_long_disabled_stacks() {
        // 5,000 `create` blocks stacked below a disabled loose head (parked),
        // and 5,000 in a disabled loop in `main`: one position each, the same
        // two for all of them, nothing declared, and each block visited once.
        let count = 5_000;
        let creates = |prefix: &str| -> Vec<serde_json::Value> {
            (0..count)
                .map(|i| {
                    serde_json::json!({"id": format!("{prefix}{i}"), "type": "var.declare", "v": 1,
                        "fields": {"TYPE": "int", "NAME": {"sym": format!("{prefix}_s{i}"), "name": format!("v{i}")}}})
                })
                .collect()
        };
        let document: Document = serde_json::from_value(serde_json::json!({
            "format": "blocks2cpp/project", "formatVersion": 1,
            "generator": {"app": "0.1.0", "catalog": "1.0.0"},
            "project": {"id": "p", "name": "T", "language": {"standard": "c++20"}},
            "modules": [{"id": "m", "name": "main", "workspace": {"blocks": [
                {"id": "main", "type": "program.main", "v": 1, "statements": {"BODY": [
                    {"id": "kept", "type": "var.declare", "v": 1,
                     "fields": {"TYPE": "int", "NAME": {"sym": "s_kept", "name": "kept"}}},
                    {"id": "loop", "type": "control.forever", "v": 1, "disabled": true,
                     "statements": {"BODY": creates("d")}}
                ]}},
                {"id": "head", "type": "io.print", "v": 1, "disabled": true, "x": 0, "y": 0,
                 "stack": creates("p")}
            ]}}]
        }))
        .expect("document");
        let blocks = 2 * count + 4;
        let analysis = crate::analyze_with(&document, b2c_catalog::core_catalog().sigs());
        // `kept` declares one name, and hides it in its own starting value.
        assert_eq!(analysis.index.trail.len(), 2);
        assert_eq!(analysis.index.blocks.len(), blocks);
        let points: BTreeSet<Option<NodeId>> = analysis
            .index
            .blocks
            .values()
            .filter_map(|points| points.at.map(|point| point.node))
            .collect();
        // `main` and `kept` see nothing; the loop, everything in it and the
        // parked stack see `kept` (node 1, after the hiding node 0).
        assert_eq!(points, BTreeSet::from([None, Some(1)]));
        let kept = ["kept"];
        for id in ["d0", "d4999", "p0", "p4999", "head"] {
            let names: Vec<String> = analysis
                .symbols_in_scope(&block(id), None)
                .into_iter()
                .map(|s| s.name)
                .collect();
            assert_eq!(names, kept, "{id}");
        }
        // Every walk visits each block once.
        let mut index = ScopeIndex::default();
        assert_eq!(index.record_unlowered(&document), blocks);
        assert_eq!(index.blocks.len(), blocks);
        let mut index = ScopeIndex::default();
        let point = Point {
            node: None,
            module: 0,
        };
        let looped = &document.modules[0].workspace.blocks[0].statements["BODY"][1];
        assert_eq!(index.record_tree(looped, point, 1), count + 1);
    }

    /// A table with a struct `Point` (fields `x` and `y`, and a field of
    /// another struct listed by mistake), an enum `Color` (`Red`), a function
    /// `area`, the locals `x` and `Point`, and the `y` that `b1` creates.
    fn user_types_table(module: &ModuleId) -> b2c_ir::sast::SymbolTable {
        let symbol = |name: &str, kind: SymbolKind, ty: Type| b2c_ir::sast::Symbol {
            name: b2c_ir::text::Ident::new(name).expect("name"),
            kind,
            ty,
            origin: Origin::whole(module.clone(), block("b_types")),
        };
        let variable = |name: &str, block: &str| {
            let mut local = symbol(name, SymbolKind::Variable { is_const: false }, Type::Int);
            local.origin.block = self::block(block);
            local
        };
        let field =
            |name: &str, owner: &str| symbol(name, SymbolKind::Field { owner: sym(owner) }, Type::Double);
        let point = SymbolKind::Struct {
            fields: vec![sym("f_x"), sym("f_y"), sym("f_stray")],
            comparable: true,
            printable: false,
        };
        let color = SymbolKind::Enum {
            values: vec![sym("e_red")],
        };
        let red = SymbolKind::Enumerator { owner: sym("s_col") };
        let area = SymbolKind::Function {
            params: Vec::new(),
            optional: 0,
        };
        let mut table = b2c_ir::sast::SymbolTable::default();
        for (id, entry) in [
            ("s_x", variable("x", "b0")),
            ("s_local_pt", variable("Point", "b2")),
            ("s_y", variable("y", "b1")),
            ("s_pt", symbol("Point", point, Type::Named(sym("s_pt")))),
            ("f_x", field("x", "s_pt")),
            ("f_y", field("y", "s_pt")),
            ("f_stray", field("stray", "s_other")),
            ("s_col", symbol("Color", color, Type::Named(sym("s_col")))),
            ("e_red", symbol("Red", red, Type::Named(sym("s_col")))),
            ("f_area", symbol("area", area, Type::Double)),
        ] {
            table.symbols.insert(sym(id), entry);
        }
        table
    }

    /// An analysis with [`user_types_table`]: the user types and `area` are
    /// visible everywhere in module `m`, as wave W2's definitions will be;
    /// `b1` sees the local `x`, and `b2`, in an inner list, also a local
    /// `Point` that hides the struct.
    fn with_user_types() -> Analysis {
        let module = ModuleId::new("m").expect("id");
        let table = user_types_table(&module);
        let mut index = ScopeIndex::default();
        let outer = Point {
            node: Some(0),
            module: 0,
        };
        index.record_block(&block("b1"), outer);
        index.record_block(
            &block("b2"),
            Point {
                node: Some(1),
                module: 0,
            },
        );
        index.record_main_end(outer);
        let (pt, col, area) = (sym("s_pt"), sym("s_col"), sym("f_area"));
        index.finish(
            vec![
                node(None, "x", Some("s_x")),
                node(Some(0), "Point", Some("s_local_pt")),
            ],
            1,
            [(0, "Point", &pt), (0, "Color", &col), (0, "area", &area)],
        );
        Analysis {
            program: Program {
                standard: b2c_ir::sast::CppStandard::Cpp20,
                modules: vec![b2c_ir::sast::Module {
                    id: module,
                    name: String::from("main"),
                    items: Vec::new(),
                }],
                symbols: table,
                layout: b2c_ir::layout::Layout::default(),
            },
            diagnostics: Vec::new(),
            index,
            slot_types: BTreeMap::new(),
            constants: BTreeMap::new(),
        }
    }

    fn listed(infos: &[SymbolInfo]) -> Vec<String> {
        infos.iter().map(|info| info.id.to_string()).collect()
    }

    /// The clipboard binds a pasted field or enumerator through its owner
    /// (types.md §2.4), so the scope query lists each visible struct's and
    /// enum's members, with their owner.
    #[test]
    fn user_types_come_with_their_members() {
        let analysis = with_user_types();
        let at = analysis.scope_at(&block("b1"), None).expect("an answer");
        // Sorted by name: `x` the field and `x` the local, by ID.
        assert_eq!(
            listed(&at),
            ["s_col", "s_pt", "e_red", "f_area", "f_x", "s_x", "f_y"]
        );
        let field = at.iter().find(|info| info.id == sym("f_x")).expect("the field");
        assert_eq!(field.kind, SymbolInfoKind::Field { owner: sym("s_pt") });
        let red = at.iter().find(|info| info.id == sym("e_red")).expect("the value");
        assert_eq!(red.kind, SymbolInfoKind::Enumerator { owner: sym("s_col") });
        // A local `Point` hides the struct, and so its fields; not the enum.
        let inner = analysis.scope_at(&block("b2"), Some("BODY")).expect("an answer");
        assert_eq!(listed(&inner), ["s_col", "s_local_pt", "e_red", "f_area", "s_x"]);
        // After `create y`: the new `y` hides nothing of `Point`.
        let after = analysis.scope_after(&block("b1")).expect("an answer");
        assert_eq!(
            listed(&after),
            ["s_col", "s_pt", "e_red", "f_area", "f_x", "s_x", "f_y", "s_y"]
        );
        // The parked scope, which a paste on the canvas binds with.
        let parked = analysis.parked_scope(&ModuleId::new("m").expect("id"));
        assert_eq!(listed(&parked), listed(&at));
        // Every symbol, members included, is in the program's list once.
        assert_eq!(analysis.symbol_infos().len(), 10);
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
                    kind: ItemKind::Main(b2c_ir::sast::MainDef { body, args: None }),
                    origin: Origin::whole(module, block("main")),
                    comment: None,
                    headers: b2c_ir::headers::ItemHeaders::default(),
                    shared: false,
                }],
            }],
            symbols: b2c_ir::sast::SymbolTable::default(),
            layout: b2c_ir::layout::Layout::default(),
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
                _ => Vec::new(),
            })
            .collect();
        while let Some(stmt) = stmts.pop() {
            if let StmtKind::Forever { body } = stmt.kind {
                stmts.extend(body.stmts);
            }
        }
    }

    /// What [`Analysis::scope_at`] answers, worked out from
    /// [`Analysis::scope_index_parts`] alone, as the editor's lookup does.
    fn lookup(analysis: &Analysis, at: Option<ScopePoint>) -> Option<Vec<String>> {
        let parts = analysis.scope_index_parts();
        let ScopePoint { node: start, module } = at?;
        let trail: Vec<_> = parts.trail().collect();
        let mut names = BTreeSet::new();
        let mut found: Vec<&SymbolId> = Vec::new();
        let mut node = start;
        while let Some(index) = node {
            let Some(&(parent, name, sym)) = trail.get(index) else {
                break;
            };
            if names.insert(name) {
                found.extend(sym);
            }
            node = parent.filter(|parent| *parent < index);
        }
        if let Some(functions) = parts.functions().nth(module) {
            found.extend(
                functions
                    .iter()
                    .filter(|(name, _)| !names.contains(name.as_str()))
                    .map(|(_, sym)| sym),
            );
        }
        let members: BTreeMap<_, _> = parts.members().collect();
        let mut out: Vec<&SymbolId> = Vec::new();
        for id in found {
            out.push(id);
            out.extend(members.get(id).into_iter().flatten());
        }
        let mut infos: Vec<SymbolInfo> = analysis.symbol_infos();
        infos.retain(|info| out.contains(&&info.id));
        Some(infos.iter().map(|info| info.id.to_string()).collect())
    }

    /// The parts give exactly the scope query's answers: at every recorded
    /// block and input, in a parked scope, and after a block.
    #[test]
    fn the_parts_answer_as_the_scope_query_does() {
        let analysis = with_user_types();
        let parts = analysis.scope_index_parts();
        let mut asked = 0;
        for ScopeBlock { id, at, inputs } in parts.blocks() {
            assert_eq!(
                lookup(&analysis, at),
                analysis.scope_at(id, None).map(|found| listed(&found))
            );
            for (name, point) in inputs {
                assert_eq!(
                    lookup(&analysis, Some(point)),
                    analysis.scope_at(id, Some(name)).map(|found| listed(&found))
                );
            }
            asked += 1;
        }
        assert_eq!(asked, 2);
        let modules: Vec<_> = parts.modules().collect();
        let parked: Vec<_> = parts.parked().collect();
        assert_eq!(modules.len(), 1);
        assert_eq!(parked, [Some(0)]);
        assert_eq!(
            lookup(
                &analysis,
                Some(ScopePoint {
                    node: parked[0],
                    module: 0
                })
            ),
            Some(listed(&analysis.parked_scope(modules[0])))
        );
        for (node, (parent, _, _)) in parts.trail().enumerate() {
            assert!(parent.is_none_or(|parent| parent < node));
        }
        let members: Vec<_> = parts
            .members()
            .map(|(owner, members)| {
                (
                    owner.to_string(),
                    members.iter().map(ToString::to_string).collect::<Vec<_>>(),
                )
            })
            .collect();
        assert_eq!(
            members,
            [
                (String::from("s_col"), vec![String::from("e_red")]),
                (
                    String::from("s_pt"),
                    vec![String::from("f_x"), String::from("f_y")]
                )
            ]
        );
        let created: Vec<_> = parts
            .created()
            .into_iter()
            .map(|(block, sym)| (block.to_string(), sym.to_string()))
            .collect();
        assert!(
            created.contains(&(String::from("b1"), String::from("s_y"))),
            "{created:?}"
        );
    }
}
