//! Pass 3: flow checks on a lowered item (spec §6.6).
//!
//! * A function that gives back a value must not reach its end (`E0410`).
//! * Statements after one that never completes (a jump, or an endless loop)
//!   can never run (`W0502`).
//! * A scalar variable created without a value should be given one before it
//!   is read (`W0503`). This is a deliberately simple approximation: a use is
//!   reported only when *no* path to it assigns the variable, and loops count
//!   every assignment in their body as possibly done before any use, so there
//!   are no false alarms for values carried from one round to the next.
//! * A `forever` loop that nothing can leave gets an info note (`I0513`).
//! * *go on into the next case* must be the last block of a case that is
//!   not the switch's last part (`E0427`, w2.md §1.6).
//!
//! W2's statements follow the same rules (w2.md §2.13): a switch completes
//! when it has no default part or one of its parts completes, *throw* and
//! *throw the error again* end their path like `return`, a `do … while`
//! body runs at least once, and a `try` completes when its `try` part or a
//! catch part does.

use std::collections::BTreeSet;

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{
    Block, Expr, ExprKind, Item, ItemKind, PassMode, Stmt, StmtKind, SwitchCase, SymbolKind, SymbolTable,
};
use b2c_ir::types::Type;

use crate::codes;
use crate::messages::{Diags, a_type, quoted};
use crate::typing::is_scalar;

/// Runs the flow checks on one item.
pub(crate) fn check_item(item: &Item, symbols: &SymbolTable, diags: &mut Diags) {
    let mut flow = Flow {
        symbols,
        diags,
        loop_depth: 0,
    };
    let mut state = State::default();
    match &item.kind {
        ItemKind::Main(main) => {
            flow.list(&main.body, &mut state);
        }
        ItemKind::Function(function) => {
            let completes = flow.list(&function.body, &mut state);
            if completes && !matches!(function.ret, Type::Void | Type::Error) {
                let name = symbols
                    .get(&function.symbol)
                    .map(|s| s.name.as_str().to_owned())
                    .unwrap_or_default();
                let message = format!(
                    "{} must give back {}, but it can reach its end without a 'return' block. Add a 'return' \
                     block at the end (or on every path).",
                    quoted(&name),
                    a_type(&function.ret, symbols)
                );
                flow.diags
                    .error(codes::MISSING_RETURN, item.origin.location(), message);
            }
        }
        // A definition has no statements; its values are checked where
        // they are lowered.
        ItemKind::Struct(_) | ItemKind::Enum(_) | ItemKind::Global(_) | ItemKind::UseHeader { .. } => {}
    }
}

/// Where *go on into the next case* may stand in a statement list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FallThrough {
    /// Nowhere: the list is not a part of a switch.
    Never,
    /// As the last statement: the list is a case that another part follows.
    Last,
    /// Nowhere, and as its last statement it is told so: the list is the
    /// switch's last part.
    LastPart,
}

/// Definite-assignment state for scalar variables created without a value.
///
/// After branches (`if` arms, a loop body) the state is the union of the
/// state before them and each branch's state at its end. Instead of copying
/// the whole state for every branch, which made long programs with many
/// variables quadratic, changes are logged: a branch is undone afterwards and
/// what it added is applied once all branches are done ([`State::undo`],
/// [`State::apply`]).
#[derive(Debug, Default)]
struct State {
    /// Variables created without a value.
    tracked: BTreeSet<SymbolId>,
    /// Tracked variables that may have been given a value (or were reported).
    assigned: BTreeSet<SymbolId>,
    /// Every change, oldest first.
    log: Vec<Change>,
}

/// One change to a [`State`].
#[derive(Debug)]
enum Change {
    Tracked(SymbolId),
    Assigned(SymbolId),
    Unassigned(SymbolId),
}

impl State {
    /// Whether a tracked variable has certainly not been given a value yet.
    fn unassigned(&self, sym: &SymbolId) -> bool {
        self.tracked.contains(sym) && !self.assigned.contains(sym)
    }

    fn track(&mut self, sym: &SymbolId) {
        if self.tracked.insert(sym.clone()) {
            self.log.push(Change::Tracked(sym.clone()));
        }
    }

    fn assign(&mut self, sym: &SymbolId) {
        if self.assigned.insert(sym.clone()) {
            self.log.push(Change::Assigned(sym.clone()));
        }
    }

    fn unassign(&mut self, sym: &SymbolId) {
        if self.assigned.remove(sym) {
            self.log.push(Change::Unassigned(sym.clone()));
        }
    }

    /// A point to [`undo`](Self::undo) to.
    fn mark(&self) -> usize {
        self.log.len()
    }

    /// Undoes every change since `mark`, returning the variables added since
    /// then that were still there.
    fn undo(&mut self, mark: usize) -> Vec<Change> {
        let changes = self.log.split_off(mark);
        let added = changes
            .iter()
            .filter_map(|change| match change {
                Change::Tracked(sym) if self.tracked.contains(sym) => Some(Change::Tracked(sym.clone())),
                Change::Assigned(sym) if self.assigned.contains(sym) => Some(Change::Assigned(sym.clone())),
                _ => None,
            })
            .collect();
        for change in changes.into_iter().rev() {
            match change {
                Change::Tracked(sym) => {
                    self.tracked.remove(&sym);
                }
                Change::Assigned(sym) => {
                    self.assigned.remove(&sym);
                }
                Change::Unassigned(sym) => {
                    self.assigned.insert(sym);
                }
            }
        }
        added
    }

    /// Adds what branches added (from [`undo`](Self::undo)).
    fn apply(&mut self, added: Vec<Change>) {
        for change in added {
            match change {
                Change::Tracked(sym) => self.track(&sym),
                Change::Assigned(sym) => self.assign(&sym),
                Change::Unassigned(_) => {}
            }
        }
    }
}

struct Flow<'a> {
    symbols: &'a SymbolTable,
    diags: &'a mut Diags,
    /// Number of loops around the current statement.
    loop_depth: usize,
}

impl Flow<'_> {
    /// Checks a statement list; returns whether control can reach its end.
    fn list(&mut self, block: &Block, state: &mut State) -> bool {
        self.list_in(block, state, FallThrough::Never)
    }

    /// [`Self::list`] for a list where *go on into the next case* may stand
    /// as `fall_through` says.
    fn list_in(&mut self, block: &Block, state: &mut State, fall_through: FallThrough) -> bool {
        let mut completes = true;
        let last = block.stmts.len().saturating_sub(1);
        for (index, stmt) in block.stmts.iter().enumerate() {
            if matches!(stmt.kind, StmtKind::FallThrough) {
                self.check_fall_through(stmt, fall_through, index == last);
            }
            if !completes {
                self.diags.warning(
                    codes::UNREACHABLE,
                    stmt.origin.location(),
                    "This block can never run: the program never gets past the block before it, which always \
                     returns, stops the program, leaves or skips the loop, or repeats forever. Remove this block \
                     or move it.",
                );
                return false;
            }
            completes = self.stmt(stmt, state);
        }
        completes
    }

    /// Checks a statement; returns whether it can complete normally.
    #[allow(clippy::too_many_lines, reason = "one arm per statement")]
    fn stmt(&mut self, stmt: &Stmt, state: &mut State) -> bool {
        match &stmt.kind {
            StmtKind::VarDecl(decl) => {
                if let Some(init) = &decl.init {
                    self.self_reference(init, &decl.symbol);
                    self.uses(init, state);
                } else if self.symbols.get(&decl.symbol).is_some_and(|s| is_scalar(&s.ty)) {
                    state.track(&decl.symbol);
                    state.unassign(&decl.symbol);
                }
                true
            }
            StmtKind::Assign { target, value } => {
                self.uses(value, state);
                state.assign(target);
                true
            }
            StmtKind::CompoundAssign { target, value, .. } => {
                self.uses(value, state);
                self.read(target, stmt, state);
                true
            }
            StmtKind::If { branches, else_body } => self.if_else(branches, else_body.as_ref(), state),
            StmtKind::While { cond, until, body } => {
                self.enter_loop(body, state);
                self.uses(cond, state);
                self.loop_body(body, state);
                let endless = matches!(cond.kind, ExprKind::Bool(value) if value != *until);
                !endless || contains_break(body)
            }
            StmtKind::Repeat { count, body } => {
                self.uses(count, state);
                self.enter_loop(body, state);
                self.loop_body(body, state);
                true
            }
            StmtKind::ForRange {
                from, to, step, body, ..
            } => {
                for value in [Some(from), Some(to), step.as_ref()].into_iter().flatten() {
                    self.uses(value, state);
                }
                self.enter_loop(body, state);
                self.loop_body(body, state);
                true
            }
            StmtKind::Forever { body } => {
                self.enter_loop(body, state);
                self.loop_body(body, state);
                let leaves = contains_break(body);
                if !leaves && !contains_exit(body) {
                    self.diags.info(
                        codes::ENDLESS_LOOP,
                        stmt.origin.location(),
                        "This 'forever' loop never stops: there is no 'leave loop', 'return' or 'stop program' \
                         block inside it. That is fine if the program should run until it is closed.",
                    );
                }
                leaves
            }
            // Outside a loop these are errors already; treating them as jumps
            // would only add "unreachable" warnings.
            StmtKind::Break | StmtKind::Continue => self.loop_depth == 0,
            StmtKind::Return { value } => {
                if let Some(value) = value {
                    self.uses(value, state);
                }
                false
            }
            StmtKind::Exit { code, .. } => {
                if let Some(code) = code {
                    self.uses(code, state);
                }
                false
            }
            StmtKind::Eval { expr: value }
            | StmtKind::SetField { value, .. }
            | StmtKind::Decimals { digits: value }
            | StmtKind::Wait { seconds: value } => {
                self.uses(value, state);
                true
            }
            StmtKind::Print { items, .. } | StmtKind::WriteFile { items, .. } => {
                for item in items {
                    self.uses(item, state);
                }
                true
            }
            StmtKind::Ask { prompt, target, .. } => {
                if let Some(prompt) = prompt {
                    self.uses(prompt, state);
                }
                state.assign(target);
                true
            }
            StmtKind::Switch {
                value,
                cases,
                default,
            } => self.switch(value, cases, default.as_ref(), state),
            StmtKind::DoWhile { body, cond, until } => {
                // The body runs at least once, so what it assigns stays
                // assigned after the loop.
                self.enter_loop(body, state);
                self.loop_depth += 1;
                let reaches_cond = self.list(body, state) || contains_continue(body);
                self.loop_depth -= 1;
                self.uses(cond, state);
                let endless = matches!(cond.kind, ExprKind::Bool(value) if value != *until);
                (reaches_cond && !endless) || contains_break(body)
            }
            StmtKind::RangeFor { range, body, .. } => {
                self.uses(range, state);
                self.enter_loop(body, state);
                self.loop_body(body, state);
                true
            }
            StmtKind::Try { body, catches } => {
                let parts: Vec<&Block> = std::iter::once(body)
                    .chain(catches.iter().map(|c| &c.body))
                    .collect();
                self.branches(&parts, false, state)
            }
            StmtKind::Throw { message, .. } => {
                self.uses(message, state);
                false
            }
            StmtKind::Rethrow => false,
            StmtKind::FallThrough | StmtKind::StaticCheck { .. } => true,
            StmtKind::FileLines {
                path, body, failed, ..
            } => {
                self.uses(path, state);
                if let Some(failed) = failed {
                    self.branches(&[failed], true, state);
                }
                self.enter_loop(body, state);
                self.loop_body(body, state);
                true
            }
            StmtKind::OpenFile { path, failed, .. } => {
                self.uses(path, state);
                if let Some(failed) = failed {
                    self.branches(&[failed], true, state);
                }
                true
            }
            StmtKind::CollectionOp { op, .. } => {
                for value in op.operands() {
                    self.uses(value, state);
                }
                true
            }
            StmtKind::Flip { target } => {
                self.read(target, stmt, state);
                true
            }
        }
    }

    /// Parts of which one runs, or none when `may_skip`: each is checked from
    /// the state before them, and afterwards the state holds what any of
    /// them assigned. Returns whether control can get past them.
    fn branches(&mut self, parts: &[&Block], may_skip: bool, state: &mut State) -> bool {
        let mut completes = may_skip;
        let mut added = Vec::new();
        for part in parts {
            let mark = state.mark();
            completes |= self.list(part, state);
            added.extend(state.undo(mark));
        }
        state.apply(added);
        completes
    }

    /// A switch (w2.md §2.9): one part runs, or none without a default
    /// part. A part that ends with *go on into the next case* goes on into
    /// the next part, whose own end then counts.
    fn switch(
        &mut self,
        value: &Expr,
        cases: &[SwitchCase],
        default: Option<&Block>,
        state: &mut State,
    ) -> bool {
        self.uses(value, state);
        let parts: Vec<&Block> = cases.iter().map(|case| &case.body).chain(default).collect();
        let mut completes = default.is_none();
        let mut added = Vec::new();
        for (index, part) in parts.iter().enumerate() {
            let last_part = index + 1 == parts.len();
            let fall_through = if last_part {
                FallThrough::LastPart
            } else {
                FallThrough::Last
            };
            let mark = state.mark();
            let part_completes = self.list_in(part, state, fall_through);
            added.extend(state.undo(mark));
            let falls_on = matches!(part.stmts.last().map(|s| &s.kind), Some(StmtKind::FallThrough));
            completes |= part_completes && (!falls_on || last_part);
        }
        state.apply(added);
        completes
    }

    /// `B2C-E0427` for *go on into the next case* where it may not stand.
    fn check_fall_through(&mut self, stmt: &Stmt, fall_through: FallThrough, is_last: bool) {
        let message = match (fall_through, is_last) {
            (FallThrough::Last, true) => return,
            (FallThrough::LastPart, _) => {
                "'go on into the next case' can't be used in the last part of a switch: there is no next case \
                 to go on into. Remove it."
            }
            _ => {
                "'go on into the next case' must be the last block of a case, directly in the case (not \
                 inside another block). Move it to the end of the case."
            }
        };
        self.diags
            .error(codes::MISPLACED_FALLTHROUGH, stmt.origin.location(), message);
    }

    fn if_else(
        &mut self,
        branches: &[b2c_ir::sast::IfBranch],
        else_body: Option<&Block>,
        state: &mut State,
    ) -> bool {
        let mut completes = false;
        let mut added = Vec::new();
        for branch in branches {
            self.uses(&branch.cond, state);
            let mark = state.mark();
            completes |= self.list(&branch.body, state);
            added.extend(state.undo(mark));
        }
        match else_body {
            Some(body) => {
                let mark = state.mark();
                completes |= self.list(body, state);
                added.extend(state.undo(mark));
            }
            None => completes = true,
        }
        state.apply(added);
        completes
    }

    /// Assignments anywhere in a loop body may happen before any use in a
    /// later round, so they count as possibly done when the loop starts.
    fn enter_loop(&self, body: &Block, state: &mut State) {
        let mut assigned = BTreeSet::new();
        self.assignments(body, &mut assigned);
        for sym in &assigned {
            state.assign(sym);
        }
    }

    fn loop_body(&mut self, body: &Block, state: &mut State) {
        let mark = state.mark();
        self.loop_depth += 1;
        self.list(body, state);
        self.loop_depth -= 1;
        let added = state.undo(mark);
        state.apply(added);
    }

    /// Every variable a block may assign.
    #[allow(clippy::too_many_lines, reason = "one arm per statement")]
    fn assignments(&self, block: &Block, out: &mut BTreeSet<SymbolId>) {
        for stmt in &block.stmts {
            match &stmt.kind {
                StmtKind::Assign { target, value } | StmtKind::CompoundAssign { target, value, .. } => {
                    out.insert(target.clone());
                    self.editable_args(value, out);
                }
                StmtKind::Ask { target, .. } | StmtKind::Flip { target } => {
                    out.insert(target.clone());
                }
                StmtKind::If { branches, else_body } => {
                    for branch in branches {
                        self.editable_args(&branch.cond, out);
                        self.assignments(&branch.body, out);
                    }
                    if let Some(body) = else_body {
                        self.assignments(body, out);
                    }
                }
                StmtKind::While { body, .. }
                | StmtKind::Repeat { body, .. }
                | StmtKind::ForRange { body, .. }
                | StmtKind::Forever { body } => self.assignments(body, out),
                StmtKind::DoWhile { body, cond, .. } => {
                    self.assignments(body, out);
                    self.editable_args(cond, out);
                }
                StmtKind::RangeFor { range, body, .. } => {
                    self.editable_args(range, out);
                    self.assignments(body, out);
                }
                StmtKind::Switch {
                    value,
                    cases,
                    default,
                } => {
                    self.editable_args(value, out);
                    for case in cases {
                        self.assignments(&case.body, out);
                    }
                    if let Some(body) = default {
                        self.assignments(body, out);
                    }
                }
                StmtKind::Try { body, catches } => {
                    self.assignments(body, out);
                    for catch in catches {
                        self.assignments(&catch.body, out);
                    }
                }
                StmtKind::FileLines {
                    path, body, failed, ..
                } => {
                    self.editable_args(path, out);
                    self.assignments(body, out);
                    if let Some(failed) = failed {
                        self.assignments(failed, out);
                    }
                }
                StmtKind::OpenFile { path, failed, .. } => {
                    self.editable_args(path, out);
                    if let Some(failed) = failed {
                        self.assignments(failed, out);
                    }
                }
                StmtKind::Throw { message: value, .. }
                | StmtKind::SetField { value, .. }
                | StmtKind::Decimals { digits: value }
                | StmtKind::Wait { seconds: value }
                | StmtKind::StaticCheck { cond: value, .. } => self.editable_args(value, out),
                StmtKind::CollectionOp { op, .. } => {
                    for value in op.operands() {
                        self.editable_args(value, out);
                    }
                }
                StmtKind::Eval { expr } => self.editable_args(expr, out),
                StmtKind::VarDecl(decl) => {
                    if let Some(init) = &decl.init {
                        self.editable_args(init, out);
                    }
                }
                StmtKind::Print { items, .. } | StmtKind::WriteFile { items, .. } => {
                    for item in items {
                        self.editable_args(item, out);
                    }
                }
                StmtKind::Return { value: Some(value) }
                | StmtKind::Exit {
                    code: Some(value), ..
                } => {
                    self.editable_args(value, out);
                }
                StmtKind::Return { value: None }
                | StmtKind::Exit { code: None, .. }
                | StmtKind::Break
                | StmtKind::Continue
                | StmtKind::Rethrow
                | StmtKind::FallThrough => {}
            }
        }
    }

    /// Variables passed to editable parameters inside an expression, and
    /// the lines *read line from … into* reads into.
    fn editable_args(&self, expr: &Expr, out: &mut BTreeSet<SymbolId>) {
        match &expr.kind {
            ExprKind::Call { function, args } => {
                for (i, arg) in args.iter().enumerate() {
                    if let (ExprKind::Var(sym), true) = (&arg.kind, self.is_editable(function, i)) {
                        out.insert(sym.clone());
                    }
                }
            }
            ExprKind::ReadLine { line, .. } => {
                out.insert(line.clone());
            }
            _ => {}
        }
        for child in children(expr) {
            self.editable_args(child, out);
        }
    }

    /// Whether parameter `index` of `function` is editable.
    fn is_editable(&self, function: &SymbolId, index: usize) -> bool {
        let Some(SymbolKind::Function { params, .. }) = self.symbols.get(function).map(|s| &s.kind) else {
            return false;
        };
        params
            .get(index)
            .and_then(|p| self.symbols.get(p))
            .is_some_and(|p| {
                matches!(
                    p.kind,
                    SymbolKind::Parameter {
                        mode: PassMode::Editable
                    }
                )
            })
    }

    /// Checks the variables an expression reads.
    fn uses(&mut self, expr: &Expr, state: &mut State) {
        match &expr.kind {
            ExprKind::Var(sym) => self.read_at(sym, expr, state),
            ExprKind::Call { function, args } => {
                let mut assigned = Vec::new();
                for (i, arg) in args.iter().enumerate() {
                    match &arg.kind {
                        ExprKind::Var(sym) if self.is_editable(function, i) => assigned.push(sym.clone()),
                        _ => self.uses(arg, state),
                    }
                }
                for sym in &assigned {
                    state.assign(sym);
                }
            }
            ExprKind::ReadLine { line, .. } => state.assign(line),
            _ => {
                for child in children(expr) {
                    self.uses(child, state);
                }
            }
        }
    }

    /// A read of `sym` by a statement (compound assignment).
    fn read(&mut self, sym: &SymbolId, stmt: &Stmt, state: &mut State) {
        if state.unassigned(sym) {
            self.report_unassigned(sym, stmt.origin.location());
        }
        state.assign(sym);
    }

    /// A read of `sym` by an expression.
    fn read_at(&mut self, sym: &SymbolId, expr: &Expr, state: &mut State) {
        if state.unassigned(sym) {
            self.report_unassigned(sym, expr.origin.location());
            state.assign(sym);
        }
    }

    fn report_unassigned(&mut self, sym: &SymbolId, location: b2c_ir::diag::Location) {
        let name = self
            .symbols
            .get(sym)
            .map(|s| s.name.as_str().to_owned())
            .unwrap_or_default();
        let message = format!(
            "{} is used before it is given a value, so it still has its default value (0, or false). Give it a \
             starting value when you create it, or set it before this block.",
            quoted(&name)
        );
        self.diags
            .warning(codes::USE_BEFORE_ASSIGNMENT, location, message);
    }

    /// Warns about a variable used in its own starting value.
    fn self_reference(&mut self, init: &Expr, sym: &SymbolId) {
        if let ExprKind::Var(used) = &init.kind
            && used == sym
        {
            let name = self
                .symbols
                .get(sym)
                .map(|s| s.name.as_str().to_owned())
                .unwrap_or_default();
            let message = format!(
                "{} is used in its own starting value, before it has a value. Use another value here.",
                quoted(&name)
            );
            self.diags
                .warning(codes::USE_BEFORE_ASSIGNMENT, init.origin.location(), message);
            return;
        }
        for child in children(init) {
            self.self_reference(child, sym);
        }
    }
}

/// The direct sub-expressions of an expression.
pub(crate) fn children(expr: &Expr) -> Vec<&Expr> {
    expr.kind.children()
}

/// The statement lists of a statement that are not a loop's body or a
/// switch's parts: what a `break` or `continue` there jumps out of is the
/// loop around the statement.
fn plain_lists(stmt: &Stmt) -> Vec<&Block> {
    match &stmt.kind {
        StmtKind::If { branches, else_body } => {
            branches.iter().map(|b| &b.body).chain(else_body.iter()).collect()
        }
        StmtKind::Try { body, catches } => std::iter::once(body)
            .chain(catches.iter().map(|c| &c.body))
            .collect(),
        StmtKind::FileLines { failed, .. } | StmtKind::OpenFile { failed, .. } => failed.iter().collect(),
        _ => Vec::new(),
    }
}

/// Whether a loop body has a `break` that leaves this loop (not an inner
/// loop, and not from a switch, where it is `B2C-E0401`).
fn contains_break(body: &Block) -> bool {
    body.stmts.iter().any(|stmt| match &stmt.kind {
        StmtKind::Break => true,
        _ => plain_lists(stmt).into_iter().any(contains_break),
    })
}

/// Whether a loop body has a `continue` of this loop, also from a switch.
fn contains_continue(body: &Block) -> bool {
    body.stmts.iter().any(|stmt| match &stmt.kind {
        StmtKind::Continue => true,
        StmtKind::Switch { cases, default, .. } => {
            cases.iter().any(|case| contains_continue(&case.body))
                || default.as_ref().is_some_and(contains_continue)
        }
        _ => plain_lists(stmt).into_iter().any(contains_continue),
    })
}

/// Whether a block has a `return`, `stop program`, *throw* or *throw the
/// error again* anywhere inside.
fn contains_exit(body: &Block) -> bool {
    body.stmts.iter().any(|stmt| match &stmt.kind {
        StmtKind::Return { .. } | StmtKind::Exit { .. } | StmtKind::Throw { .. } | StmtKind::Rethrow => true,
        StmtKind::While { body, .. }
        | StmtKind::Repeat { body, .. }
        | StmtKind::ForRange { body, .. }
        | StmtKind::Forever { body }
        | StmtKind::DoWhile { body, .. }
        | StmtKind::RangeFor { body, .. } => contains_exit(body),
        StmtKind::FileLines { body, failed, .. } => {
            contains_exit(body) || failed.as_ref().is_some_and(contains_exit)
        }
        StmtKind::Switch { cases, default, .. } => {
            cases.iter().any(|case| contains_exit(&case.body)) || default.as_ref().is_some_and(contains_exit)
        }
        _ => plain_lists(stmt).into_iter().any(contains_exit),
    })
}
