//! Constant folding (L10, M3 contracts types.md §11.2): what it reports, the
//! values it works out, and two cross-checks on random expressions: a
//! reference evaluator with exact arithmetic (no report on an expression
//! that is not constant, and the same reports and values otherwise), and
//! g++, which must agree with every value as a `constexpr`.

// Test code: unwrap/expect/panic and printing to stderr are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use std::fmt::Write as _;
use std::num::FpCategory;

use b2c_ir::diag::Severity;
use b2c_ir::ids::SymbolId;
use b2c_lang::{Analysis, ConstValue, Constant};
use common::*;
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::{Config, TestRunner};
use serde_json::Value;

const DIVISION_BY_ZERO: &str = "B2C-E0316";
const OVERFLOW: &str = "B2C-E0517";
const NOT_FINITE: &str = "B2C-W0506";

/// The codes constant folding reports, in a program's diagnostics, sorted.
fn folding_codes(a: &Analysis) -> Vec<String> {
    let mut codes: Vec<String> = codes(a)
        .into_iter()
        .filter(|code| [DIVISION_BY_ZERO, OVERFLOW, NOT_FINITE].contains(&code.as_str()))
        .collect();
    codes.sort();
    codes
}

/// The value the analysis recorded for the `const` variable `name`.
fn constant(a: &Analysis, name: &str) -> Option<Constant> {
    a.constants().get(&SymbolId::new(&sym(name)).unwrap()).copied()
}

/// `main` with `x` (an `int` variable, not constant), the constants `zero`
/// (0), `three` (3) and `half` (`const double`, 0.5), then `body`.
fn program(body: Vec<Value>) -> Analysis {
    let mut steps = vec![
        declare("int", "x", Some(num("7"))),
        declare_const("int", "zero", num("0")),
        declare_const("int", "three", num("3")),
        declare_const("double", "half", num("0.5")),
    ];
    steps.extend(body);
    run_main(steps)
}

/// `const <ty> r = <src>` in [`program`].
fn folded(ty: &str, src: &str) -> Analysis {
    program(vec![declare_const(ty, "r", e(src))])
}

#[track_caller]
fn reports(ty: &str, src: &str, expected: &[&str]) {
    let a = folded(ty, src);
    assert_eq!(folding_codes(&a), expected, "{src}:\n{}", render_all(&a));
}

#[track_caller]
fn value(ty: &str, src: &str) -> Option<ConstValue> {
    let a = folded(ty, src);
    assert!(
        !a.diagnostics.iter().any(|d| d.severity == Severity::Error),
        "{src}:\n{}",
        render_all(&a)
    );
    constant(&a, "r").map(|c| c.value)
}

#[test]
fn whole_number_division_by_a_constant_zero() {
    for src in [
        "5 / 0",
        "x / 0",
        "x % 0",
        "x / zero",
        "x / (three - 3)",
        "x % (2 * 0)",
        "x / false",
    ] {
        reports("int", src, &[DIVISION_BY_ZERO]);
    }
    let a = folded("int", "x / 0");
    let d = only(&a, DIVISION_BY_ZERO);
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(
        d.message,
        "This divides a whole number by zero: the value it divides by is always 0. That stops the program with a \
         crash. Check the value you divide by."
    );
    assert!(
        only(&folded("int", "x % zero"), DIVISION_BY_ZERO)
            .message
            .contains("'mod'")
    );
    // Characters divide as whole numbers.
    reports("int", "'a' / 0", &[DIVISION_BY_ZERO]);
    // Not constant, or not zero: nothing.
    for src in ["x / x", "x / (x - x)", "x / three", "5 / 2", "x % 1", "x / -1"] {
        reports("int", src, &[]);
    }
    // One report for one mistake, also further up.
    reports(
        "int",
        "(x / 0) + (1 / 0) * 2",
        &[DIVISION_BY_ZERO, DIVISION_BY_ZERO],
    );
}

#[test]
fn division_by_zero_in_blocks_and_change_blocks() {
    let a = program(vec![
        declare("int", "r", Some(arith("div", get("x"), num("0")))),
        declare("int", "m", Some(arith("mod", get("x"), get("zero")))),
        change_op("x", "div", num("0")),
        change_op("x", "mod", e("three - 3")),
        declare("double", "d", Some(num("1.5"))),
        change_op("d", "div", num("0")),
        change_op("x", "div", num("2")),
        change_op("d", "mul", num("0")),
        // `x / 0.0` divides as decimal numbers: no crash, but the infinity or
        // NaN is then stored in the whole number `x`, which C++ leaves
        // undefined.
        change_op("x", "div", num("0.0")),
        change_op("d", "div", num("0.0")),
    ]);
    assert_eq!(
        folding_codes(&a),
        [
            DIVISION_BY_ZERO,
            DIVISION_BY_ZERO,
            DIVISION_BY_ZERO,
            DIVISION_BY_ZERO,
            OVERFLOW,
            NOT_FINITE,
            NOT_FINITE
        ],
        "{}",
        render_all(&a)
    );
    // The two messages of `change`: a remainder, and a whole division.
    let a = program(vec![change_op("x", "mod", num("0"))]);
    assert!(
        only(&a, DIVISION_BY_ZERO).message.contains("'mod'"),
        "{}",
        render_all(&a)
    );
    let a = program(vec![change_op("x", "div", num("0"))]);
    assert!(
        only(&a, DIVISION_BY_ZERO)
            .message
            .starts_with("This divides a whole number by zero")
    );
}

/// A decimal division by zero whose infinity or NaN goes straight into a
/// whole number is an error (`B2C-E0517`, as for `convert`), and a warning
/// (`B2C-W0506`) everywhere else.
#[test]
fn decimal_division_by_zero_into_a_whole_number() {
    #[track_caller]
    fn check(steps: Vec<Value>, expected: &[&str]) {
        let a = program(steps);
        assert_eq!(folding_codes(&a), expected, "{}", render_all(&a));
    }
    let into_whole = &[OVERFLOW][..];
    let warning = &[NOT_FINITE][..];
    check(vec![declare("int", "y", Some(e("x / 0.0")))], into_whole);
    check(vec![declare("char", "y", Some(e("1 / 0.0")))], into_whole);
    check(
        vec![declare("int", "y", Some(e("-(x / (half - 0.5))")))],
        into_whole,
    );
    check(
        vec![declare("int", "y", Some(e("true ? x / 0.0 : 1.5")))],
        into_whole,
    );
    check(vec![set("x", e("x / 0.0"))], into_whole);
    check(vec![change_op("x", "div", e("0.0"))], into_whole);
    check(vec![repeat(e("10 / 0.0"), vec![])], into_whole);
    check(vec![exit(e("1 / 0.0"))], into_whole);
    check(
        vec![declare("int", "y", Some(convert("int", e("x / 0.0"))))],
        into_whole,
    );
    check(
        vec![declare("int", "y", Some(random_int(e("1 / 0.0"), num("3"))))],
        into_whole,
    );
    // A double, a `bool` (infinity and NaN are `true`) or a value that is
    // not stored as it is: the warning.
    check(vec![declare("double", "y", Some(e("x / 0.0")))], warning);
    check(vec![declare("bool", "y", Some(e("x / 0.0")))], warning);
    check(
        vec![declare("int", "y", Some(e("x > 0 ? x / 0.0 : 1.5")))],
        warning,
    );
    check(vec![declare("int", "y", Some(e("(x / 0.0) > 1.0")))], warning);
    check(vec![print(vec![e("x / 0.0")])], warning);
    let a = program(vec![declare("int", "y", Some(e("x / 0.0")))]);
    let d = only(&a, OVERFLOW);
    assert_eq!(d.severity, Severity::Error);
    assert_eq!(
        d.message,
        "This divides by zero: the value it divides by is always 0, so the result is infinity or 'nan' (not a \
         number), and it is then stored as a whole number, which cannot hold either, so C++ gives an \
         unpredictable result. Check the value you divide by."
    );
}

/// A function's return value and arguments go into its types too.
#[test]
fn decimal_division_by_zero_into_a_return_value_or_argument() {
    let a = run(vec![
        func(
            "whole",
            "int",
            &[("n", "int", "copy")],
            vec![ret(Some(e("n / 0.0")))],
        ),
        func(
            "real",
            "double",
            &[("d", "double", "copy")],
            vec![ret(Some(e("d / 0.0")))],
        ),
        main(vec![
            call_stmt("whole", vec![e("1 / 0.0")]),
            call_stmt("real", vec![e("1 / 0.0")]),
        ]),
    ]);
    assert_eq!(
        folding_codes(&a),
        [OVERFLOW, OVERFLOW, NOT_FINITE, NOT_FINITE],
        "{}",
        render_all(&a)
    );
}

#[test]
fn whole_number_overflow() {
    for src in [
        "2147483647 + 1",
        "-2147483647 - 2",
        "65536 * 65536",
        "-(-2147483647 - 1)",
        "(-2147483647 - 1) / -1",
        "(-2147483647 - 1) % -1",
        "three * 1000000000",
        "x + 2147483647 * 2",
    ] {
        reports("int", src, &[OVERFLOW]);
    }
    let a = folded("int", "2147483647 + 1");
    assert_eq!(
        only(&a, OVERFLOW).message,
        "This calculation gives 2147483648, which is too large for a whole number (the largest is 2147483647), so \
         C++ gives an unpredictable result. Use smaller values, or decimal numbers."
    );
    assert!(
        only(&folded("int", "-2147483647 - 2"), OVERFLOW)
            .message
            .contains("too small")
    );
    // The smallest `int` itself is fine, and so is everything in range.
    for src in [
        "-2147483648",
        "-2147483647 - 1",
        "2147483647 + 0",
        "46340 * 46340",
        "x * 2147483647",
    ] {
        reports("int", src, &[]);
    }
    // A decimal number converted to a whole number that does not fit.
    let a = program(vec![declare_const(
        "int",
        "r",
        input(convert_block("int", e("1e10"))),
    )]);
    assert_eq!(folding_codes(&a), [OVERFLOW], "{}", render_all(&a));
    assert!(
        only(&a, OVERFLOW)
            .message
            .contains("This turns 10000000000 into a whole number")
    );
    let a = program(vec![declare_const(
        "int",
        "r",
        input(convert_block("int", e("-2147483648.5"))),
    )]);
    assert_eq!(folding_codes(&a), Vec::<String>::new(), "{}", render_all(&a));
    assert_eq!(
        constant(&a, "r").map(|c| c.value),
        Some(ConstValue::Int(i32::MIN))
    );
}

/// A `math.convert` block, without the `input` wrapper.
fn convert_block(to: &str, value: Value) -> Value {
    convert(to, value)["block"].clone()
}

#[test]
fn decimal_results_that_are_not_finite() {
    for src in [
        "1.5 / 0",
        "x / 0.0",
        "1.0 / zero",
        "half / (half - 0.5)",
        "1e308 * 10",
        "-1e308 - 1e308",
    ] {
        reports("double", src, &[NOT_FINITE]);
    }
    let a = folded("double", "x / 0.0");
    assert_eq!(only(&a, NOT_FINITE).severity, Severity::Warning);
    for src in ["x / 2.0", "1e308 + 1", "1e-300 * 1e-300", "half / 2"] {
        reports("double", src, &[]);
    }
}

#[test]
fn operands_cpp_does_not_evaluate_are_not_checked() {
    for src in [
        "false && (1 / 0 == 1)",
        "true || (x % 0 == 1)",
        "(three > 5) && (2147483647 + 1 > 0)",
        "zero == 0 || 1 / zero > 0",
    ] {
        reports("bool", src, &[]);
    }
    reports("int", "true ? 1 : 1 / 0", &[]);
    reports("int", "three < 0 ? 2147483647 + 1 : 2", &[]);
    // When the left side or the condition is not constant, both may run.
    reports("bool", "x > 0 && (1 / 0 == 1)", &[DIVISION_BY_ZERO]);
    reports("int", "x > 0 ? 1 : 1 / 0", &[DIVISION_BY_ZERO]);
    reports("bool", "true && (1 / 0 == 1)", &[DIVISION_BY_ZERO]);
    // A constant left side that decides makes the whole a constant.
    assert_eq!(value("bool", "false && x > 0"), Some(ConstValue::Bool(false)));
    assert_eq!(value("bool", "true || x > 0"), Some(ConstValue::Bool(true)));
    assert_eq!(value("bool", "x > 0 || true"), None);
    assert_eq!(value("int", "three > 2 ? 10 : x"), Some(ConstValue::Int(10)));
}

#[test]
fn values_follow_cpp() {
    use ConstValue as V;
    assert_eq!(value("int", "7 / 2"), Some(V::Int(3)));
    assert_eq!(value("int", "-7 / 2"), Some(V::Int(-3)));
    assert_eq!(value("int", "-7 % 2"), Some(V::Int(-1)));
    assert_eq!(value("int", "7 % -2"), Some(V::Int(1)));
    assert_eq!(value("int", "'a' + 1"), Some(V::Int(98)));
    assert_eq!(value("int", "three * three - zero"), Some(V::Int(9)));
    assert_eq!(value("int", "-2147483648"), Some(V::Int(i32::MIN)));
    assert_eq!(
        value("double", "7 / 2"),
        Some(V::Double(3.0)),
        "converted to the variable's type"
    );
    assert_eq!(value("double", "7 / 2.0"), Some(V::Double(3.5)));
    assert_eq!(value("double", "half * 3"), Some(V::Double(1.5)));
    assert_eq!(value("bool", "3 < 2.5"), Some(V::Bool(false)));
    assert_eq!(value("bool", "!(three == 3)"), Some(V::Bool(false)));
    assert_eq!(value("char", "'q'"), Some(V::Char(b'q')));
    assert_eq!(value("char", "true ? 'y' : 'n'"), Some(V::Char(b'y')));
    assert_eq!(value("bool", "three"), Some(V::Bool(true)));
    // Not constant.
    assert_eq!(value("int", "x + 1"), None);
    assert_eq!(value("int", "three + x"), None);
    // A `char` outside ASCII depends on the target, so it is not recorded.
    let a = folded("char", "200");
    assert_eq!(constant(&a, "r"), None);
    // Blocks fold like slots.
    let a = program(vec![declare_const(
        "int",
        "r",
        arith("mul", get("three"), logic_value()),
    )]);
    assert_eq!(
        constant(&a, "r").map(|c| c.value),
        Some(V::Int(3)),
        "{}",
        render_all(&a)
    );
}

/// `true and (not false)` as blocks: a constant `bool`.
fn logic_value() -> Value {
    logic("and", vec![boolean(true), not(boolean(false))])
}

#[test]
fn constants_record_whether_cpp_sees_a_constant_expression() {
    let a = program(vec![
        declare_const("int", "a", e("three * 2")),
        declare_const("double", "b", e("half * 2")),
        declare_const("double", "c", e("2.5")),
        declare_const("int", "d", e("x + 1")),
        declare("int", "e", Some(e("3"))),
        declare_const("auto", "f", e("three > 1")),
    ]);
    let get = |name: &str| constant(&a, name);
    assert_eq!(
        get("a"),
        Some(Constant {
            value: ConstValue::Int(6),
            constant_expression: true
        })
    );
    // Reading a `const double` is not a constant expression in C++.
    assert_eq!(
        get("b"),
        Some(Constant {
            value: ConstValue::Double(1.0),
            constant_expression: false
        })
    );
    assert_eq!(
        get("c"),
        Some(Constant {
            value: ConstValue::Double(2.5),
            constant_expression: true
        })
    );
    assert_eq!(get("three").map(|c| c.constant_expression), Some(true));
    assert_eq!(get("d"), None, "not constant");
    assert_eq!(get("e"), None, "not `const`");
    assert_eq!(get("f").map(|c| c.value), Some(ConstValue::Bool(true)));
    assert_eq!(get("x"), None);
    // The public evaluation agrees for any expression.
    let main = &a.program.modules[0].items[0];
    let b2c_ir::sast::ItemKind::Main(main) = &main.kind else {
        panic!()
    };
    let b2c_ir::sast::StmtKind::VarDecl(decl) = &main.body.stmts[4].kind else {
        panic!()
    };
    assert_eq!(
        b2c_lang::constant_value(decl.init.as_ref().unwrap(), a.constants()),
        Some(Constant {
            value: ConstValue::Int(6),
            constant_expression: true
        })
    );
}

#[test]
fn m2_examples_report_nothing_new() {
    for (name, document) in resolved_examples() {
        let a = analyze(&document);
        assert_eq!(folding_codes(&a), Vec::<String>::new(), "{name}");
    }
}

// --- Random expressions -------------------------------------------------------------

/// A whole-number expression.
#[derive(Debug, Clone)]
enum I {
    Lit(i64),
    /// `x`: not constant.
    X,
    /// `three`: the constant 3.
    Three,
    Neg(Box<I>),
    Bin(&'static str, Box<I>, Box<I>),
    Cond(Box<B>, Box<I>, Box<I>),
}

/// A `bool` expression.
#[derive(Debug, Clone)]
enum B {
    Lit(bool),
    Not(Box<B>),
    Cmp(&'static str, Box<I>, Box<I>),
    Logic(&'static str, Box<B>, Box<B>),
}

/// A `double` expression.
#[derive(Debug, Clone)]
enum D {
    Lit(&'static str),
    /// `half`: a `const double` 0.5, which C++ does not read in constant
    /// expressions.
    Half,
    Neg(Box<D>),
    Bin(&'static str, Box<D>, Box<D>),
    /// A whole number on the left of a decimal operation.
    Mix(&'static str, Box<I>, Box<D>),
}

/// An expression of one of the three types.
#[derive(Debug, Clone)]
enum Any {
    I(I),
    B(B),
    D(D),
}

fn int_expr() -> BoxedStrategy<I> {
    let leaf = prop_oneof![
        4 => prop::sample::select(vec![0_i64, 1, 2, 3, 7, 46341, 65536, 2_147_483_647]).prop_map(I::Lit),
        1 => Just(I::X),
        1 => Just(I::Three),
    ];
    leaf.prop_recursive(4, 24, 3, |inner| {
        prop_oneof![
            1 => inner.clone().prop_map(|e| I::Neg(Box::new(e))),
            4 => (prop::sample::select(vec!["+", "-", "*", "/", "%"]), inner.clone(), inner.clone())
                .prop_map(|(op, l, r)| I::Bin(op, Box::new(l), Box::new(r))),
            1 => (bool_leaf(inner.clone()), inner.clone(), inner)
                .prop_map(|(c, t, e)| I::Cond(Box::new(c), Box::new(t), Box::new(e))),
        ]
    })
    .boxed()
}

/// A `bool` made from whole-number expressions, one level deep.
fn bool_leaf(ints: impl Strategy<Value = I> + Clone) -> impl Strategy<Value = B> {
    prop_oneof![
        1 => any::<bool>().prop_map(B::Lit),
        3 => (prop::sample::select(vec!["<", "<=", ">", ">=", "==", "!="]), ints.clone(), ints)
            .prop_map(|(op, l, r)| B::Cmp(op, Box::new(l), Box::new(r))),
    ]
}

fn bool_expr() -> impl Strategy<Value = B> {
    bool_leaf(int_expr()).prop_recursive(3, 16, 2, |inner| {
        prop_oneof![
            1 => inner.clone().prop_map(|b| B::Not(Box::new(b))),
            3 => (prop::sample::select(vec!["&&", "||"]), inner.clone(), inner)
                .prop_map(|(op, l, r)| B::Logic(op, Box::new(l), Box::new(r))),
        ]
    })
}

fn double_expr() -> impl Strategy<Value = D> {
    let leaf = prop_oneof![
        4 => prop::sample::select(vec!["0.0", "0.5", "2.5", "1e308", "1e-300", "3.0"]).prop_map(D::Lit),
        1 => Just(D::Half),
    ];
    leaf.prop_recursive(3, 16, 2, |inner| {
        prop_oneof![
            1 => inner.clone().prop_map(|e| D::Neg(Box::new(e))),
            3 => (prop::sample::select(vec!["+", "-", "*", "/"]), inner.clone(), inner.clone())
                .prop_map(|(op, l, r)| D::Bin(op, Box::new(l), Box::new(r))),
            1 => (prop::sample::select(vec!["+", "-", "*", "/"]), int_expr(), inner)
                .prop_map(|(op, l, r)| D::Mix(op, Box::new(l), Box::new(r))),
        ]
    })
}

fn any_expr() -> impl Strategy<Value = Any> {
    prop_oneof![
        3 => int_expr().prop_map(Any::I),
        2 => bool_expr().prop_map(Any::B),
        2 => double_expr().prop_map(Any::D),
    ]
}

// Printing: fully parenthesised C++, which is also slot syntax.

fn show_i(e: &I) -> String {
    match e {
        I::Lit(v) => v.to_string(),
        I::X => String::from("x"),
        I::Three => String::from("three"),
        I::Neg(e) => format!("(-{})", show_i(e)),
        I::Bin(op, l, r) => format!("({} {op} {})", show_i(l), show_i(r)),
        I::Cond(c, t, e) => format!("({} ? {} : {})", show_b(c), show_i(t), show_i(e)),
    }
}

fn show_b(e: &B) -> String {
    match e {
        B::Lit(v) => v.to_string(),
        B::Not(b) => format!("(!{})", show_b(b)),
        B::Cmp(op, l, r) => format!("({} {op} {})", show_i(l), show_i(r)),
        B::Logic(op, l, r) => format!("({} {op} {})", show_b(l), show_b(r)),
    }
}

fn show_d(e: &D) -> String {
    match e {
        D::Lit(v) => (*v).to_owned(),
        D::Half => String::from("half"),
        D::Neg(e) => format!("(-{})", show_d(e)),
        D::Bin(op, l, r) => format!("({} {op} {})", show_d(l), show_d(r)),
        D::Mix(op, l, r) => format!("({} {op} {})", show_i(l), show_d(r)),
    }
}

/// What the reference evaluator found: the codes it expects and, if the
/// expression is constant, its value and whether C++ sees a constant
/// expression.
#[derive(Debug, Default)]
struct Reference {
    codes: Vec<String>,
}

/// A value and whether it is a constant expression in C++.
type Known<T> = Option<(T, bool)>;

impl Reference {
    fn fault(&mut self, code: &str) {
        self.codes.push(code.to_owned());
    }

    /// Exact whole-number arithmetic in `i128`, checked against `int`.
    fn int(&mut self, expr: &I) -> Known<i128> {
        let fits = |value: i128| value >= i128::from(i32::MIN) && value <= i128::from(i32::MAX);
        match expr {
            I::Lit(value) => Some((i128::from(*value), true)),
            I::X => None,
            I::Three => Some((3, true)),
            I::Neg(operand) => {
                let (value, constant) = self.int(operand)?;
                if fits(-value) {
                    Some((-value, constant))
                } else {
                    self.fault(OVERFLOW);
                    None
                }
            }
            I::Bin(op, lhs, rhs) => {
                let left = self.int(lhs);
                let right = self.int(rhs);
                if matches!(*op, "/" | "%") && right.is_some_and(|(divisor, _)| divisor == 0) {
                    self.fault(DIVISION_BY_ZERO);
                    return None;
                }
                let ((x, x_constant), (y, y_constant)) = (left?, right?);
                let value = match *op {
                    "+" => x + y,
                    "-" => x - y,
                    "*" => x * y,
                    // `i128` division truncates towards zero, as C++ does.
                    "/" => x / y,
                    _ if x / y == i128::from(i32::MAX) + 1 => {
                        // C++ leaves `x % y` undefined when `x / y` does not fit.
                        self.fault(OVERFLOW);
                        return None;
                    }
                    _ => x % y,
                };
                if fits(value) {
                    Some((value, x_constant && y_constant))
                } else {
                    self.fault(OVERFLOW);
                    None
                }
            }
            I::Cond(cond, then_value, else_value) => {
                let Some((cond, cond_constant)) = self.bool(cond) else {
                    self.int(then_value);
                    self.int(else_value);
                    return None;
                };
                let (value, constant) = self.int(if cond { then_value } else { else_value })?;
                Some((value, cond_constant && constant))
            }
        }
    }

    fn bool(&mut self, expr: &B) -> Known<bool> {
        match expr {
            B::Lit(value) => Some((*value, true)),
            B::Not(operand) => self.bool(operand).map(|(value, constant)| (!value, constant)),
            B::Cmp(op, lhs, rhs) => {
                let left = self.int(lhs);
                let right = self.int(rhs);
                let ((x, x_constant), (y, y_constant)) = (left?, right?);
                let value = match *op {
                    "<" => x < y,
                    "<=" => x <= y,
                    ">" => x > y,
                    ">=" => x >= y,
                    "==" => x == y,
                    _ => x != y,
                };
                Some((value, x_constant && y_constant))
            }
            B::Logic(op, lhs, rhs) => {
                let decides = *op == "||";
                let left = self.bool(lhs);
                if let Some((value, constant)) = left
                    && value == decides
                {
                    return Some((decides, constant));
                }
                let right = self.bool(rhs);
                let ((_, x_constant), (value, y_constant)) = (left?, right?);
                Some((value, x_constant && y_constant))
            }
        }
    }

    fn double(&mut self, expr: &D) -> Known<f64> {
        let (left, right, op) = match expr {
            D::Lit(text) => return Some((text.parse().unwrap(), true)),
            D::Half => return Some((0.5, false)),
            D::Neg(operand) => return self.double(operand).map(|(value, constant)| (-value, constant)),
            D::Bin(op, lhs, rhs) => (self.double(lhs), self.double(rhs), *op),
            D::Mix(op, lhs, rhs) => {
                // Exact: every `int` is a `double`.
                #[allow(clippy::cast_precision_loss)]
                let left = self.int(lhs).map(|(value, constant)| (value as f64, constant));
                (left, self.double(rhs), *op)
            }
        };
        if op == "/" && right.is_some_and(|(divisor, _)| divisor.classify() == FpCategory::Zero) {
            self.fault(NOT_FINITE);
            return None;
        }
        let ((x, x_constant), (y, y_constant)) = (left?, right?);
        let value = match op {
            "+" => x + y,
            "-" => x - y,
            "*" => x * y,
            _ => x / y,
        };
        if value.is_finite() {
            Some((value, x_constant && y_constant))
        } else {
            self.fault(NOT_FINITE);
            None
        }
    }

    /// Evaluates an expression: its C++ type, source, and value.
    fn eval(&mut self, e: &Any) -> (&'static str, String, Known<ConstValue>) {
        match e {
            Any::I(e) => (
                "int",
                show_i(e),
                self.int(e)
                    .map(|(v, ce)| (ConstValue::Int(i32::try_from(v).unwrap()), ce)),
            ),
            Any::B(e) => (
                "bool",
                show_b(e),
                self.bool(e).map(|(v, ce)| (ConstValue::Bool(v), ce)),
            ),
            Any::D(e) => (
                "double",
                show_d(e),
                self.double(e).map(|(v, ce)| (ConstValue::Double(v), ce)),
            ),
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// The analyser reports exactly what the reference evaluator finds,
    /// nothing on an expression that is not constant unless a constant part of
    /// it is at fault, and records the same value.
    #[test]
    fn folding_agrees_with_a_reference_evaluator(expr in any_expr()) {
        let mut reference = Reference::default();
        let (ty, src, known) = reference.eval(&expr);
        let mut expected = reference.codes.clone();
        expected.sort();
        let a = folded(ty, &src);
        prop_assert_eq!(folding_codes(&a), expected, "{}\n{}", src, render_all(&a));
        // Nothing else is wrong with the program.
        let others: Vec<String> = a
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error && d.code.0 != DIVISION_BY_ZERO && d.code.0 != OVERFLOW)
            .map(|d| d.code.0.clone())
            .collect();
        prop_assert!(others.is_empty(), "{}\n{}", src, render_all(&a));
        let recorded = constant(&a, "r").map(|c| (c.value, c.constant_expression));
        prop_assert_eq!(recorded, known, "{}", src);
    }
}

/// Random expressions whose value the reference evaluator works out with no
/// report and that use no variable: g++ must accept each as a `constexpr`
/// with the same value, under C++17 and C++20 (`static_assert`).
#[test]
fn folded_values_agree_with_gxx() {
    let mut runner = TestRunner::new_with_rng(
        Config::default(),
        proptest::test_runner::TestRng::deterministic_rng(proptest::test_runner::RngAlgorithm::ChaCha),
    );
    let strategy = any_expr();
    let mut source = String::from("constexpr int three = 3;\n");
    let mut cases = 0;
    let mut checked = Vec::new();
    while cases < 400 {
        let expr = strategy.new_tree(&mut runner).unwrap().current();
        let mut reference = Reference::default();
        let (ty, src, known) = reference.eval(&expr);
        if src.contains('x') || src.contains("half") || !reference.codes.is_empty() {
            continue;
        }
        let Some((value, _)) = known else { continue };
        let value = match value {
            ConstValue::Int(v) if v == i32::MIN => String::from("(-2147483647 - 1)"),
            ConstValue::Int(v) => v.to_string(),
            ConstValue::Bool(v) => v.to_string(),
            ConstValue::Double(v) => format!("{v:?}"),
            ConstValue::Char(_) => unreachable!(),
        };
        writeln!(
            source,
            "constexpr {ty} r{cases} = {src};\nstatic_assert(r{cases} == {value}, \"{cases}\");"
        )
        .unwrap();
        checked.push(src);
        cases += 1;
    }
    // Every operator is in the sample.
    for op in [" / ", " % ", " && ", " || ", " ? ", "(-", " * "] {
        assert!(checked.iter().any(|src| src.contains(op)), "{op}");
    }
    if !gxx::available() {
        return;
    }
    for standard in ["c++17", "c++20"] {
        if let Err(messages) = gxx::syntax_check_with(&source, standard) {
            panic!("g++ ({standard}) disagrees:\n{messages}\n--- source ---\n{source}");
        }
    }
}
