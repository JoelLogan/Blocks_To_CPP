//! An expression oracle: random well-typed `int` and `bool` expressions are
//! evaluated in Rust (with C++ semantics) and by a generated, compiled C++
//! program. Any difference means the generator printed a different tree than
//! it was given, e.g. a missing pair of parentheses.

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{BinaryOp, Expr, OutputStream, PrintSeparator, UnaryOp};
use b2c_ir::types::Type;
use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::TestRunner;

use crate::builder::Builder;
use crate::{generate_checked, gxx, options};

/// Values of the `int` variables `a`, `b`, `c`.
const INTS: [i64; 3] = [7, -3, 2];
/// Values of the `bool` variables `p`, `q`.
const BOOLS: [bool; 2] = [true, false];

#[derive(Debug, Clone, PartialEq)]
enum IntExpr {
    Lit(i64),
    Var(usize),
    Unary(UnaryOp, Box<IntExpr>),
    Binary(BinaryOp, Box<IntExpr>, Box<IntExpr>),
    Cond(Box<BoolExpr>, Box<IntExpr>, Box<IntExpr>),
}

#[derive(Debug, Clone, PartialEq)]
enum BoolExpr {
    Lit(bool),
    Var(usize),
    Not(Box<BoolExpr>),
    Compare(BinaryOp, Box<IntExpr>, Box<IntExpr>),
    Logic(BinaryOp, Box<BoolExpr>, Box<BoolExpr>),
}

/// Evaluates every subexpression (so nothing in the program can overflow or
/// divide by zero, even in a branch C++ would skip) and returns the value.
fn eval_int(e: &IntExpr) -> Option<i64> {
    let value = match e {
        IntExpr::Lit(v) => *v,
        IntExpr::Var(i) => INTS[*i],
        IntExpr::Unary(UnaryOp::Neg, x) => -eval_int(x)?,
        IntExpr::Unary(_, x) => eval_int(x)?,
        IntExpr::Binary(op, l, r) => {
            let (l, r) = (eval_int(l)?, eval_int(r)?);
            if matches!(op, BinaryOp::Div | BinaryOp::Mod) && l == i64::from(i32::MIN) && r == -1 {
                return None; // overflows `int` in C++
            }
            match op {
                BinaryOp::Add => l + r,
                BinaryOp::Sub => l - r,
                BinaryOp::Mul => l.checked_mul(r)?,
                BinaryOp::Div => l.checked_div(r)?,
                BinaryOp::Mod => l.checked_rem(r)?,
                _ => unreachable!(),
            }
        }
        IntExpr::Cond(c, t, f) => {
            let (c, t, f) = (eval_bool(c)?, eval_int(t)?, eval_int(f)?);
            if c { t } else { f }
        }
    };
    i32::try_from(value).ok().map(i64::from)
}

fn eval_bool(e: &BoolExpr) -> Option<bool> {
    Some(match e {
        BoolExpr::Lit(v) => *v,
        BoolExpr::Var(i) => BOOLS[*i],
        BoolExpr::Not(x) => !eval_bool(x)?,
        BoolExpr::Compare(op, l, r) => {
            let (l, r) = (eval_int(l)?, eval_int(r)?);
            match op {
                BinaryOp::Lt => l < r,
                BinaryOp::Le => l <= r,
                BinaryOp::Gt => l > r,
                BinaryOp::Ge => l >= r,
                BinaryOp::Eq => l == r,
                BinaryOp::Ne => l != r,
                _ => unreachable!(),
            }
        }
        BoolExpr::Logic(op, l, r) => {
            let (l, r) = (eval_bool(l)?, eval_bool(r)?);
            match op {
                BinaryOp::And => l && r,
                BinaryOp::Or => l || r,
                BinaryOp::Eq => l == r,
                BinaryOp::Ne => l != r,
                _ => unreachable!(),
            }
        }
    })
}

/// Whether a comparison of identical operands appears (GCC's
/// `-Wtautological-compare` would reject the program under `-Werror`).
fn has_self_comparison_int(e: &IntExpr) -> bool {
    match e {
        IntExpr::Lit(_) | IntExpr::Var(_) => false,
        IntExpr::Unary(_, x) => has_self_comparison_int(x),
        IntExpr::Binary(_, l, r) => has_self_comparison_int(l) || has_self_comparison_int(r),
        IntExpr::Cond(c, t, f) => {
            has_self_comparison_bool(c) || has_self_comparison_int(t) || has_self_comparison_int(f)
        }
    }
}

fn has_self_comparison_bool(e: &BoolExpr) -> bool {
    match e {
        BoolExpr::Lit(_) | BoolExpr::Var(_) => false,
        BoolExpr::Not(x) => has_self_comparison_bool(x),
        BoolExpr::Compare(_, l, r) => l == r || has_self_comparison_int(l) || has_self_comparison_int(r),
        BoolExpr::Logic(op, l, r) => {
            (matches!(op, BinaryOp::Eq | BinaryOp::Ne) && l == r)
                || has_self_comparison_bool(l)
                || has_self_comparison_bool(r)
        }
    }
}

fn int_expr(depth: u32) -> BoxedStrategy<IntExpr> {
    let leaf = prop_oneof![
        (0i64..12).prop_map(IntExpr::Lit),
        (0..INTS.len()).prop_map(IntExpr::Var)
    ];
    if depth == 0 {
        return leaf.boxed();
    }
    let arith = prop::sample::select(vec![
        BinaryOp::Add,
        BinaryOp::Sub,
        BinaryOp::Mul,
        BinaryOp::Div,
        BinaryOp::Mod,
    ]);
    let unary = prop::sample::select(vec![UnaryOp::Neg, UnaryOp::Plus]);
    prop_oneof![
        2 => leaf,
        1 => (unary, int_expr(depth - 1)).prop_map(|(op, x)| IntExpr::Unary(op, Box::new(x))),
        4 => (arith, int_expr(depth - 1), int_expr(depth - 1))
            .prop_map(|(op, l, r)| IntExpr::Binary(op, Box::new(l), Box::new(r))),
        1 => (bool_expr(depth - 1), int_expr(depth - 1), int_expr(depth - 1))
            .prop_map(|(c, t, f)| IntExpr::Cond(Box::new(c), Box::new(t), Box::new(f))),
    ]
    .boxed()
}

fn bool_expr(depth: u32) -> BoxedStrategy<BoolExpr> {
    let leaf = prop_oneof![
        any::<bool>().prop_map(BoolExpr::Lit),
        (0..BOOLS.len()).prop_map(BoolExpr::Var)
    ];
    if depth == 0 {
        return leaf.boxed();
    }
    let compare = prop::sample::select(vec![
        BinaryOp::Lt,
        BinaryOp::Le,
        BinaryOp::Gt,
        BinaryOp::Ge,
        BinaryOp::Eq,
        BinaryOp::Ne,
    ]);
    let logic = prop::sample::select(vec![BinaryOp::And, BinaryOp::Or, BinaryOp::Eq, BinaryOp::Ne]);
    prop_oneof![
        1 => leaf,
        1 => bool_expr(depth - 1).prop_map(|x| BoolExpr::Not(Box::new(x))),
        2 => (compare, int_expr(depth - 1), int_expr(depth - 1))
            .prop_map(|(op, l, r)| BoolExpr::Compare(op, Box::new(l), Box::new(r))),
        2 => (logic, bool_expr(depth - 1), bool_expr(depth - 1))
            .prop_map(|(op, l, r)| BoolExpr::Logic(op, Box::new(l), Box::new(r))),
    ]
    .boxed()
}

/// Builds SAST expressions.
struct ToSast<'a> {
    b: &'a Builder,
    ints: Vec<SymbolId>,
    bools: Vec<SymbolId>,
}

impl ToSast<'_> {
    fn int(&self, e: &IntExpr) -> Expr {
        match e {
            IntExpr::Lit(v) => self.b.int(&v.to_string()),
            IntExpr::Var(i) => self.b.get(&self.ints[*i]),
            IntExpr::Unary(op, x) => self.b.un(*op, self.int(x)),
            IntExpr::Binary(op, l, r) => self.b.bin(*op, self.int(l), self.int(r)),
            IntExpr::Cond(c, t, f) => self.b.cond(self.bool(c), self.int(t), self.int(f)),
        }
    }

    fn bool(&self, e: &BoolExpr) -> Expr {
        match e {
            BoolExpr::Lit(v) => self.b.boolean(*v),
            BoolExpr::Var(i) => self.b.get(&self.bools[*i]),
            BoolExpr::Not(x) => self.b.un(UnaryOp::Not, self.bool(x)),
            BoolExpr::Compare(op, l, r) => self.b.bin(*op, self.int(l), self.int(r)),
            BoolExpr::Logic(op, l, r) => self.b.bin(*op, self.bool(l), self.bool(r)),
        }
    }
}

#[test]
fn generated_expressions_compute_what_the_tree_says() {
    if !gxx::available() {
        return;
    }
    let mut runner = TestRunner::deterministic();
    let mut ints = Vec::new();
    let mut bools = Vec::new();
    while ints.len() < 150 {
        let e = int_expr(4).new_tree(&mut runner).unwrap().current();
        if let Some(value) = eval_int(&e).filter(|_| !has_self_comparison_int(&e)) {
            ints.push((e, value));
        }
    }
    while bools.len() < 150 {
        let e = bool_expr(4).new_tree(&mut runner).unwrap().current();
        if let Some(value) = eval_bool(&e).filter(|_| !has_self_comparison_bool(&e)) {
            bools.push((e, value));
        }
    }

    let b = Builder::new();
    let int_vars: Vec<_> = ["a", "b", "c"].iter().map(|n| b.var(n, Type::Int)).collect();
    let bool_vars: Vec<_> = ["p", "q"].iter().map(|n| b.var(n, Type::Bool)).collect();
    let mut body = Vec::new();
    for (var, value) in int_vars.iter().zip(INTS) {
        let literal = b.int(&value.abs().to_string());
        let init = if value < 0 {
            b.un(UnaryOp::Neg, literal)
        } else {
            literal
        };
        body.push(b.declare(var, Some(init)));
    }
    for (var, value) in bool_vars.iter().zip(BOOLS) {
        body.push(b.declare(var, Some(b.boolean(value))));
    }
    let to_sast = ToSast {
        b: &b,
        ints: int_vars,
        bools: bool_vars,
    };
    for (e, _) in &ints {
        body.push(b.print_with(
            vec![to_sast.int(e)],
            PrintSeparator::None,
            true,
            OutputStream::Out,
        ));
    }
    for (e, _) in &bools {
        body.push(b.print_with(
            vec![to_sast.bool(e)],
            PrintSeparator::None,
            true,
            OutputStream::Out,
        ));
    }
    let main = b.main(body);
    let program = b.program(vec![main]);
    let project = generate_checked(&program, &options("Oracle"));
    let executable = gxx::build(&project, &[]).unwrap();
    let out = gxx::run(&executable, b"");
    assert!(out.status.success(), "{}", out.stderr());
    let stdout = out.stdout();
    let source_lines: Vec<&str> = project.files[0]
        .contents
        .lines()
        .filter(|l| l.trim_start().starts_with("std::cout"))
        .collect();
    let expected = ints
        .iter()
        .map(|(_, v)| v.to_string())
        .chain(bools.iter().map(|(_, v)| v.to_string()));
    for (index, (actual, expected)) in stdout.lines().zip(expected).enumerate() {
        assert_eq!(actual, expected, "expression {index}: {}", source_lines[index]);
    }
    assert_eq!(stdout.lines().count(), ints.len() + bools.len());
}
