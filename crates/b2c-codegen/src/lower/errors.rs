//! Desugaring W2's error nodes (`docs/plans/m3-contracts/w2.md` §1.7, §2.5):
//! package B.
//!
//! *try … catch* is `try { … } catch (const E& e) { … } …`, each catch
//! variable printed also when no block reads it (g++ does not warn about an
//! unused one, even at the strict level); *throw* is `throw E(message);`
//! and *throw the error again* `throw;`. *message of* is `e.what()`, a
//! `const char*` that the expressions around it treat as quoted text
//! (types.md §11.1). The error types' headers come with their types.

use b2c_ir::sast::{Expr, ExprKind, StmtKind};
use b2c_ir::types::Type;

use super::{Lowerer, name_expr};
use crate::cast::{CCatch, CExpr, CStmtKind};

impl Lowerer<'_> {
    /// *try … catch*, *throw* and *throw the error again*.
    pub(super) fn error_stmt(&mut self, kind: &StmtKind) -> CStmtKind {
        match kind {
            StmtKind::Try { body, catches } => {
                let body = self.block(body);
                let catches = catches
                    .iter()
                    .filter_map(|catch| {
                        let name = self.variable(&catch.var)?.name.clone();
                        let ty = self.ctype(&catch.error);
                        self.note_type(&ty);
                        let body = self.scoped_block(&catch.body, vec![name.clone()]);
                        Some(CCatch {
                            ty,
                            name,
                            body,
                            origin: Some(catch.origin.clone()),
                        })
                    })
                    .collect();
                CStmtKind::Try { body, catches }
            }
            StmtKind::Throw { error, message } => {
                let Type::Lib(app) = error else {
                    return CStmtKind::Error;
                };
                let ty = self.ctype(error);
                self.note_type(&ty);
                let message = self.expr(message);
                CStmtKind::Throw(Some(CExpr::call_fixed(app.ty().cpp_name(), vec![message])))
            }
            StmtKind::Rethrow => CStmtKind::Throw(None),
            _ => CStmtKind::Error,
        }
    }

    /// *message of* (`ExprKind::ErrorMessage`): `e.what()`.
    pub(super) fn error_expr(&mut self, expr: &Expr) -> CExpr {
        let ExprKind::ErrorMessage(error) = &expr.kind else {
            return CExpr::error();
        };
        match self.variable(error) {
            Some(symbol) => CExpr::method(name_expr(symbol.name.clone()), "what", Vec::new()),
            None => CExpr::error(),
        }
    }
}

#[cfg(test)]
mod tests {
    //! *try … catch*, *throw*, *throw the error again* and *message of*,
    //! printed from hand-built SAST, with the headers they record.

    use b2c_ir::sast::{Block, Catch, CppStandard, Stmt};
    use b2c_ir::types::LibType;

    use super::super::text::tests::{expr, headers, lowerer, program, text, var_id};
    use super::*;
    use crate::emit;

    fn error(ty: LibType) -> Type {
        Type::lib(ty, Vec::new()).unwrap()
    }

    fn stmt(kind: StmtKind) -> Stmt {
        let origin = text("x").origin;
        Stmt {
            kind,
            origin,
            comment: None,
        }
    }

    fn message_of(name: &str) -> Expr {
        expr(ExprKind::ErrorMessage(var_id(name)), Type::String)
    }

    #[test]
    fn try_catch_throw_and_message_print() {
        let program = program(
            CppStandard::Cpp20,
            &[
                ("e", error(LibType::InvalidArgument)),
                ("other", error(LibType::Exception)),
            ],
        );
        let catch = |error_type: LibType, name: &str, body: Vec<Stmt>| Catch {
            error: error(error_type),
            var: var_id(name),
            body: Block { stmts: body },
            origin: text("x").origin,
        };
        let joined = expr(ExprKind::Join(vec![text("Oh: "), message_of("e")]), Type::String);
        let print = |value: Expr| {
            stmt(StmtKind::Print {
                items: vec![value],
                separator: b2c_ir::sast::PrintSeparator::None,
                newline: true,
                stream: b2c_ir::sast::OutputStream::Out,
            })
        };
        let kind = StmtKind::Try {
            body: Block {
                stmts: vec![stmt(StmtKind::Throw {
                    error: error(LibType::InvalidArgument),
                    message: text("bad"),
                })],
            },
            catches: vec![
                catch(
                    LibType::InvalidArgument,
                    "e",
                    vec![print(joined), stmt(StmtKind::Rethrow)],
                ),
                catch(LibType::Exception, "other", vec![]),
            ],
        };
        let mut generator = lowerer(&program);
        let cpp = generator.error_stmt(&kind);
        assert_eq!(
            emit::stmt_text(&cpp),
            "try {\n    throw std::invalid_argument(\"bad\");\n} catch (const std::invalid_argument& e) {\n    \
             std::cout << std::string(\"Oh: \") + e.what() << '\\n';\n    throw;\n} catch (const std::exception& \
             other) {\n}\n"
        );
        assert_eq!(
            generator.item_headers.body,
            headers(&["<exception>", "<iostream>", "<stdexcept>", "<string>"])
        );
        // The message is used as it is where text is written, and turned
        // into a `std::string` where it is joined first.
        let mut generator = lowerer(&program);
        let first = expr(ExprKind::Join(vec![message_of("e"), text("!")]), Type::String);
        let cpp = generator.expr(&first);
        assert_eq!(emit::expr_text(&cpp).trim_end(), r#"std::string(e.what()) + "!""#);
        let cpp = generator.expr(&message_of("other"));
        assert_eq!(emit::expr_text(&cpp).trim_end(), "other.what()");
    }
}
