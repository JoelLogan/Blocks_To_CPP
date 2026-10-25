//! Lowering the error blocks of W2 (`docs/plans/m3-contracts/w2.md` §1.7,
//! §2.5): package B.
//!
//! * *try … catch* lowers its `TRY` part, then each catch part: its error
//!   type (`ERROR<i>`), its variable (`NAME<i>`, a constant of that type,
//!   declared by the block's typing from `field:ERROR`) visible in that part
//!   only, and its statements, lowered through [`Lowerer::in_catch`]. A part
//!   that an earlier part hides is `B2C-W0507`.
//! * *throw … with message* and *throw the error again* end their path (the
//!   flow checks know them); the latter reports through
//!   [`Lowerer::check_rethrow`] outside a catch part (`B2C-E0402`).
//! * *message of* reads a caught error: `e.what()`.

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{Catch, Expr, ExprKind, Origin, StmtKind, Symbol, SymbolKind};
use b2c_ir::sig::PartType;
use b2c_ir::types::{LibType, Type};
use b2c_model::Block;

use super::convert::Subject;
use super::expr::error_expr;
use super::{Lowerer, field};
use crate::access;
use crate::codes;
use crate::scope::ListKey;

/// The pseudo-list around a catch part's statements that holds its
/// variable: C++ treats the variable and the part's outermost statements
/// as one scope, so the part cannot declare the name again.
const CATCH_LIST: &str = "#catch";

/// An error type of the standard library.
fn error_type(ty: LibType) -> Type {
    Type::lib(ty, Vec::new()).unwrap_or(Type::Error)
}

/// The errors a catch part can catch, by their catalog names.
fn catchable() -> [(&'static str, Type); 4] {
    [
        ("std::exception", error_type(LibType::Exception)),
        ("std::invalid_argument", error_type(LibType::InvalidArgument)),
        ("std::out_of_range", error_type(LibType::OutOfRange)),
        ("std::runtime_error", error_type(LibType::RuntimeError)),
    ]
}

/// The errors *throw* can throw: `std::exception` takes no message.
fn throwable() -> [(&'static str, Type); 3] {
    [
        ("std::runtime_error", error_type(LibType::RuntimeError)),
        ("std::invalid_argument", error_type(LibType::InvalidArgument)),
        ("std::out_of_range", error_type(LibType::OutOfRange)),
    ]
}

/// What an error is called in a message: "out-of-range error".
fn error_noun(ty: &Type) -> &'static str {
    match ty {
        Type::Lib(app) => match app.ty() {
            LibType::RuntimeError => "runtime error",
            LibType::InvalidArgument => "invalid argument error",
            LibType::OutOfRange => "out-of-range error",
            _ => "error",
        },
        _ => "error",
    }
}

/// Whether a catch part for `earlier` catches every error a later part for
/// `later` would (w2.md §2.5): the same type, or `std::exception`, the base
/// of the others. `std::invalid_argument` and `std::out_of_range` both
/// derive from `std::logic_error`, not from each other, and
/// `std::runtime_error` from neither.
fn hides(earlier: &Type, later: &Type) -> bool {
    earlier == later || *earlier == error_type(LibType::Exception)
}

impl Lowerer<'_> {
    /// *try … catch*, *throw*, *throw the error again*.
    pub(super) fn error_statement(&mut self, block: &Block) -> Option<StmtKind> {
        match block.block_type.as_str() {
            "error.try" => Some(self.try_catch(block)),
            "error.throw" => {
                let error = self.choice(block, "ERROR", error_type(LibType::RuntimeError), &throwable());
                let [message] = self.typed_values(
                    block,
                    [(
                        "MESSAGE",
                        Subject::Phrase {
                            text: String::from("The message of 'throw'"),
                            stores: false,
                        },
                    )],
                );
                Some(StmtKind::Throw { error, message })
            }
            "error.rethrow" => {
                self.check_rethrow(block);
                Some(StmtKind::Rethrow)
            }
            _ => None,
        }
    }

    /// *message of*: the message of a caught error, `e.what()`.
    pub(super) fn error_value(&mut self, block: &Block, origin: Origin) -> Expr {
        let location = self.loc(&block.id, field("ERROR"));
        let Ok(sym) = access::ref_field(block, "ERROR") else {
            self.incomplete(location, "Choose the error of a catch part in this block.");
            return error_expr(origin);
        };
        let Some(ty) = self.read_variable(sym, &location) else {
            return error_expr(origin);
        };
        // `where = { X = "exception" }`, `X` on `ERROR`: only a caught error
        // has a message.
        let mut typed = self.typed(self.block_sig(&block.block_type));
        typed.field("ERROR", PartType::Known(ty));
        let typing = self.type_parts(block, &mut typed, None);
        self.report_field_fit(block, &typed, &typing, "ERROR");
        let ty = typing.output.unwrap_or(Type::String);
        Expr {
            kind: ExprKind::ErrorMessage(sym.clone()),
            ty,
            origin,
        }
    }

    /// *try … catch*: the `TRY` part, then each catch part with its error
    /// type and variable.
    fn try_catch(&mut self, block: &Block) -> StmtKind {
        let body = self.body(block, "TRY", false);
        let count = access::catch_count(block);
        // `NAME` is `field:ERROR` (w2.md §2.1): each part's variable has its
        // part's error type, which the typing declares as `NAME<i>`.
        let mut typed = self.typed(self.block_sig(&block.block_type));
        typed.count("ERROR", count);
        typed.count("NAME", count);
        let mut errors = Vec::with_capacity(count);
        for i in 0..count {
            let name = format!("ERROR{i}");
            let error = self.choice(block, &name, error_type(LibType::Exception), &catchable());
            typed.field(&name, PartType::Known(error.clone()));
            errors.push(error);
        }
        let typing = self.type_parts(block, &mut typed, None);
        let mut catches = Vec::with_capacity(count);
        for (i, error) in errors.iter().enumerate() {
            self.report_hidden_catch(block, &errors, i);
            let declared = typing
                .declares
                .get(&format!("NAME{i}"))
                .filter(|ty| **ty != Type::Error)
                .cloned()
                .unwrap_or_else(|| error.clone());
            let origin = self.origin(&block.id, field(&format!("ERROR{i}")));
            if let Some((var, body)) = self.catch_part(block, i, declared) {
                catches.push(Catch {
                    error: error.clone(),
                    var,
                    body,
                    origin,
                });
            }
        }
        StmtKind::Try { body, catches }
    }

    /// One catch part: its variable, of type `ty`, declared in a scope of
    /// its own around the part's statements. Without a variable (a damaged
    /// block, reported), the statements are still lowered, so their own
    /// problems are reported, and the part is left out.
    fn catch_part(
        &mut self,
        block: &Block,
        index: usize,
        ty: Type,
    ) -> Option<(SymbolId, b2c_ir::sast::Block)> {
        let name_field = format!("NAME{index}");
        let list = format!("CATCH{index}");
        let location = self.loc(&block.id, field(&name_field));
        self.scopes.push(ListKey::new(&block.id, CATCH_LIST), false);
        let declared = if let Ok(decl) = access::decl_field(block, &name_field) {
            let ident = self.ident(&decl.name, &location, false);
            self.check_new_name(&decl.name, &location);
            self.warn_if_function_name(&decl.name, &location);
            let symbol = Symbol {
                name: ident,
                kind: SymbolKind::Variable { is_const: true },
                ty,
                origin: self.origin(&block.id, field(&name_field)),
            };
            self.insert_symbol(&decl.sym, &decl.name, symbol, location)
                .then(|| {
                    self.scopes.declare(&decl.name, &decl.sym);
                    // Visible in its own part only, not after the block.
                    self.index.record_part_variable(&decl.sym);
                    decl.sym.clone()
                })
        } else {
            self.incomplete(location, "This catch part needs a name for the error it catches.");
            None
        };
        // The part's statements are joined to the variable's scope.
        let body = self.in_catch(|lowerer| lowerer.body(block, &list, true));
        self.scopes.pop();
        declared.map(|var| (var, body))
    }

    /// `B2C-W0507`: catch part `index` never runs when an earlier part
    /// catches the same error or `std::exception` (w2.md §2.5); g++ warns
    /// about it too (`-Wexceptions`).
    fn report_hidden_catch(&mut self, block: &Block, errors: &[Type], index: usize) {
        let Some(later) = errors.get(index).filter(|ty| **ty != Type::Error) else {
            return;
        };
        let Some(earlier_index) = errors
            .iter()
            .take(index)
            .position(|earlier| *earlier != Type::Error && hides(earlier, later))
        else {
            return;
        };
        let earlier = &errors[earlier_index];
        let noun = error_noun(later);
        let message = if earlier == later {
            format!(
                "This part never runs: a part before it already catches every {noun}. Remove one of the two, or \
                 choose another error here."
            )
        } else {
            format!(
                "This part never runs: a part before it catches every error, so every {noun} too. Move this \
                 part above that one."
            )
        };
        let location = self.loc(&block.id, field(&format!("ERROR{index}")));
        let related = self.loc(&block.id, field(&format!("ERROR{earlier_index}")));
        let diagnostic = b2c_ir::Diagnostic::warning(
            codes::UNREACHABLE_CATCH,
            b2c_ir::DiagSource::Analyser,
            location,
            message,
        )
        .with_related(related, "the part that catches it first");
        self.diags.push(diagnostic);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn earlier_parts_hide_the_same_error_and_its_derived_ones() {
        let exception = error_type(LibType::Exception);
        let runtime = error_type(LibType::RuntimeError);
        let invalid = error_type(LibType::InvalidArgument);
        let range = error_type(LibType::OutOfRange);
        for error in [&exception, &runtime, &invalid, &range] {
            assert!(hides(error, error));
            assert!(hides(&exception, error));
        }
        // No other pair: the order of input_validation's three parts is
        // free.
        for (a, b) in [
            (&invalid, &range),
            (&range, &invalid),
            (&runtime, &invalid),
            (&invalid, &runtime),
        ] {
            assert!(!hides(a, b));
        }
        assert!(!hides(&runtime, &exception));
        assert_eq!(error_noun(&range), "out-of-range error");
        assert_eq!(error_noun(&exception), "error");
    }
}
