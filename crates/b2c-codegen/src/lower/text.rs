//! Desugaring W2's text, input and output, and math nodes
//! (`docs/plans/m3-contracts/w2.md` §1.3, §1.4): package B.
//!
//! The forms they share with the collection blocks are in `forms.rs`
//! (`element_at`, `size_of`, `library_object`), the C++ nodes in `cast.rs`,
//! the helpers in `helpers.rs`. Each form records the headers it needs: a
//! standard function its header, a helper its own (types.md §5.2).

use b2c_ir::sast::{CppStandard, Expr, ExprKind, MathOp, StmtKind, TextOp};
use b2c_ir::types::Type;

use super::Lowerer;
use crate::cast::{BinOp, CExpr, CStmtKind, CType};
use crate::helpers::Helper;

/// Whether an operand is an `int` literal, which is never negative here and
/// so converts to `std::size_t` without a cast and without a warning.
fn is_int_literal(expr: &Expr) -> bool {
    matches!(expr.kind, ExprKind::Int(_))
}

impl Lowerer<'_> {
    /// *show numbers with … decimal places* (`StmtKind::Decimals`):
    /// `std::cout << std::fixed << std::setprecision(n);`.
    pub(super) fn text_stmt(&mut self, kind: &StmtKind) -> CStmtKind {
        let StmtKind::Decimals { digits } = kind else {
            return CStmtKind::Error;
        };
        self.include("<iostream>");
        self.include("<iomanip>");
        let digits = self.expr(digits);
        let fixed = CExpr::binary(BinOp::Shl, CExpr::fixed("std::cout"), CExpr::fixed("std::fixed"));
        let precision = CExpr::call_fixed("std::setprecision", vec![digits]);
        CStmtKind::Expr(CExpr::binary(BinOp::Shl, fixed, precision))
    }

    /// *letter of*, *length of*, the text calls, *text as number*, *as text*,
    /// the math calls and *random decimal*.
    pub(super) fn text_expr(&mut self, expr: &Expr) -> CExpr {
        match &expr.kind {
            ExprKind::Letter { text, index } => self.element_at(text, index),
            ExprKind::TextLength(text) => self.size_of(text),
            ExprKind::TextCall { op, args } => self.text_call(*op, args),
            ExprKind::ToNumber { to, text } => {
                self.include("<string>");
                let name = if *to == Type::Double {
                    "std::stod"
                } else {
                    "std::stoi"
                };
                let text = self.expr(text);
                CExpr::call_fixed(name, vec![text])
            }
            ExprKind::AsText(value) => {
                let lowered = self.expr(value);
                self.std_string_of(value, lowered)
            }
            ExprKind::MathCall { op, args } => self.math_call(*op, args, &expr.ty),
            ExprKind::RandomReal { low, high } => {
                self.use_helper(Helper::RandomReal);
                let low = self.expr(low);
                let high = self.expr(high);
                CExpr::call_fixed("b2c::random_real", vec![low, high])
            }
            _ => CExpr::error(),
        }
    }

    /// The operations of `TextOp` (w2.md §1.3).
    fn text_call(&mut self, op: TextOp, args: &[Expr]) -> CExpr {
        let helper = |name: &'static str, helper: Helper, lowerer: &mut Self| {
            lowerer.use_helper(helper);
            let args = args.iter().map(|arg| lowerer.expr(arg)).collect();
            CExpr::call_fixed(name, args)
        };
        match (op, args) {
            (TextOp::ToLower, _) => helper("b2c::to_lower", Helper::ToLower, self),
            (TextOp::ToUpper, _) => helper("b2c::to_upper", Helper::ToUpper, self),
            (TextOp::Split, _) => helper("b2c::split", Helper::Split, self),
            (TextOp::Trim, _) => helper("b2c::trim", Helper::Trim, self),
            (TextOp::ReplaceAll, _) => helper("b2c::replace_all", Helper::ReplaceAll, self),
            (TextOp::Contains, [text, part]) => {
                // `s.find(t) != std::string::npos`
                let object = self.library_object(text);
                let part = self.expr(part);
                self.include("<string>");
                let found = CExpr::method(object, "find", vec![part]);
                CExpr::binary(BinOp::Ne, found, CExpr::fixed("std::string::npos"))
            }
            (TextOp::StartsWith | TextOp::EndsWith, [text, part]) => {
                let starts = op == TextOp::StartsWith;
                if self.standard == CppStandard::Cpp17 {
                    // `std::string`'s own members arrive with C++20.
                    if starts {
                        helper("b2c::starts_with", Helper::StartsWith, self)
                    } else {
                        helper("b2c::ends_with", Helper::EndsWith, self)
                    }
                } else {
                    let object = self.library_object(text);
                    let part = self.expr(part);
                    let name = if starts { "starts_with" } else { "ends_with" };
                    CExpr::method(object, name, vec![part])
                }
            }
            (TextOp::Part, [text, start, length]) => {
                // `s.substr(static_cast<std::size_t>(i), static_cast<std::size_t>(n))`
                let object = self.library_object(text);
                let start = self.size_argument(start);
                let length = self.size_argument(length);
                CExpr::method(object, "substr", vec![start, length])
            }
            _ => CExpr::error(),
        }
    }

    /// A position or length passed to a `std::string` member, which takes a
    /// `std::size_t`: cast, so the code compiles clean under
    /// `-Wsign-conversion` (the *strict* level), unless it is an `int`
    /// literal, as `element_at` does.
    fn size_argument(&mut self, value: &Expr) -> CExpr {
        let lowered = self.expr(value);
        if is_int_literal(value) {
            lowered
        } else {
            let size = CType::Size;
            self.note_type(&size);
            CExpr::cast(size, lowered)
        }
    }

    /// The math functions (w2.md §1.4). The `number` results (`std::abs`,
    /// `std::max`, `std::min`, `std::clamp`) have the type of the block's
    /// output; *larger*, *smaller* and *kept between* name it as the
    /// template's argument, so operands of mixed types never fail to compile
    /// (`std::max<double>(i, c)`).
    fn math_call(&mut self, op: MathOp, args: &[Expr], ty: &Type) -> CExpr {
        let lowered: Vec<CExpr> = args.iter().map(|arg| self.expr(arg)).collect();
        match op {
            MathOp::Abs => {
                // `std::abs` of a whole number is declared in `<cstdlib>`, of a
                // decimal number in `<cmath>`.
                if *ty == Type::Double {
                    self.include("<cmath>");
                } else {
                    self.include("<cstdlib>");
                }
                CExpr::call_fixed(op.cpp_name(), lowered)
            }
            MathOp::Max | MathOp::Min | MathOp::Clamp => {
                self.include("<algorithm>");
                let ty = self.ctype(ty);
                CExpr::call_template(op.cpp_name(), vec![ty], lowered)
            }
            _ => {
                self.include("<cmath>");
                CExpr::call_fixed(op.cpp_name(), lowered)
            }
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    //! Each form of the text, input and output, and math nodes, printed
    //! from hand-built SAST, with the headers and helpers it records.

    use std::collections::BTreeSet;

    use b2c_ir::headers::Header;
    use b2c_ir::ids::{BlockId, ModuleId, SymbolId};
    use b2c_ir::layout::Layout;
    use b2c_ir::sast::{Module, Origin, Program, Symbol, SymbolKind, SymbolTable};
    use b2c_ir::text::{CharLit, Ident, NumLit, NumType, StrLit};

    use super::*;
    use crate::emit;

    fn origin() -> Origin {
        Origin::whole(ModuleId::new("mod_main").unwrap(), BlockId::new("b1").unwrap())
    }

    pub(in crate::lower) fn expr(kind: ExprKind, ty: Type) -> Expr {
        Expr {
            kind,
            ty,
            origin: origin(),
        }
    }

    pub(in crate::lower) fn int(text: &str) -> Expr {
        expr(
            ExprKind::Int(NumLit::parse(text, NumType::Int).unwrap()),
            Type::Int,
        )
    }

    fn real(text: &str) -> Expr {
        expr(
            ExprKind::Float(NumLit::parse(text, NumType::Double).unwrap()),
            Type::Double,
        )
    }

    pub(in crate::lower) fn text(value: &str) -> Expr {
        expr(ExprKind::Str(StrLit::new(value).unwrap()), Type::String)
    }

    fn letter(value: &str) -> Expr {
        expr(ExprKind::Char(CharLit::new(value).unwrap()), Type::Char)
    }

    /// The symbol of the variable `name`.
    pub(in crate::lower) fn var_id(name: &str) -> SymbolId {
        SymbolId::new(&format!("sym_{name}")).unwrap()
    }

    /// The variable `name` as a value.
    fn var(name: &str, ty: Type) -> Expr {
        expr(ExprKind::Var(var_id(name)), ty)
    }

    /// A program of `standard` whose symbols are these variables, by name.
    pub(in crate::lower) fn program(standard: CppStandard, vars: &[(&str, Type)]) -> Program {
        let mut symbols = SymbolTable::default();
        for (name, ty) in vars {
            let symbol = Symbol {
                name: Ident::new(name).unwrap(),
                kind: SymbolKind::Variable { is_const: false },
                ty: ty.clone(),
                origin: origin(),
            };
            symbols.symbols.insert(var_id(name), symbol);
        }
        let modules = vec![Module {
            id: ModuleId::new("mod_main").unwrap(),
            name: String::from("main"),
            items: Vec::new(),
        }];
        Program {
            standard,
            layout: Layout::flat(&modules, &symbols),
            modules,
            symbols,
        }
    }

    /// A lowerer over `program` with an item started, its code in the body
    /// part.
    pub(in crate::lower) fn lowerer(program: &Program) -> Lowerer<'_> {
        let mut lowerer = Lowerer::new(program);
        lowerer.start_item();
        lowerer.start_body();
        lowerer.scopes = vec![Vec::new()];
        lowerer
    }

    pub(in crate::lower) fn headers(names: &[&str]) -> BTreeSet<Header> {
        names.iter().map(|name| Header::standard(name).unwrap()).collect()
    }

    /// The C++ of `expr` and the headers it records.
    fn printed(program: &Program, expr: &Expr) -> (String, BTreeSet<Header>) {
        let mut lowerer = lowerer(program);
        let cpp = lowerer.expr(expr);
        (
            emit::expr_text(&cpp).trim_end().to_owned(),
            lowerer.item_headers.body,
        )
    }

    fn call(op: TextOp, args: Vec<Expr>, ty: Type) -> Expr {
        expr(ExprKind::TextCall { op, args }, ty)
    }

    fn math(op: MathOp, args: Vec<Expr>, ty: Type) -> Expr {
        expr(ExprKind::MathCall { op, args }, ty)
    }

    #[test]
    fn text_forms_print_with_their_headers() {
        let program = program(CppStandard::Cpp20, &[("s", Type::String), ("i", Type::Int)]);
        let s = || var("s", Type::String);
        let letter_at = |text: Expr, index: Expr| {
            expr(
                ExprKind::Letter {
                    text: Box::new(text),
                    index: Box::new(index),
                },
                Type::Char,
            )
        };
        let to_number = |to: Type, text: Expr| {
            expr(
                ExprKind::ToNumber {
                    to: to.clone(),
                    text: Box::new(text),
                },
                to,
            )
        };
        let cases = [
            (
                expr(ExprKind::TextLength(Box::new(s())), Type::Int),
                "static_cast<int>(s.size())",
                vec!["<string>"],
            ),
            (
                letter_at(s(), var("i", Type::Int)),
                "s.at(static_cast<std::size_t>(i))",
                vec!["<cstddef>", "<string>"],
            ),
            (
                letter_at(text("abc"), int("0")),
                r#"std::string("abc").at(0)"#,
                vec!["<string>"],
            ),
            (to_number(Type::Int, s()), "std::stoi(s)", vec!["<string>"]),
            (
                to_number(Type::Double, text("2.5")),
                r#"std::stod("2.5")"#,
                vec!["<string>"],
            ),
            (
                call(TextOp::Contains, vec![s(), text("a")], Type::Bool),
                r#"s.find("a") != std::string::npos"#,
                vec!["<string>"],
            ),
            (
                call(TextOp::StartsWith, vec![text("ab"), text("a")], Type::Bool),
                r#"std::string("ab").starts_with("a")"#,
                vec!["<string>"],
            ),
            (
                call(
                    TextOp::Part,
                    vec![s(), var("i", Type::Int), int("3")],
                    Type::String,
                ),
                "s.substr(static_cast<std::size_t>(i), 3)",
                vec!["<cstddef>", "<string>"],
            ),
            (
                call(TextOp::ReplaceAll, vec![s(), text("a"), text("o")], Type::String),
                r#"b2c::replace_all(s, "a", "o")"#,
                vec![],
            ),
            (
                call(TextOp::Split, vec![s(), text(",")], Type::Error),
                "0 /* error */",
                vec![],
            ),
        ];
        for (expr, expected, needs) in cases {
            let (cpp, found) = printed(&program, &expr);
            assert_eq!(cpp, expected);
            assert_eq!(found, headers(&needs), "{expected}");
        }
    }

    #[test]
    fn the_text_of_a_text_operation_is_used_as_it_is() {
        // `b2c::to_lower(s)` and `s.substr(…)` are `std::string` objects
        // already, so neither a join nor a member call wraps them in
        // `std::string(…)` again, as it does a text literal.
        let program = program(CppStandard::Cpp20, &[("s", Type::String)]);
        let s = || var("s", Type::String);
        let lower = || call(TextOp::ToLower, vec![s()], Type::String);
        let letter_of = |text: Expr| {
            expr(
                ExprKind::Letter {
                    text: Box::new(text),
                    index: Box::new(int("0")),
                },
                Type::Char,
            )
        };
        let part = call(TextOp::Part, vec![s(), int("1"), int("3")], Type::String);
        for (value, expected) in [
            (letter_of(lower()), "b2c::to_lower(s).at(0)"),
            (
                expr(ExprKind::Join(vec![lower(), text("!")]), Type::String),
                r#"b2c::to_lower(s) + "!""#,
            ),
            (
                expr(ExprKind::Join(vec![part, text("!")]), Type::String),
                r#"s.substr(1, 3) + "!""#,
            ),
            (letter_of(text("abc")), r#"std::string("abc").at(0)"#),
        ] {
            assert_eq!(printed(&program, &value).0, expected);
        }
    }

    #[test]
    fn starts_and_ends_with_are_helpers_in_cpp17() {
        let program = program(CppStandard::Cpp17, &[("s", Type::String)]);
        let mut lowerer = lowerer(&program);
        for (op, expected) in [
            (TextOp::StartsWith, r#"b2c::starts_with(s, "a")"#),
            (TextOp::EndsWith, r#"b2c::ends_with(s, "a")"#),
        ] {
            let cpp = lowerer.expr(&call(op, vec![var("s", Type::String), text("a")], Type::Bool));
            assert_eq!(emit::expr_text(&cpp).trim_end(), expected);
        }
        assert_eq!(
            lowerer.usage.helpers.iter().copied().collect::<Vec<_>>(),
            [Helper::StartsWith, Helper::EndsWith]
        );
    }

    #[test]
    fn as_text_writes_each_type_as_join_does() {
        let program = program(CppStandard::Cpp20, &[("b", Type::Bool), ("s", Type::String)]);
        let as_text = |value: Expr| expr(ExprKind::AsText(Box::new(value)), Type::String);
        for (value, expected) in [
            (int("7"), "std::to_string(7)"),
            (real("2.5"), "b2c::to_text(2.5)"),
            (letter("c"), "std::string(1, 'c')"),
            (var("b", Type::Bool), r#"std::string(b ? "true" : "false")"#),
            (text("hi"), r#"std::string("hi")"#),
            (var("s", Type::String), "s"),
        ] {
            assert_eq!(printed(&program, &as_text(value)).0, expected);
        }
    }

    #[test]
    fn math_forms_name_their_type_and_header() {
        let program = program(CppStandard::Cpp20, &[("c", Type::Char), ("x", Type::Double)]);
        let c = || var("c", Type::Char);
        let cases = [
            (
                math(MathOp::Sqrt, vec![int("9")], Type::Double),
                "std::sqrt(9)",
                vec!["<cmath>"],
            ),
            (
                math(MathOp::Abs, vec![int("3")], Type::Int),
                "std::abs(3)",
                vec!["<cstdlib>"],
            ),
            (
                math(MathOp::Abs, vec![var("x", Type::Double)], Type::Double),
                "std::abs(x)",
                vec!["<cmath>"],
            ),
            (
                math(MathOp::Max, vec![c(), real("2.5")], Type::Double),
                "std::max<double>(c, 2.5)",
                vec!["<algorithm>"],
            ),
            (
                math(MathOp::Min, vec![c(), int("2")], Type::Int),
                "std::min<int>(c, 2)",
                vec!["<algorithm>"],
            ),
            (
                math(MathOp::Clamp, vec![c(), int("0"), real("9.5")], Type::Double),
                "std::clamp<double>(c, 0, 9.5)",
                vec!["<algorithm>"],
            ),
            (
                math(MathOp::Hypot, vec![int("3"), int("4")], Type::Double),
                "std::hypot(3, 4)",
                vec!["<cmath>"],
            ),
            (
                expr(
                    ExprKind::RandomReal {
                        low: Box::new(real("0.0")),
                        high: Box::new(real("1.0")),
                    },
                    Type::Double,
                ),
                "b2c::random_real(0.0, 1.0)",
                vec![],
            ),
        ];
        for (expr, expected, needs) in cases {
            let (cpp, found) = printed(&program, &expr);
            assert_eq!(cpp, expected);
            assert_eq!(found, headers(&needs), "{expected}");
        }
    }

    #[test]
    fn decimals_set_the_console_format() {
        let program = program(CppStandard::Cpp20, &[]);
        let mut lowerer = lowerer(&program);
        let stmt = lowerer.text_stmt(&StmtKind::Decimals { digits: int("2") });
        assert_eq!(
            emit::stmt_text(&stmt).trim_end(),
            "std::cout << std::fixed << std::setprecision(2);"
        );
        assert_eq!(lowerer.item_headers.body, headers(&["<iomanip>", "<iostream>"]));
    }
}
