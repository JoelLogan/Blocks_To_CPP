//! A bound on how deeply a lowered item nests.
//!
//! The analyser limits block nesting ([`crate::collect::MAX_BLOCK_DEPTH`]) and
//! the operator levels of each expression slot
//! ([`b2c_model::limits::MAX_EXPR_DEPTH`]). Some blocks still expand into
//! several levels of the SAST: an `and` block with 32 conditions is a chain
//! of 31 `&&` operators, so 64 such blocks nested in each other would make a
//! tree about 2,000 levels deep. Later stages walk the SAST recursively and
//! stop at a depth of their own (code generation at 256, where it emits error
//! placeholders), so a program the analyser accepts must stay well below
//! that. This module finds the statements that do not.
//!
//! The measure is deliberately conservative: a statement counts two levels
//! (some walks visit a statement and then its body as separate levels), and
//! each level of an expression counts one.

use b2c_ir::sast::{Block, Expr, Stmt, StmtKind};

use crate::flow::children;

/// The largest weighted nesting accepted. Valid programs stay far below it:
/// 64 levels of blocks count at most 128, and an expression slot adds at most
/// 64 levels.
pub(crate) const MAX_NESTING: usize = 224;

/// The first statement (in program order) of a body whose nesting, or the
/// nesting of its values, exceeds [`MAX_NESTING`].
pub(crate) fn too_deep(body: &Block) -> Option<&Stmt> {
    block(body, 0)
}

fn block(block: &Block, depth: usize) -> Option<&Stmt> {
    block.stmts.iter().find_map(|stmt| statement(stmt, depth + 2))
}

fn statement(stmt: &Stmt, depth: usize) -> Option<&Stmt> {
    if depth > MAX_NESTING {
        return Some(stmt);
    }
    let (exprs, bodies) = parts(stmt);
    if exprs.into_iter().any(|expr| expr_too_deep(expr, depth + 1)) {
        return Some(stmt);
    }
    bodies.into_iter().find_map(|body| block(body, depth))
}

fn expr_too_deep(expr: &Expr, depth: usize) -> bool {
    depth > MAX_NESTING
        || children(expr)
            .into_iter()
            .any(|child| expr_too_deep(child, depth + 1))
}

/// The expressions and statement lists directly inside a statement.
pub(crate) fn parts(stmt: &Stmt) -> (Vec<&Expr>, Vec<&Block>) {
    match &stmt.kind {
        StmtKind::VarDecl(decl) => (decl.init.iter().collect(), Vec::new()),
        StmtKind::Assign { value, .. } | StmtKind::CompoundAssign { value, .. } => (vec![value], Vec::new()),
        StmtKind::If { branches, else_body } => (
            branches.iter().map(|b| &b.cond).collect(),
            branches.iter().map(|b| &b.body).chain(else_body.iter()).collect(),
        ),
        StmtKind::While { cond, body, .. } => (vec![cond], vec![body]),
        StmtKind::Repeat { count, body } => (vec![count], vec![body]),
        StmtKind::ForRange {
            from, to, step, body, ..
        } => ([from, to].into_iter().chain(step.iter()).collect(), vec![body]),
        StmtKind::Forever { body } => (Vec::new(), vec![body]),
        StmtKind::Break | StmtKind::Continue => (Vec::new(), Vec::new()),
        StmtKind::Return { value } => (value.iter().collect(), Vec::new()),
        StmtKind::Exit { code, .. } => (code.iter().collect(), Vec::new()),
        StmtKind::Eval { expr } => (vec![expr], Vec::new()),
        StmtKind::Print { items, .. } => (items.iter().collect(), Vec::new()),
        StmtKind::Ask { prompt, .. } => (prompt.iter().collect(), Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use b2c_ir::ids::{BlockId, ModuleId};
    use b2c_ir::sast::{BinaryOp, ExprKind, Origin};
    use b2c_ir::text::NumLit;
    use b2c_ir::types::Type;

    use super::*;

    fn origin(block: &str) -> Origin {
        Origin::whole(ModuleId::new("m").expect("id"), BlockId::new(block).expect("id"))
    }

    fn one() -> Expr {
        Expr {
            kind: ExprKind::Int(NumLit::int(1)),
            ty: Type::Int,
            origin: origin("n"),
        }
    }

    /// `1 + 1 + …` with `operators` operators.
    fn chain(operators: usize) -> Expr {
        (0..operators).fold(one(), |lhs, _| Expr {
            kind: ExprKind::Binary {
                op: BinaryOp::Add,
                lhs: Box::new(lhs),
                rhs: Box::new(one()),
            },
            ty: Type::Int,
            origin: origin("n"),
        })
    }

    fn stmt(kind: StmtKind, id: &str) -> Stmt {
        Stmt {
            kind,
            origin: origin(id),
            comment: None,
        }
    }

    /// `forever { … }` nested `levels` times around `inner`.
    fn nested(levels: usize, inner: Stmt) -> Block {
        let mut body = Block { stmts: vec![inner] };
        for i in 0..levels {
            body = Block {
                stmts: vec![stmt(StmtKind::Forever { body }, &format!("f{i}"))],
            };
        }
        body
    }

    #[test]
    fn measures_statements_twice_and_expressions_once() {
        let eval = |operators| {
            stmt(
                StmtKind::Eval {
                    expr: chain(operators),
                },
                "e",
            )
        };
        // A top-level statement is at 2; its expression's root at 3.
        assert!(
            too_deep(&Block {
                stmts: vec![eval(MAX_NESTING - 3)]
            })
            .is_none()
        );
        let body = Block {
            stmts: vec![eval(MAX_NESTING - 2)],
        };
        assert_eq!(too_deep(&body).map(|s| s.origin.block.as_str()), Some("e"));
        // Ten levels of loops take 20 of the budget.
        assert!(too_deep(&nested(10, eval(MAX_NESTING - 23))).is_none());
        assert_eq!(
            too_deep(&nested(10, eval(MAX_NESTING - 22))).map(|s| s.origin.block.as_str()),
            Some("e")
        );
        // Statements alone.
        let deep = nested(MAX_NESTING / 2, stmt(StmtKind::Break, "b"));
        assert!(too_deep(&deep).is_some());
        let fine = nested(MAX_NESTING / 2 - 2, stmt(StmtKind::Break, "b"));
        assert!(too_deep(&fine).is_none());
    }

    #[test]
    fn valid_programs_fit() {
        // The deepest program the other limits allow: 64 levels of blocks
        // (counted twice) and an expression slot with 64 levels.
        let eval = stmt(
            StmtKind::Eval {
                expr: chain(b2c_model::limits::MAX_EXPR_DEPTH),
            },
            "e",
        );
        assert!(too_deep(&nested(crate::collect::MAX_BLOCK_DEPTH - 1, eval)).is_none());
    }
}
