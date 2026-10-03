//! Read-only walks over the SAST used while desugaring: which symbols a piece
//! of code mentions, reads or may change, and choosing fresh readable names.

use std::collections::BTreeSet;

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{Block, Expr, ExprKind, PassMode, Stmt, StmtKind, SymbolKind, SymbolTable};
use b2c_ir::text::Ident;

/// Maximum nesting of statements and expressions the generator descends into.
/// Valid projects stay far below it (the project file's JSON depth is limited
/// to 128); deeper input is cut off with error placeholders instead of risking
/// a stack overflow.
pub(super) const MAX_DEPTH: usize = 256;

/// Receives every statement and expression of a walk, in pre-order.
trait Visitor {
    /// Called for each statement.
    fn stmt(&mut self, _stmt: &Stmt) {}
    /// Called for each expression.
    fn expr(&mut self, _expr: &Expr) {}
}

/// Walks a block. Returns false if the walk was cut off at [`MAX_DEPTH`].
fn walk_block(block: &Block, visitor: &mut impl Visitor, depth: usize) -> bool {
    block.stmts.iter().all(|stmt| walk_stmt(stmt, visitor, depth + 1))
}

/// Walks an optional expression.
fn walk_opt(expr: Option<&Expr>, visitor: &mut impl Visitor, depth: usize) -> bool {
    expr.is_none_or(|e| walk_expr(e, visitor, depth))
}

/// Walks a statement and everything inside it.
fn walk_stmt(stmt: &Stmt, visitor: &mut impl Visitor, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    visitor.stmt(stmt);
    let next = depth + 1;
    match &stmt.kind {
        StmtKind::VarDecl(decl) => walk_opt(decl.init.as_ref(), visitor, next),
        StmtKind::Assign { value, .. } | StmtKind::CompoundAssign { value, .. } => {
            walk_expr(value, visitor, next)
        }
        StmtKind::If { branches, else_body } => {
            branches
                .iter()
                .all(|b| walk_expr(&b.cond, visitor, next) && walk_block(&b.body, visitor, next))
                && else_body.as_ref().is_none_or(|b| walk_block(b, visitor, next))
        }
        StmtKind::While { cond, body, .. } => {
            walk_expr(cond, visitor, next) && walk_block(body, visitor, next)
        }
        StmtKind::Repeat { count, body } => {
            walk_expr(count, visitor, next) && walk_block(body, visitor, next)
        }
        StmtKind::ForRange {
            from, to, step, body, ..
        } => {
            walk_expr(from, visitor, next)
                && walk_expr(to, visitor, next)
                && walk_opt(step.as_ref(), visitor, next)
                && walk_block(body, visitor, next)
        }
        StmtKind::Forever { body } => walk_block(body, visitor, next),
        StmtKind::Break | StmtKind::Continue => true,
        StmtKind::Return { value } => walk_opt(value.as_ref(), visitor, next),
        StmtKind::Exit { code, .. } => walk_opt(code.as_ref(), visitor, next),
        StmtKind::Eval { expr } => walk_expr(expr, visitor, next),
        StmtKind::Print { items, .. } => items.iter().all(|e| walk_expr(e, visitor, next)),
        StmtKind::Ask { prompt, .. } => walk_opt(prompt.as_ref(), visitor, next),
    }
}

/// Walks an expression and its operands.
fn walk_expr(expr: &Expr, visitor: &mut impl Visitor, depth: usize) -> bool {
    if depth > MAX_DEPTH {
        return false;
    }
    visitor.expr(expr);
    let next = depth + 1;
    match &expr.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Bool(_)
        | ExprKind::Str(_)
        | ExprKind::Char(_)
        | ExprKind::Var(_) => true,
        ExprKind::Unary { operand, .. } => walk_expr(operand, visitor, next),
        ExprKind::Binary { lhs, rhs, .. } | ExprKind::RandomInt { low: lhs, high: rhs } => {
            walk_expr(lhs, visitor, next) && walk_expr(rhs, visitor, next)
        }
        ExprKind::Conditional {
            cond,
            then_value,
            else_value,
        } => {
            walk_expr(cond, visitor, next)
                && walk_expr(then_value, visitor, next)
                && walk_expr(else_value, visitor, next)
        }
        ExprKind::Call { args: items, .. } | ExprKind::Join(items) => {
            items.iter().all(|e| walk_expr(e, visitor, next))
        }
        ExprKind::Convert { value, .. } => walk_expr(value, visitor, next),
    }
}

/// Collects every symbol that is declared or referred to.
#[derive(Default)]
struct Mentions(BTreeSet<SymbolId>);

impl Visitor for Mentions {
    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::VarDecl(decl) => {
                self.0.insert(decl.symbol.clone());
            }
            StmtKind::Assign { target, .. }
            | StmtKind::CompoundAssign { target, .. }
            | StmtKind::Ask { target, .. }
            | StmtKind::ForRange { var: target, .. } => {
                self.0.insert(target.clone());
            }
            _ => {}
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Var(id) | ExprKind::Call { function: id, .. } => {
                self.0.insert(id.clone());
            }
            _ => {}
        }
    }
}

/// The names of every symbol declared or referred to in `block` and `exprs`.
/// If the walk is cut off, the names found so far are returned (the code is
/// replaced by error placeholders at that depth anyway).
pub(super) fn names_mentioned(
    symbols: &SymbolTable,
    block: &Block,
    exprs: &[Option<&Expr>],
) -> BTreeSet<String> {
    let mut mentions = Mentions::default();
    walk_block(block, &mut mentions, 0);
    for expr in exprs.iter().flatten() {
        walk_expr(expr, &mut mentions, 0);
    }
    mentions
        .0
        .iter()
        .filter_map(|id| symbols.get(id))
        .map(|symbol| symbol.name.as_str().to_owned())
        .collect()
}

/// Collects the variables an expression reads, and whether it calls anything.
#[derive(Default)]
struct Reads {
    vars: BTreeSet<SymbolId>,
    calls: bool,
}

impl Visitor for Reads {
    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Var(id) => {
                self.vars.insert(id.clone());
            }
            ExprKind::Call { .. } | ExprKind::RandomInt { .. } => self.calls = true,
            _ => {}
        }
    }
}

/// Collects the variables a block may change.
struct Writes<'a> {
    symbols: &'a SymbolTable,
    vars: BTreeSet<SymbolId>,
}

impl Writes<'_> {
    /// Whether argument `index` of a call to `function` is passed by editable
    /// reference. Unknown functions count as editable, to stay on the safe side.
    fn is_editable_arg(&self, function: &SymbolId, index: usize) -> bool {
        let Some(SymbolKind::Function { params }) = self.symbols.get(function).map(|s| &s.kind) else {
            return true;
        };
        match params
            .get(index)
            .and_then(|p| self.symbols.get(p))
            .map(|s| &s.kind)
        {
            Some(SymbolKind::Parameter { mode }) => *mode == PassMode::Editable,
            _ => true,
        }
    }
}

impl Visitor for Writes<'_> {
    fn stmt(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Assign { target, .. }
            | StmtKind::CompoundAssign { target, .. }
            | StmtKind::Ask { target, .. }
            | StmtKind::ForRange { var: target, .. } => {
                self.vars.insert(target.clone());
            }
            _ => {}
        }
    }

    fn expr(&mut self, expr: &Expr) {
        if let ExprKind::Call { function, args } = &expr.kind {
            for (index, arg) in args.iter().enumerate() {
                if let ExprKind::Var(id) = &arg.kind
                    && self.is_editable_arg(function, index)
                {
                    self.vars.insert(id.clone());
                }
            }
        }
    }
}

/// Whether `expr` has the same value every time `body` repeats: it calls no
/// function (calls may have effects or return random numbers) and reads no
/// variable that `body` may change. Loop bounds that are not invariant are
/// evaluated once, before the loop, so that `repeat (n) times` runs exactly
/// `n` times even if the body changes `n`.
pub(super) fn is_loop_invariant(symbols: &SymbolTable, expr: &Expr, body: &Block) -> bool {
    let mut reads = Reads::default();
    if !walk_expr(expr, &mut reads, 0) || reads.calls {
        return false;
    }
    if reads.vars.is_empty() {
        return true;
    }
    let mut writes = Writes {
        symbols,
        vars: BTreeSet::new(),
    };
    walk_block(body, &mut writes, 0) && reads.vars.is_disjoint(&writes.vars)
}

/// Picks a readable name that is not in `taken`: the first free name of
/// `first`, otherwise `stem2`, `stem3`, …
pub(super) fn fresh_name(first: &[&'static str], stem: &'static str, taken: &BTreeSet<String>) -> Ident {
    let free = |name: &str| {
        if taken.contains(name) {
            None
        } else {
            Ident::new(name).ok()
        }
    };
    if let Some(ident) = first.iter().find_map(|name| free(name)) {
        return ident;
    }
    // At most `taken.len()` candidates are taken, so this finds a name long
    // before the bound; the bound only guarantees termination.
    let limit = u32::try_from(taken.len()).unwrap_or(u32::MAX).saturating_add(64);
    if let Some(ident) = (2..=limit).find_map(|n| free(&format!("{stem}{n}"))) {
        return ident;
    }
    Ident::generated("b2c_name").unwrap_or_else(|_| Ident::main())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn taken(names: &[&str]) -> BTreeSet<String> {
        names.iter().map(|n| (*n).to_owned()).collect()
    }

    #[test]
    fn fresh_names_follow_the_readable_sequence() {
        let first = ["i", "j", "k"];
        assert_eq!(fresh_name(&first, "i", &taken(&[])).as_str(), "i");
        assert_eq!(fresh_name(&first, "i", &taken(&["i"])).as_str(), "j");
        assert_eq!(fresh_name(&first, "i", &taken(&["i", "j"])).as_str(), "k");
        assert_eq!(fresh_name(&first, "i", &taken(&["i", "j", "k"])).as_str(), "i2");
        assert_eq!(
            fresh_name(&first, "i", &taken(&["i", "j", "k", "i2", "i3"])).as_str(),
            "i4"
        );
    }

    #[test]
    fn fresh_names_terminate_for_large_taken_sets() {
        let mut names = taken(&["n"]);
        names.extend((2..500).map(|n| format!("n{n}")));
        assert_eq!(fresh_name(&["n"], "n", &names).as_str(), "n500");
    }
}
