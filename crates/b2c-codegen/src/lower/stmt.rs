//! Desugaring of statements (spec §3.7, "Generates" columns).

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{
    AskMode, Block, CompoundOp, Expr, ExprKind, IfBranch, OutputStream, PrintSeparator, RangeDirection, Stmt,
    StmtKind, VarDecl,
};
use b2c_ir::types::Type;

use super::scan::{self, MAX_DEPTH};
use super::{Lowerer, int_literal, is_scalar, name_expr};
use crate::cast::{AssignOp, BinOp, CExpr, CStmt, CStmtKind, CType, ForDecl, ForStep, IncOp, Init};
use crate::helpers::Helper;

/// Whether an expression is the integer literal 1.
fn is_literal_one(expr: &Expr) -> bool {
    matches!(&expr.kind, ExprKind::Int(lit) if lit.as_str() == "1")
}

/// The C++ assignment operator for a compound operator.
fn assign_op(op: CompoundOp) -> AssignOp {
    match op {
        CompoundOp::Add => AssignOp::Add,
        CompoundOp::Sub => AssignOp::Sub,
        CompoundOp::Mul => AssignOp::Mul,
        CompoundOp::Div => AssignOp::Div,
        CompoundOp::Mod => AssignOp::Mod,
    }
}

/// The helper call that reads a value of a scalar type.
fn ask_function(ty: &Type) -> Option<&'static str> {
    Some(match ty {
        Type::Int => "b2c::ask<int>",
        Type::Double => "b2c::ask<double>",
        Type::Char => "b2c::ask<char>",
        Type::Bool => "b2c::ask<bool>",
        _ => return None,
    })
}

impl Lowerer<'_> {
    /// Desugars a statement, keeping its origin and comment.
    pub(super) fn stmt(&mut self, stmt: &Stmt) -> CStmt {
        let kind = if self.depth >= MAX_DEPTH {
            CStmtKind::Error
        } else {
            self.depth += 1;
            let kind = self.stmt_kind(&stmt.kind);
            self.depth -= 1;
            kind
        };
        CStmt {
            kind,
            origin: Some(stmt.origin.clone()),
            comment: stmt.comment.clone(),
        }
    }

    /// Desugars the statement itself.
    fn stmt_kind(&mut self, kind: &StmtKind) -> CStmtKind {
        match kind {
            StmtKind::VarDecl(decl) => self.var_decl(decl),
            StmtKind::Assign { target, value } => match self.variable(target) {
                Some(symbol) => CStmtKind::Assign {
                    target: symbol.name.clone(),
                    op: AssignOp::Set,
                    value: self.expr(value),
                },
                None => CStmtKind::Error,
            },
            StmtKind::CompoundAssign { target, op, value } => self.compound_assign(target, *op, value),
            StmtKind::If { branches, else_body } => self.if_stmt(branches, else_body.as_ref()),
            StmtKind::While { cond, until, body } => {
                let cond = if *until {
                    self.negated_condition(cond)
                } else {
                    self.expr(cond)
                };
                CStmtKind::While {
                    cond,
                    body: self.block(body),
                }
            }
            StmtKind::Repeat { count, body } => self.repeat(count, body),
            StmtKind::ForRange {
                var,
                from,
                to,
                step,
                direction,
                body,
            } => self.for_range(var, from, to, step.as_ref(), *direction, body),
            StmtKind::Forever { body } => CStmtKind::While {
                cond: CExpr::fixed("true"),
                body: self.block(body),
            },
            StmtKind::Break => CStmtKind::Break,
            StmtKind::Continue => CStmtKind::Continue,
            StmtKind::Return { value } => {
                let value = match value {
                    Some(value) => Some(self.expr(value)),
                    // `return;` is not allowed in `int main()`.
                    None if self.in_main => Some(int_literal(0)),
                    None => None,
                };
                CStmtKind::Return(value)
            }
            StmtKind::Exit { code, in_main } => self.exit(code.as_ref(), *in_main),
            StmtKind::Eval { expr } => CStmtKind::Expr(self.expr(expr)),
            StmtKind::Print {
                items,
                separator,
                newline,
                stream,
            } => self.print(items, *separator, *newline, *stream),
            StmtKind::Ask { prompt, target, mode } => self.ask(prompt.as_ref(), target, *mode),
        }
    }

    /// `int score = 0;`, `const auto x = …;` or `int x{};`.
    fn var_decl(&mut self, decl: &VarDecl) -> CStmtKind {
        let Some(symbol) = self.variable(&decl.symbol) else {
            return CStmtKind::Error;
        };
        let declared = CType::value(&symbol.ty);
        self.note_type(declared);
        let init = decl.init.as_ref().map(|value| {
            let lowered = self.expr(value);
            // `auto s = "text";` would make a `const char*`; keep it a string.
            if declared == CType::String && decl.written_auto {
                self.std_string_of(value, lowered)
            } else {
                lowered
            }
        });
        let (ty, init) = match init {
            Some(value) if decl.written_auto || declared == CType::Error => (CType::Auto, Init::Value(value)),
            Some(value) => (declared, Init::Value(value)),
            None if declared == CType::Error => (CType::Auto, Init::Value(CExpr::error())),
            None => (declared, Init::Braces),
        };
        // The name is visible from here on (C++ scope starts at the declarator).
        self.declare(symbol.name.clone());
        CStmtKind::Decl {
            is_const: decl.is_const,
            ty,
            name: symbol.name.clone(),
            init,
        }
    }

    /// `x += v;`, or `++x;` for `change x by 1`.
    fn compound_assign(&mut self, target: &SymbolId, op: CompoundOp, value: &Expr) -> CStmtKind {
        let Some(symbol) = self.variable(target) else {
            return CStmtKind::Error;
        };
        let numeric = matches!(symbol.ty, Type::Int | Type::Double | Type::Char);
        if op == CompoundOp::Add && numeric && is_literal_one(value) {
            return CStmtKind::Inc {
                target: symbol.name.clone(),
                op: IncOp::Inc,
            };
        }
        CStmtKind::Assign {
            target: symbol.name.clone(),
            op: assign_op(op),
            value: self.expr(value),
        }
    }

    /// `if (…) { … } else if (…) { … } else { … }`.
    fn if_stmt(&mut self, branches: &[IfBranch], else_body: Option<&Block>) -> CStmtKind {
        let branches: Vec<_> = branches
            .iter()
            .map(|b| (self.expr(&b.cond), self.block(&b.body)))
            .collect();
        let else_body = else_body.map(|body| self.block(body));
        if branches.is_empty() {
            return else_body.map_or(CStmtKind::Error, CStmtKind::Block);
        }
        CStmtKind::If { branches, else_body }
    }

    /// `for (int i = 0; i < n; ++i) { … }` with a fresh counter name.
    fn repeat(&mut self, count: &Expr, body: &Block) -> CStmtKind {
        // The counter must not hide anything the count or the body uses, and
        // must not clash with what the body declares or anything visible here.
        let mut taken = self.visible_names();
        taken.extend(scan::names_mentioned(self.symbols, body, &[Some(count)]));
        let counter = scan::fresh_name(&["i", "j", "k"], "i", &taken);
        taken.insert(counter.as_str().to_owned());
        let limit = self.expr(count);
        let mut decls = vec![ForDecl {
            name: counter.clone(),
            value: int_literal(0),
        }];
        let limit = if scan::is_loop_invariant(self.symbols, count, body) {
            limit
        } else {
            let name = scan::fresh_name(&["n"], "n", &taken);
            decls.push(ForDecl {
                name: name.clone(),
                value: limit,
            });
            name_expr(name)
        };
        let cond = CExpr::binary(BinOp::Lt, name_expr(counter.clone()), limit);
        let body = self.scoped_block(body, decls.iter().map(|d| d.name.clone()).collect());
        CStmtKind::For {
            ty: CType::Int,
            decls,
            cond,
            step: ForStep::Inc(IncOp::Inc, counter),
            body,
        }
    }

    /// `for (int i = from; i < to; ++i) { … }` and its variants.
    fn for_range(
        &mut self,
        var: &SymbolId,
        from: &Expr,
        to: &Expr,
        step: Option<&Expr>,
        direction: RangeDirection,
        body: &Block,
    ) -> CStmtKind {
        let Some(symbol) = self.variable(var) else {
            return CStmtKind::Error;
        };
        let var = symbol.name.clone();
        let ty = match CType::value(&symbol.ty) {
            ty @ (CType::Int | CType::Double | CType::Char) => ty,
            _ => CType::Int,
        };
        let mut taken = self.visible_names();
        taken.extend(scan::names_mentioned(
            self.symbols,
            body,
            &[Some(from), Some(to), step],
        ));
        taken.insert(var.as_str().to_owned());
        let mut decls = vec![ForDecl {
            name: var.clone(),
            value: self.expr(from),
        }];
        // Bounds that may change while the loop runs are evaluated once.
        let mut bound = |this: &mut Self, expr: &Expr, stem: &'static str| {
            let lowered = this.expr(expr);
            if scan::is_loop_invariant(this.symbols, expr, body) {
                return lowered;
            }
            let name = scan::fresh_name(&[stem], stem, &taken);
            taken.insert(name.as_str().to_owned());
            decls.push(ForDecl {
                name: name.clone(),
                value: lowered,
            });
            name_expr(name)
        };
        let limit = bound(self, to, "end");
        let step = step
            .filter(|s| !is_literal_one(s))
            .map(|s| bound(self, s, "step"));
        let (compare, inc, assign) = match direction {
            RangeDirection::UpExclusive => (BinOp::Lt, IncOp::Inc, AssignOp::Add),
            RangeDirection::UpInclusive => (BinOp::Le, IncOp::Inc, AssignOp::Add),
            RangeDirection::DownInclusive => (BinOp::Ge, IncOp::Dec, AssignOp::Sub),
        };
        let cond = CExpr::binary(compare, name_expr(var.clone()), limit);
        let step = match step {
            Some(amount) => ForStep::Assign(assign, var, amount),
            None => ForStep::Inc(inc, var),
        };
        let body = self.scoped_block(body, decls.iter().map(|d| d.name.clone()).collect());
        CStmtKind::For {
            ty,
            decls,
            cond,
            step,
            body,
        }
    }

    /// `return code;` in `main`, `std::exit(code);` elsewhere.
    fn exit(&mut self, code: Option<&Expr>, in_main: bool) -> CStmtKind {
        let code = code.map_or_else(|| int_literal(0), |code| self.expr(code));
        // `return` only ends the program in `main` itself; the analyser's flag
        // and the generator's own position must agree.
        if in_main && self.in_main {
            return CStmtKind::Return(Some(code));
        }
        self.include("<cstdlib>");
        CStmtKind::Expr(CExpr::call_fixed("std::exit", vec![code]))
    }

    /// `std::cout << a << " " << b << '\n';`
    fn print(
        &mut self,
        items: &[Expr],
        separator: PrintSeparator,
        newline: bool,
        stream: OutputStream,
    ) -> CStmtKind {
        if items.is_empty() && !newline {
            return CStmtKind::Error;
        }
        self.include("<iostream>");
        let mut chain = CExpr::fixed(match stream {
            OutputStream::Out => "std::cout",
            OutputStream::Err => "std::cerr",
        });
        let separator = match separator {
            PrintSeparator::None => None,
            PrintSeparator::Space => Some("\" \""),
            PrintSeparator::Comma => Some("\", \""),
        };
        for (index, item) in items.iter().enumerate() {
            if let Some(separator) = separator.filter(|_| index > 0) {
                chain = CExpr::binary(BinOp::Shl, chain, CExpr::fixed(separator));
            }
            let operand = self.print_operand(item);
            chain = CExpr::binary(BinOp::Shl, chain, operand);
        }
        if newline {
            chain = CExpr::binary(BinOp::Shl, chain, CExpr::fixed("'\\n'"));
        }
        CStmtKind::Expr(chain)
    }

    /// `ask (prompt) and save answer in [target]` (spec §3.7.6).
    fn ask(&mut self, prompt: Option<&Expr>, target: &SymbolId, mode: AskMode) -> CStmtKind {
        let Some(symbol) = self.variable(target) else {
            return CStmtKind::Error;
        };
        let name = symbol.name.clone();
        match mode {
            AskMode::KeepAsking => {
                let (function, helper) = if symbol.ty == Type::String {
                    ("b2c::ask_line", Helper::AskLine)
                } else if let Some(function) = ask_function(&symbol.ty) {
                    (function, Helper::Ask)
                } else {
                    return CStmtKind::Error;
                };
                self.use_helper(helper);
                let args = prompt.map(|p| self.prompt_arg(p)).into_iter().collect();
                CStmtKind::Assign {
                    target: name,
                    op: AssignOp::Set,
                    value: CExpr::call_fixed(function, args),
                }
            }
            AskMode::Simple => {
                let read = if symbol.ty == Type::String {
                    // `std::ws` skips the line break a previous `>>` left behind.
                    self.include("<string>");
                    let stream = CExpr::binary(BinOp::Shr, CExpr::fixed("std::cin"), CExpr::fixed("std::ws"));
                    CExpr::call_fixed("std::getline", vec![stream, name_expr(name)])
                } else if is_scalar(&symbol.ty) {
                    CExpr::binary(BinOp::Shr, CExpr::fixed("std::cin"), name_expr(name))
                } else {
                    return CStmtKind::Error;
                };
                self.include("<iostream>");
                let read = CStmtKind::Expr(read);
                match prompt {
                    None => read,
                    Some(prompt) => {
                        let prompt = self.print_operand(prompt);
                        let show = CExpr::binary(BinOp::Shl, CExpr::fixed("std::cout"), prompt);
                        CStmtKind::Seq(vec![CStmt::new(CStmtKind::Expr(show)), CStmt::new(read)])
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_functions_cover_scalars_only() {
        assert_eq!(ask_function(&Type::Int), Some("b2c::ask<int>"));
        assert_eq!(ask_function(&Type::Bool), Some("b2c::ask<bool>"));
        assert_eq!(ask_function(&Type::String), None);
        assert_eq!(ask_function(&Type::Error), None);
        assert_eq!(assign_op(CompoundOp::Mod).token(), "%=");
    }
}
