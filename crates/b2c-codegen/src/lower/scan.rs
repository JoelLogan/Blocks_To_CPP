//! Read-only walks over the SAST used while desugaring: which symbols a piece
//! of code mentions, reads or may change, and choosing fresh readable names.
//!
//! Every node of W2 has its arm (w2.md §2.13), so a name a W2 block declares
//! counts when a fresh name is chosen, a call keeps a value from being
//! hoisted out of a loop, and a change to a variable counts as a write. The
//! hidden names of W2's forms (`b2c_file`) are not symbols: they start with
//! `b2c`, which no user name may, and are chosen against the names already
//! in scope, as *repeat*'s counter is.

use std::collections::BTreeSet;

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{Block, Expr, ExprKind, PassMode, RangeMode, Stmt, StmtKind, SymbolKind, SymbolTable};
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
        StmtKind::Break
        | StmtKind::Continue
        | StmtKind::Rethrow
        | StmtKind::FallThrough
        | StmtKind::Flip { .. } => true,
        StmtKind::Return { value } => walk_opt(value.as_ref(), visitor, next),
        StmtKind::Exit { code, .. } => walk_opt(code.as_ref(), visitor, next),
        StmtKind::Eval { expr } => walk_expr(expr, visitor, next),
        StmtKind::Print { items, .. } | StmtKind::WriteFile { items, .. } => {
            items.iter().all(|e| walk_expr(e, visitor, next))
        }
        StmtKind::Ask { prompt, .. } => walk_opt(prompt.as_ref(), visitor, next),
        StmtKind::Switch {
            value,
            cases,
            default,
        } => {
            walk_expr(value, visitor, next)
                && cases.iter().all(|case| {
                    walk_expr(&case.label, visitor, next) && walk_block(&case.body, visitor, next)
                })
                && default
                    .as_ref()
                    .is_none_or(|body| walk_block(body, visitor, next))
        }
        StmtKind::DoWhile { body, cond, .. } => {
            walk_block(body, visitor, next) && walk_expr(cond, visitor, next)
        }
        StmtKind::RangeFor { range, body, .. } => {
            walk_expr(range, visitor, next) && walk_block(body, visitor, next)
        }
        StmtKind::Try { body, catches } => {
            walk_block(body, visitor, next) && catches.iter().all(|c| walk_block(&c.body, visitor, next))
        }
        StmtKind::Throw { message: value, .. }
        | StmtKind::SetField { value, .. }
        | StmtKind::Decimals { digits: value }
        | StmtKind::Wait { seconds: value }
        | StmtKind::StaticCheck { cond: value, .. } => walk_expr(value, visitor, next),
        StmtKind::FileLines {
            path, body, failed, ..
        } => {
            walk_expr(path, visitor, next)
                && failed.as_ref().is_none_or(|f| walk_block(f, visitor, next))
                && walk_block(body, visitor, next)
        }
        StmtKind::OpenFile { path, failed, .. } => {
            walk_expr(path, visitor, next) && failed.as_ref().is_none_or(|f| walk_block(f, visitor, next))
        }
        StmtKind::CollectionOp { op, .. } => op.operands().into_iter().all(|e| walk_expr(e, visitor, next)),
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
        ExprKind::Item { .. }
        | ExprKind::Size(_)
        | ExprKind::IsEmpty(_)
        | ExprKind::Contains { .. }
        | ExprKind::Next(_)
        | ExprKind::ListOf(_)
        | ExprKind::Field { .. }
        | ExprKind::New { .. }
        | ExprKind::Enumerator(_)
        | ExprKind::Letter { .. }
        | ExprKind::TextLength(_)
        | ExprKind::TextCall { .. }
        | ExprKind::ToNumber { .. }
        | ExprKind::AsText(_)
        | ExprKind::ErrorMessage(_)
        | ExprKind::MathCall { .. }
        | ExprKind::RandomReal { .. }
        | ExprKind::ReadLine { .. }
        | ExprKind::FileExists(_) => expr
            .kind
            .children()
            .into_iter()
            .all(|e| walk_expr(e, visitor, next)),
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
            | StmtKind::ForRange { var: target, .. }
            | StmtKind::FileLines { line: target, .. }
            | StmtKind::OpenFile { file: target, .. }
            | StmtKind::WriteFile { file: target, .. }
            | StmtKind::CollectionOp {
                collection: target, ..
            }
            | StmtKind::SetField { target, .. }
            | StmtKind::Flip { target } => {
                self.0.insert(target.clone());
            }
            StmtKind::RangeFor { vars, .. } => self.0.extend(vars.iter().cloned()),
            StmtKind::Try { catches, .. } => self.0.extend(catches.iter().map(|catch| catch.var.clone())),
            _ => {}
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Var(id) | ExprKind::Call { function: id, .. } | ExprKind::ErrorMessage(id) => {
                self.0.insert(id.clone());
            }
            ExprKind::ReadLine { file, line } => {
                self.0.insert(file.clone());
                self.0.insert(line.clone());
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

/// Whether `block` declares or refers to any of the symbols `ids`: for a
/// range-`for` whose body never uses its names, which then get
/// `[[maybe_unused]]` (w2.md §2.12). A walk that is cut off counts as a
/// use, so the attribute is only added when it is certainly needed.
pub(super) fn mentions_any(block: &Block, ids: &[SymbolId]) -> bool {
    let mut mentions = Mentions::default();
    !walk_block(block, &mut mentions, 0) || ids.iter().any(|id| mentions.0.contains(id))
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
            ExprKind::Var(id) | ExprKind::ErrorMessage(id) => {
                self.vars.insert(id.clone());
            }
            // A different value each time, or a value that changes what it
            // reads (W2: a random decimal, a line read from a file, text read
            // as a number, which may throw).
            ExprKind::Call { .. }
            | ExprKind::RandomInt { .. }
            | ExprKind::RandomReal { .. }
            | ExprKind::ReadLine { .. }
            | ExprKind::ToNumber { .. } => self.calls = true,
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
            | StmtKind::ForRange { var: target, .. }
            | StmtKind::SetField { target, .. }
            | StmtKind::Flip { target }
            | StmtKind::OpenFile { file: target, .. }
            | StmtKind::WriteFile { file: target, .. } => {
                self.vars.insert(target.clone());
            }
            // Every change to a collection, also one that keeps its size
            // (*set item*, *sort*), changes what reads of it give.
            StmtKind::CollectionOp { collection, .. } => {
                self.vars.insert(collection.clone());
            }
            // An editable *for each* item changes the collection's items.
            StmtKind::RangeFor {
                range,
                mode: RangeMode::Editable,
                ..
            } => {
                if let ExprKind::Var(id) = &range.kind {
                    self.vars.insert(id.clone());
                }
            }
            _ => {}
        }
    }

    fn expr(&mut self, expr: &Expr) {
        match &expr.kind {
            ExprKind::Call { function, args } => {
                for (index, arg) in args.iter().enumerate() {
                    if let ExprKind::Var(id) = &arg.kind
                        && self.is_editable_arg(function, index)
                    {
                        self.vars.insert(id.clone());
                    }
                }
            }
            // *read line* changes the line and moves on in the file.
            ExprKind::ReadLine { file, line } => {
                self.vars.insert(file.clone());
                self.vars.insert(line.clone());
            }
            _ => {}
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

    // ---- W2's nodes (w2.md §2.13) ----------------------------------------

    use b2c_ir::ids::{BlockId, ModuleId};
    use b2c_ir::sast::{CollectionChange, Origin, Symbol};
    use b2c_ir::text::{NumLit, NumType};
    use b2c_ir::types::{LibType, Type};

    fn origin() -> Origin {
        Origin::whole(ModuleId::new("mod_main").unwrap(), BlockId::new("blk").unwrap())
    }

    fn id(text: &str) -> SymbolId {
        SymbolId::new(text).unwrap()
    }

    fn symbols(names: &[(&str, &str, Type)]) -> SymbolTable {
        let mut table = SymbolTable::default();
        for (symbol, name, ty) in names {
            table.symbols.insert(
                id(symbol),
                Symbol {
                    name: Ident::new(name).unwrap(),
                    kind: SymbolKind::Variable { is_const: false },
                    ty: ty.clone(),
                    origin: origin(),
                },
            );
        }
        table
    }

    fn expr(kind: ExprKind, ty: Type) -> Expr {
        Expr {
            kind,
            ty,
            origin: origin(),
        }
    }

    fn var(symbol: &str, ty: Type) -> Expr {
        expr(ExprKind::Var(id(symbol)), ty)
    }

    fn int(value: i32) -> Expr {
        expr(ExprKind::Int(NumLit::int(value)), Type::Int)
    }

    fn real(text: &str) -> Expr {
        expr(
            ExprKind::Float(NumLit::parse(text, NumType::Double).unwrap()),
            Type::Double,
        )
    }

    fn stmt(kind: StmtKind) -> Stmt {
        Stmt {
            kind,
            origin: origin(),
            comment: None,
        }
    }

    fn list() -> Type {
        Type::lib(LibType::Vector, vec![Type::Int]).unwrap()
    }

    /// A *for each* whose item is `i`, inside a *repeat*: the counter the
    /// repeat chooses must not be `i`.
    #[test]
    fn a_for_each_item_is_a_name_mentioned() {
        let table = symbols(&[("s_items", "items", list()), ("s_i", "i", Type::Int)]);
        let body = Block {
            stmts: vec![stmt(StmtKind::RangeFor {
                vars: vec![id("s_i")],
                range: var("s_items", list()),
                mode: RangeMode::ReadOnly,
                body: Block::default(),
            })],
        };
        let names = names_mentioned(&table, &body, &[]);
        assert!(names.contains("i"), "{names:?}");
        assert_eq!(fresh_name(&["i", "j", "k"], "i", &names).as_str(), "j");
    }

    /// The size of a list the body adds to changes as the loop runs; the size
    /// of a list the body only reads does not.
    #[test]
    fn a_size_of_a_changed_list_is_not_loop_invariant() {
        let table = symbols(&[("s_items", "items", list()), ("s_total", "total", Type::Int)]);
        let size = expr(ExprKind::Size(Box::new(var("s_items", list()))), Type::Int);
        let adds = Block {
            stmts: vec![stmt(StmtKind::CollectionOp {
                collection: id("s_items"),
                op: CollectionChange::Add(int(1)),
            })],
        };
        assert!(!is_loop_invariant(&table, &size, &adds));
        let reads = Block {
            stmts: vec![stmt(StmtKind::Assign {
                target: id("s_total"),
                value: size.clone(),
            })],
        };
        assert!(is_loop_invariant(&table, &size, &reads));
        // An editable *for each* over the list changes its items too.
        let edits = Block {
            stmts: vec![stmt(StmtKind::RangeFor {
                vars: Vec::new(),
                range: var("s_items", list()),
                mode: RangeMode::Editable,
                body: Block::default(),
            })],
        };
        let first = expr(
            ExprKind::Item {
                collection: Box::new(var("s_items", list())),
                index: Box::new(int(0)),
            },
            Type::Int,
        );
        assert!(!is_loop_invariant(&table, &first, &edits));
    }

    /// A random decimal is different each time.
    #[test]
    fn a_random_decimal_is_not_loop_invariant() {
        let table = symbols(&[]);
        let random = expr(
            ExprKind::RandomReal {
                low: Box::new(real("1.5")),
                high: Box::new(real("2.5")),
            },
            Type::Double,
        );
        assert!(!is_loop_invariant(&table, &random, &Block::default()));
        assert!(is_loop_invariant(&table, &real("1.5"), &Block::default()));
    }
}
