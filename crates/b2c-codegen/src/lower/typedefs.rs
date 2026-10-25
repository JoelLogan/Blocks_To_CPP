//! User types' definitions with the functions their ticks and uses promise
//! (types.md §1.6 and §5.2): the comparisons of a comparable struct, the
//! `operator<<` of a printable struct, and the `operator<<` and `to_string`
//! of an enum that is printed, written or joined.
//!
//! The SAST has no struct or enum item before the definition blocks of
//! milestone M3's second wave, whose lowering calls these between
//! `Lowerer::start_item` and `Lowerer::finish_item` with the symbols the
//! blocks declare. They record their headers in the item's declaration
//! part: a type's definition goes where its declaration is printed, the
//! module's header when it is shared (ADR-0019).
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "called by the lowering of W2's definition blocks")
)]

use std::collections::BTreeSet;

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{CppStandard, SymbolKind};
use b2c_ir::text::{Ident, StrLit};

use super::{Lowerer, name_expr, scan};
use crate::cast::{
    BinOp, CBlock, CCase, CEnum, CExpr, CExprKind, CField, CGenerated, CStmt, CStmtKind, CStruct, CType,
    Generated, Init,
};

/// The six comparisons a comparable struct gets in C++17, in the order they
/// are printed. With only `==` and `<`, `!=`, `>` and `<=` do not compile
/// (types.md §1.6).
const COMPARISONS: [BinOp; 6] = [BinOp::Eq, BinOp::Ne, BinOp::Lt, BinOp::Gt, BinOp::Le, BinOp::Ge];

/// A string literal of generator-composed text around validated names.
/// Names never hold a character a literal refuses, so the text is always
/// accepted; anything else is dropped rather than printed unencoded.
fn text(value: &str) -> Option<CExpr> {
    StrLit::new(value).ok().map(|lit| CExpr::new(CExprKind::Str(lit)))
}

/// `return value;` as a block's only statement.
fn returning(value: CExpr) -> CBlock {
    CBlock {
        stmts: vec![CStmt::new(CStmtKind::Return(Some(value)))],
    }
}

impl Lowerer<'_> {
    /// Every name in the program, which a generated function's parameters
    /// avoid: none of them then hides a field or a global (`-Wshadow`).
    fn program_names(&self) -> BTreeSet<String> {
        self.symbols
            .symbols
            .values()
            .map(|symbol| symbol.name.as_str().to_owned())
            .collect()
    }

    /// `struct S { T field{}; … };` for the struct symbol `id`, with the
    /// operators its effective ticks promise (types.md §1.6, §5.2): in C++20
    /// the defaulted `<=>` (`<compare>`), in C++17 six hidden friends on
    /// `std::tie` (`<tuple>`), and for a printable struct a hidden-friend
    /// `operator<<` (`<ostream>`). Hidden friends are defined in the class,
    /// so they are inline and may sit in a header that several files
    /// include. `None` when `id` is not a struct.
    pub(super) fn struct_def(&mut self, id: &SymbolId) -> Option<CStruct> {
        let symbol = self.symbol(id)?;
        let SymbolKind::Struct {
            fields,
            comparable,
            printable,
        } = &symbol.kind
        else {
            return None;
        };
        let symbols = self.symbols;
        let mut lowered = Vec::with_capacity(fields.len());
        for field in fields.iter().filter_map(|field| symbols.get(field)) {
            let ty = self.ctype(&field.ty);
            self.note_type(&ty);
            lowered.push(CField {
                ty,
                name: field.name.clone(),
                init: Init::Braces,
                origin: field.origin.clone(),
            });
        }
        let names = self.program_names();
        let mut defaulted_compare = false;
        let mut friends = Vec::new();
        if *comparable {
            if self.standard >= CppStandard::Cpp20 {
                self.include("<compare>");
                defaulted_compare = true;
            } else {
                friends.extend(self.comparisons(&lowered, &names));
            }
        }
        if *printable {
            friends.push(self.print_struct(&symbol.name, &lowered, &names));
        }
        Some(CStruct {
            name: symbol.name.clone(),
            fields: lowered,
            defaulted_compare,
            friends,
            origin: symbol.origin.clone(),
            comment: None,
        })
    }

    /// The six C++17 comparisons of a struct with `fields`, each comparing
    /// the `std::tie` of both sides' fields with its own operator. Without
    /// fields every pair of values is equal, so each returns a constant and
    /// leaves its parameters unnamed.
    fn comparisons(&mut self, fields: &[CField], names: &BTreeSet<String>) -> Vec<CGenerated> {
        let lhs = scan::fresh_name(&["lhs"], "lhs", names);
        let rhs = scan::fresh_name(&["rhs"], "rhs", names);
        if !fields.is_empty() {
            self.include("<tuple>");
        }
        let tie = |side: &Ident| {
            let args = fields
                .iter()
                .map(|field| CExpr::field(name_expr(side.clone()), field.name.clone()))
                .collect();
            CExpr::call_fixed("std::tie", args)
        };
        COMPARISONS
            .into_iter()
            .map(|op| {
                let (params, result) = if fields.is_empty() {
                    let equal = matches!(op, BinOp::Eq | BinOp::Le | BinOp::Ge);
                    (
                        vec![None, None],
                        CExpr::fixed(if equal { "true" } else { "false" }),
                    )
                } else {
                    let result = CExpr::binary(op, tie(&lhs), tie(&rhs));
                    (vec![Some(lhs.clone()), Some(rhs.clone())], result)
                };
                CGenerated {
                    kind: Generated::Compare(op),
                    params,
                    body: returning(result),
                }
            })
            .collect()
    }

    /// The hidden-friend `operator<<` of a printable struct: it writes
    /// `Point{x: 1, y: 2}`, each field as *print* writes it (`bool` as
    /// `true` or `false`, a nested struct or an enum with its own
    /// `operator<<`), then returns the stream.
    fn print_struct(&mut self, name: &Ident, fields: &[CField], names: &BTreeSet<String>) -> CGenerated {
        self.include("<ostream>");
        let out = scan::fresh_name(&["out"], "out", names);
        let value = scan::fresh_name(&["value"], "value", names);
        let mut chain = name_expr(out.clone());
        let mut opening = format!("{}{{", name.as_str());
        for (index, field) in fields.iter().enumerate() {
            let separator = if index == 0 { "" } else { ", " };
            let label = format!("{opening}{separator}{}: ", field.name.as_str());
            opening.clear();
            if let Some(label) = text(&label) {
                chain = CExpr::binary(BinOp::Shl, chain, label);
            }
            let mut item = CExpr::field(name_expr(value.clone()), field.name.clone());
            if field.ty == CType::Bool {
                item = CExpr::conditional(item, CExpr::fixed("\"true\""), CExpr::fixed("\"false\""));
            }
            chain = CExpr::binary(BinOp::Shl, chain, item);
        }
        if let Some(closing) = text(&format!("{opening}}}")) {
            chain = CExpr::binary(BinOp::Shl, chain, closing);
        }
        let value = (!fields.is_empty()).then_some(value);
        CGenerated {
            kind: Generated::PrintStruct,
            params: vec![Some(out.clone()), value],
            body: CBlock {
                stmts: vec![
                    CStmt::new(CStmtKind::Expr(chain)),
                    CStmt::new(CStmtKind::Return(Some(name_expr(out)))),
                ],
            },
        }
    }

    /// `enum class E { A, B };` for the enum symbol `id`, with `operator<<`
    /// when the enum is printed or written (`<ostream>`; `std::cout <<
    /// Color::Red` does not compile for an `enum class`) and `to_string`
    /// when it is joined (`<string>`), each writing an enumerator's name.
    /// `None` when `id` is not an enum.
    pub(super) fn enum_def(&mut self, id: &SymbolId, print: bool, to_string: bool) -> Option<CEnum> {
        let symbol = self.symbol(id)?;
        let SymbolKind::Enum { values } = &symbol.kind else {
            return None;
        };
        let values: Vec<Ident> = values
            .iter()
            .filter_map(|value| self.symbol(value))
            .map(|value| value.name.clone())
            .collect();
        let names = self.program_names();
        let mut functions = Vec::new();
        if print {
            self.include("<ostream>");
            let out = scan::fresh_name(&["out"], "out", &names);
            functions.push(enum_function(
                Generated::PrintEnum,
                &symbol.name,
                &values,
                Some(out),
                &names,
            ));
        }
        if to_string {
            self.include("<string>");
            functions.push(enum_function(
                Generated::EnumToString,
                &symbol.name,
                &values,
                None,
                &names,
            ));
        }
        Some(CEnum {
            name: symbol.name.clone(),
            values,
            functions,
            origin: symbol.origin.clone(),
            comment: None,
        })
    }
}

/// An enum's `operator<<` (with the stream `out`) or `to_string`: a `switch`
/// over the enumerators that returns each one's name, then a return for any
/// other value, which keeps `-Wreturn-type` quiet.
fn enum_function(
    kind: Generated,
    name: &Ident,
    values: &[Ident],
    out: Option<Ident>,
    names: &BTreeSet<String>,
) -> CGenerated {
    let value = scan::fresh_name(&["value"], "value", names);
    let result = |label: Option<CExpr>| match (&out, label) {
        (Some(out), Some(label)) => CExpr::binary(BinOp::Shl, name_expr(out.clone()), label),
        (Some(out), None) => name_expr(out.clone()),
        (None, label) => label.unwrap_or_else(|| CExpr::fixed("\"\"")),
    };
    let cases: Vec<CCase> = values
        .iter()
        .map(|enumerator| CCase {
            label: CExpr::new(CExprKind::Enumerator {
                owner: vec![name.clone()],
                value: enumerator.clone(),
            }),
            body: vec![CStmt::new(CStmtKind::Return(Some(result(text(
                enumerator.as_str(),
            )))))],
        })
        .collect();
    let mut stmts = Vec::new();
    if !cases.is_empty() {
        stmts.push(CStmt::new(CStmtKind::Switch {
            value: name_expr(value.clone()),
            cases,
        }));
    }
    stmts.push(CStmt::new(CStmtKind::Return(Some(result(None)))));
    let value = (!values.is_empty()).then_some(value);
    let params = match out {
        Some(out) => vec![Some(out), value],
        None => vec![value],
    };
    CGenerated {
        kind,
        params,
        body: CBlock { stmts },
    }
}
