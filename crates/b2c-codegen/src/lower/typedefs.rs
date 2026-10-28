//! User types' definitions with the functions their ticks and uses promise
//! (types.md §1.6 and §5.2; w2.md §1.5, §2.11): the comparisons of a
//! comparable struct, the `operator<<` of a printable struct, and the
//! `operator<<` and `b2c::to_text` of an enum that is printed, written or
//! joined; and the struct and enum values (*new … with*, *… of …*, an
//! enum's value, *set … of … to*).
//!
//! A type's definition is lowered between `Lowerer::start_item` and
//! `Lowerer::finish_item`, and records its headers in the item's
//! declaration part: a type's definition goes where its declaration is
//! printed, the module's header when it is shared (ADR-0019). Types are
//! printed in the declaration graph's order (`ModuleFiles::source_order`,
//! modules.md §4), so each comes after the types it holds; a struct named
//! only in the lists of one printed before it gets a forward declaration
//! there.

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{CppStandard, Expr, ExprKind, Item, ItemKind, StmtKind, SymbolKind};
use b2c_ir::text::{Ident, StrLit};
use b2c_ir::types::{LibType, Type};

use super::collections::fits_as_written;
use super::scan::EnumUses;
use super::{Lowerer, is_scalar, name_expr, scan};
use crate::cast::{
    AssignOp, BinOp, CBlock, CCase, CEnum, CExpr, CExprKind, CField, CGenerated, CStmt, CStmtKind, CStruct,
    CType, CTypeDef, Generated, Init,
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

/// The struct a type names only inside lists (`std::vector`, at any
/// depth), which a forward declaration is enough for.
fn listed_struct<'t>(ty: &'t Type, symbols: &b2c_ir::sast::SymbolTable) -> Option<&'t SymbolId> {
    let mut ty = ty;
    let mut in_list = false;
    while let Type::Lib(app) = ty {
        match (app.ty(), app.args()) {
            (LibType::Vector, [item]) => {
                ty = item;
                in_list = true;
            }
            _ => return None,
        }
    }
    match ty {
        Type::Named(id)
            if in_list
                && matches!(
                    symbols.get(id).map(|symbol| &symbol.kind),
                    Some(SymbolKind::Struct { .. })
                ) =>
        {
            Some(id)
        }
        _ => None,
    }
}

impl Lowerer<'_> {
    /// A struct or enum item (`type.struct`, `type.enum`; w2.md §1.5): its
    /// definition with the functions its ticks and `uses` promise, after
    /// forward declarations of the structs its lists name that are not in
    /// `printed` yet. Records the types it prints in `printed`.
    pub(super) fn type_item(
        &mut self,
        item: &Item,
        uses: &EnumUses,
        printed: &mut BTreeSet<SymbolId>,
    ) -> Vec<CTypeDef> {
        let mut defs = Vec::new();
        self.start_item();
        match &item.kind {
            ItemKind::Struct(def) => {
                let symbols = self.symbols;
                if let Some(SymbolKind::Struct { fields, .. }) = symbols.get(&def.symbol).map(|s| &s.kind) {
                    let listed = fields
                        .iter()
                        .filter_map(|field| symbols.get(field))
                        .filter_map(|field| listed_struct(&field.ty, symbols));
                    for id in listed {
                        if *id != def.symbol
                            && !printed.contains(id)
                            && let Some(symbol) = symbols.get(id)
                        {
                            printed.insert(id.clone());
                            defs.push(CTypeDef::Forward(symbol.name.clone()));
                        }
                    }
                }
                if let Some(mut lowered) = self.struct_def(&def.symbol, &def.defaults) {
                    lowered.comment.clone_from(&item.comment);
                    defs.push(CTypeDef::Struct(lowered));
                }
                printed.insert(def.symbol.clone());
            }
            ItemKind::Enum(def) => {
                let print = uses.printed.contains(&def.symbol);
                let joined = uses.joined.contains(&def.symbol);
                if let Some(mut lowered) = self.enum_def(&def.symbol, print, joined) {
                    lowered.comment.clone_from(&item.comment);
                    defs.push(CTypeDef::Enum(lowered));
                }
                printed.insert(def.symbol.clone());
            }
            _ => {}
        }
        self.finish_item(&item.headers);
        defs
    }

    /// *set … of … to* (`StmtKind::SetField`): `s.mark = x;`.
    pub(super) fn type_stmt(&mut self, kind: &StmtKind) -> CStmtKind {
        let StmtKind::SetField { target, field, value } = kind else {
            return CStmtKind::Error;
        };
        let (Some(variable), Some(member)) = (self.variable(target), self.symbol(field)) else {
            return CStmtKind::Error;
        };
        CStmtKind::AssignTo {
            target: CExpr::field(name_expr(variable.name.clone()), member.name.clone()),
            op: AssignOp::Set,
            value: self.expr(value),
        }
    }

    /// A struct's field (`obj.mark`), a new struct value (`Student{…}`) or
    /// an enum value (`Action::Deposit`).
    pub(super) fn type_expr(&mut self, expr: &Expr) -> CExpr {
        match &expr.kind {
            ExprKind::Field { object, field } => {
                let Some(member) = self.symbol(field) else {
                    return CExpr::error();
                };
                let object = self.expr(object);
                CExpr::field(object, member.name.clone())
            }
            ExprKind::New { ty, fields } => self.new_struct(ty, fields, &expr.ty),
            ExprKind::Enumerator(value) => {
                let symbols = self.symbols;
                let Some(symbol) = symbols.get(value) else {
                    return CExpr::error();
                };
                let SymbolKind::Enumerator { owner } = &symbol.kind else {
                    return CExpr::error();
                };
                let Some(owner) = symbols.get(owner) else {
                    return CExpr::error();
                };
                CExpr::new(CExprKind::Enumerator {
                    owner: vec![owner.name.clone()],
                    value: symbol.name.clone(),
                })
            }
            _ => CExpr::error(),
        }
    }

    /// *new [S] with …*: in C++20 `Student{.name = n, .mark = m}`, empty
    /// parts left out; in C++17 `Student{n, m}`, an empty part before a
    /// filled one as `{}` (w2.md §1.5). Each part whose type is not its
    /// field's is cast as a brace list's items are (slots.md §3.1).
    fn new_struct(&mut self, id: &SymbolId, parts: &[Option<Expr>], ty: &Type) -> CExpr {
        let symbols = self.symbols;
        let Some(SymbolKind::Struct { fields, .. }) = symbols.get(id).map(|symbol| &symbol.kind) else {
            return CExpr::error();
        };
        let cty = self.ctype(ty);
        self.note_type(&cty);
        let designated = self.standard >= CppStandard::Cpp20;
        let mut items: Vec<(Option<Ident>, CExpr)> = Vec::new();
        let mut pending_empty = 0;
        for (part, field) in parts.iter().zip(fields) {
            let Some(field) = symbols.get(field) else {
                return CExpr::error();
            };
            let Some(part) = part else {
                pending_empty += 1;
                continue;
            };
            if !designated {
                for _ in 0..pending_empty {
                    items.push((None, CExpr::new(CExprKind::Braces(Vec::new()))));
                }
            }
            pending_empty = 0;
            let mut lowered = self.expr(part);
            if is_scalar(&field.ty) && !fits_as_written(part, &field.ty) {
                lowered = CExpr::cast(self.ctype(&field.ty), lowered);
            }
            let name = designated.then(|| field.name.clone());
            items.push((name, lowered));
        }
        CExpr::new(CExprKind::Init { ty: cty, items })
    }

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
    ///
    /// The default member initialisers of the struct's rows (`int mark =
    /// 0;`, W2's package C2) are in `defaults`; a field without one is
    /// value-initialised (`int mark{};`).
    pub(super) fn struct_def(
        &mut self,
        id: &SymbolId,
        defaults: &BTreeMap<SymbolId, Expr>,
    ) -> Option<CStruct> {
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
        for (sym, field) in fields
            .iter()
            .filter_map(|sym| symbols.get(sym).map(|field| (sym, field)))
        {
            let ty = self.ctype(&field.ty);
            self.note_type(&ty);
            let init = match defaults.get(sym) {
                Some(value) => Init::Value(self.expr(value)),
                None => Init::Braces,
            };
            lowered.push(CField {
                ty,
                name: field.name.clone(),
                init,
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
    /// Color::Red` does not compile for an `enum class`) and `b2c::to_text`
    /// when it is joined or turned into text (`<string>`), each writing an
    /// enumerator's name. `None` when `id` is not an enum.
    pub(super) fn enum_def(&mut self, id: &SymbolId, print: bool, to_text: bool) -> Option<CEnum> {
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
        if to_text {
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

/// An enum's `operator<<` (with the stream `out`) or `to_text`: a `switch`
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
            default: None,
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
