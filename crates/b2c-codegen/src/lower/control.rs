//! Desugaring W2's control nodes (`docs/plans/m3-contracts/w2.md` §1.6, §2.9):
//! package D.
//!
//! * *switch on* is `switch (v) { case 1: { … break; } default: { … } }`:
//!   each case is a `{ }` scope that ends with a `break;`, left out when its
//!   last statement already leaves it (*return*, *stop program*, *throw*,
//!   *throw the error again*, *skip to next round*) or goes on into the next
//!   case. A switch on an enum without a default part that misses some of
//!   its values gets `default: { break; }`, so g++'s `-Wswitch` never fires
//!   (the analyser warns, `B2C-W0508`).
//! * *do … then repeat* is `do { … } while (c);`, with *until* inverting the
//!   condition as *repeat until* does.
//! * *wait … seconds* is
//!   `std::this_thread::sleep_for(std::chrono::duration<double>(s));` and
//!   *go on into the next case* `[[fallthrough]];`.

use b2c_ir::sast::{Block, Expr, ExprKind, StmtKind, SwitchCase, SymbolKind};
use b2c_ir::types::Type;

use super::Lowerer;
use crate::cast::{CBlock, CCase, CExpr, CStmt, CStmtKind};

/// Whether a case's statements end in one that leaves the case, so no
/// `break;` follows it (w2.md §2.9).
fn ends_case(body: &Block) -> bool {
    matches!(
        body.stmts.last().map(|stmt| &stmt.kind),
        Some(
            StmtKind::Return { .. }
                | StmtKind::Exit { .. }
                | StmtKind::Throw { .. }
                | StmtKind::Rethrow
                | StmtKind::Continue
                | StmtKind::FallThrough
        )
    )
}

impl Lowerer<'_> {
    /// *switch on*, *do … then repeat*, *wait* and *go on into the next
    /// case*.
    pub(super) fn control_stmt(&mut self, kind: &StmtKind) -> CStmtKind {
        match kind {
            StmtKind::Switch {
                value,
                cases,
                default,
            } => self.switch(value, cases, default.as_ref()),
            StmtKind::DoWhile { body, cond, until } => {
                let body = self.block(body);
                let cond = if *until {
                    self.negated_condition(cond)
                } else {
                    self.condition(cond)
                };
                CStmtKind::DoWhile { body, cond }
            }
            StmtKind::FallThrough => CStmtKind::FallThrough,
            // `<chrono>` and `<thread>` are the block's catalog headers.
            StmtKind::Wait { seconds } => {
                let seconds = self.expr(seconds);
                let duration = CExpr::call_fixed("std::chrono::duration<double>", vec![seconds]);
                CStmtKind::Expr(CExpr::call_fixed("std::this_thread::sleep_for", vec![duration]))
            }
            _ => CStmtKind::Error,
        }
    }

    /// `switch (v) { case …: { … } … default: { … } }`.
    fn switch(&mut self, value: &Expr, cases: &[SwitchCase], default: Option<&Block>) -> CStmtKind {
        let switched = self.expr(value);
        let adds_default = default.is_none() && self.misses_enum_values(value, cases);
        let cases: Vec<CCase> = cases
            .iter()
            .map(|case| {
                let label = self.expr(&case.label);
                let mut body = self.block(&case.body);
                if !ends_case(&case.body) {
                    body.stmts.push(CStmt::new(CStmtKind::Break));
                }
                CCase {
                    label,
                    body: vec![CStmt::new(CStmtKind::Block(body))],
                }
            })
            .collect();
        let default = match default {
            Some(body) => Some(self.block(body)),
            None if adds_default => Some(CBlock {
                stmts: vec![CStmt::new(CStmtKind::Break)],
            }),
            None => None,
        };
        CStmtKind::Switch {
            value: switched,
            cases,
            default: default.map(|body| vec![CStmt::new(CStmtKind::Block(body))]),
        }
    }

    /// Whether `value` is of an enum some of whose values no case names.
    fn misses_enum_values(&self, value: &Expr, cases: &[SwitchCase]) -> bool {
        let Type::Named(owner) = &value.ty else {
            return false;
        };
        let Some(SymbolKind::Enum { values }) = self.symbols.get(owner).map(|symbol| &symbol.kind) else {
            return false;
        };
        values.iter().any(|enumerator| {
            !cases
                .iter()
                .any(|case| matches!(&case.label.kind, ExprKind::Enumerator(sym) if sym == enumerator))
        })
    }
}

#[cfg(test)]
mod tests {
    //! *switch on* and *do … then repeat*, printed from hand-built SAST.

    use b2c_ir::ids::SymbolId;
    use b2c_ir::sast::{BinaryOp, CppStandard, OutputStream, PrintSeparator, Stmt, Symbol, UnaryOp};
    use b2c_ir::text::Ident;
    use b2c_ir::types::LibType;

    use super::super::text::tests::{expr, int, lowerer, program, text, var_id};
    use super::*;
    use crate::emit;

    fn stmt(kind: StmtKind) -> Stmt {
        Stmt {
            kind,
            origin: text("x").origin,
            comment: None,
        }
    }

    fn block(stmts: Vec<StmtKind>) -> Block {
        Block {
            stmts: stmts.into_iter().map(stmt).collect(),
        }
    }

    fn var(name: &str, ty: Type) -> Expr {
        expr(ExprKind::Var(var_id(name)), ty)
    }

    fn print(value: Expr) -> StmtKind {
        StmtKind::Print {
            items: vec![value],
            separator: PrintSeparator::None,
            newline: true,
            stream: OutputStream::Out,
        }
    }

    fn case(label: Expr, body: Vec<StmtKind>) -> SwitchCase {
        SwitchCase {
            label,
            body: block(body),
        }
    }

    #[test]
    fn each_case_is_a_scope_that_ends_with_a_break_unless_it_leaves() {
        let program = program(CppStandard::Cpp20, &[("n", Type::Int)]);
        let kind = StmtKind::Switch {
            value: var("n", Type::Int),
            cases: vec![
                case(int("1"), vec![print(text("one"))]),
                case(int("2"), Vec::new()),
                case(
                    int("3"),
                    vec![StmtKind::Return {
                        value: Some(int("3")),
                    }],
                ),
                case(int("4"), vec![print(text("four")), StmtKind::FallThrough]),
                case(
                    int("5"),
                    vec![StmtKind::Exit {
                        code: None,
                        in_main: false,
                    }],
                ),
                case(int("6"), vec![StmtKind::Continue]),
                case(
                    int("7"),
                    vec![StmtKind::Throw {
                        error: Type::lib(LibType::RuntimeError, Vec::new()).unwrap(),
                        message: text("bad"),
                    }],
                ),
            ],
            default: Some(block(vec![print(text("other"))])),
        };
        let cpp = lowerer(&program).control_stmt(&kind);
        let expected = r#"switch (n) {
    case 1: {
        std::cout << "one" << '\n';
        break;
    }
    case 2: {
        break;
    }
    case 3: {
        return 3;
    }
    case 4: {
        std::cout << "four" << '\n';
        [[fallthrough]];
    }
    case 5: {
        std::exit(0);
    }
    case 6: {
        continue;
    }
    case 7: {
        throw std::runtime_error("bad");
    }
    default: {
        std::cout << "other" << '\n';
    }
}
"#;
        assert_eq!(emit::stmt_text(&cpp), expected);
        // Without a default part, a switch on a whole number has none.
        let kind = StmtKind::Switch {
            value: var("n", Type::Int),
            cases: vec![case(int("1"), Vec::new())],
            default: None,
        };
        let cpp = lowerer(&program).control_stmt(&kind);
        assert_eq!(
            emit::stmt_text(&cpp),
            "switch (n) {\n    case 1: {\n        break;\n    }\n}\n"
        );
    }

    #[test]
    fn a_switch_on_an_enum_that_misses_values_gets_a_default() {
        let mut program = program(CppStandard::Cpp20, &[]);
        let id = |name: &str| SymbolId::new(&format!("sym_{name}")).unwrap();
        let enum_id = id("Action");
        let action_type = Type::Named(enum_id.clone());
        let symbol = |name: &str, kind: SymbolKind| Symbol {
            name: Ident::new(name).unwrap(),
            kind,
            ty: action_type.clone(),
            origin: text("x").origin,
        };
        let values = vec![id("Add"), id("Quit")];
        let symbols = &mut program.symbols.symbols;
        symbols.insert(
            enum_id.clone(),
            symbol(
                "Action",
                SymbolKind::Enum {
                    values: values.clone(),
                },
            ),
        );
        for (value, name) in values.iter().zip(["Add", "Quit"]) {
            symbols.insert(
                value.clone(),
                symbol(
                    name,
                    SymbolKind::Enumerator {
                        owner: enum_id.clone(),
                    },
                ),
            );
        }
        symbols.insert(
            id("action"),
            symbol("action", SymbolKind::Variable { is_const: false }),
        );
        let value = |name: &str| expr(ExprKind::Enumerator(id(name)), action_type.clone());
        let default_of = |cases: Vec<SwitchCase>| {
            let kind = StmtKind::Switch {
                value: var("action", action_type.clone()),
                cases,
                default: None,
            };
            match lowerer(&program).control_stmt(&kind) {
                CStmtKind::Switch { default, .. } => default.map(|part| emit::stmt_text(&part[0].kind)),
                other => panic!("not a switch: {other:?}"),
            }
        };
        assert_eq!(
            default_of(vec![case(value("Add"), Vec::new())]).as_deref(),
            Some("{\n    break;\n}\n")
        );
        assert_eq!(
            default_of(vec![
                case(value("Add"), Vec::new()),
                case(value("Quit"), Vec::new())
            ]),
            None
        );
        // The labels are the enum's own values, written in full.
        let kind = StmtKind::Switch {
            value: var("action", action_type.clone()),
            cases: vec![case(value("Quit"), vec![print(text("bye"))])],
            default: None,
        };
        assert_eq!(
            emit::stmt_text(&lowerer(&program).control_stmt(&kind)),
            "switch (action) {\n    case Action::Quit: {\n        std::cout << \"bye\" << '\\n';\n        \
             break;\n    }\n    default: {\n        break;\n    }\n}\n"
        );
    }

    #[test]
    fn do_while_prints_its_condition_after_the_body() {
        let program = program(CppStandard::Cpp20, &[("n", Type::Int), ("done", Type::Bool)]);
        let body = || block(vec![print(var("n", Type::Int))]);
        let below = expr(
            ExprKind::Binary {
                op: BinaryOp::Lt,
                lhs: Box::new(var("n", Type::Int)),
                rhs: Box::new(int("3")),
            },
            Type::Bool,
        );
        let printed = |cond: Expr, until: bool, body: Block| {
            let kind = StmtKind::DoWhile { body, cond, until };
            emit::stmt_text(&lowerer(&program).control_stmt(&kind))
        };
        assert_eq!(
            printed(below.clone(), false, body()),
            "do {\n    std::cout << n << '\\n';\n} while (n < 3);\n"
        );
        // *until* inverts the condition as *repeat until* does.
        assert_eq!(
            printed(below, true, body()),
            "do {\n    std::cout << n << '\\n';\n} while (n >= 3);\n"
        );
        let not_done = expr(
            ExprKind::Unary {
                op: UnaryOp::Not,
                operand: Box::new(var("done", Type::Bool)),
            },
            Type::Bool,
        );
        assert_eq!(
            printed(not_done, true, block(Vec::new())),
            "do {\n} while (done);\n"
        );
    }
}
