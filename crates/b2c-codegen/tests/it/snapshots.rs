//! Snapshots of the generated C++ for every statement and expression kind and
//! for the worked examples. Every snapshot is also compiled with g++
//! (`-Wall -Wextra -Wpedantic -Werror` and more), so a snapshot can never
//! accept code that does not build.

use b2c_codegen::{CodegenOptions, HelperPlacement};
use b2c_ir::ids::ModuleId;
use b2c_ir::sast::{
    AskMode, BinaryOp, CompoundOp, Module, OutputStream, PassMode, PrintSeparator, Program, RangeDirection,
    StmtKind, UnaryOp,
};
use b2c_ir::types::Type;

use crate::builder::{Builder, block, commented, commented_item};
use crate::{examples, generate_checked, gxx, options, render};

/// Snapshots a program with the default test options and compiles it.
fn check(name: &str, program: &Program) {
    check_with(name, program, &options("Snapshot Tests"));
}

/// Snapshots a program and compiles every generated source file.
fn check_with(name: &str, program: &Program, options: &CodegenOptions) {
    let project = generate_checked(program, options);
    insta::assert_snapshot!(name, render(&project));
    gxx::syntax_check(&project, "c++20");
}

#[test]
fn guessing_game() {
    check_with(
        "guessing_game",
        &examples::guessing_game(),
        &options("Guessing Game"),
    );
}

#[test]
fn functions() {
    check_with("functions", &examples::functions(), &options("Functions"));
}

#[test]
fn empty_main() {
    let b = Builder::new();
    let main = b.main(Vec::new());
    check("empty_main", &b.program(vec![main]));
}

#[test]
fn variables() {
    let b = Builder::new();
    let score = b.var("score", Type::Int);
    let rate = b.constant("rate", Type::Double);
    let count = b.var("count", Type::Int);
    let name = b.var("name", Type::String);
    let title = b.var("title", Type::String);
    let done = b.var("done", Type::Bool);
    let letter = b.var("letter", Type::Char);
    let total = b.constant("total", Type::Int);
    let main = b.main(vec![
        b.declare(&score, Some(b.int("0"))),
        b.declare_with(&rate, Some(b.float("2.5")), true, false),
        b.declare_with(&count, Some(b.int("3")), false, true),
        b.declare_with(&name, Some(b.str("Ada")), false, true),
        b.declare(&title, None),
        b.declare(&done, None),
        b.declare(&letter, Some(b.chr("x"))),
        b.set(&score, b.bin(BinaryOp::Add, b.get(&score), b.get(&count))),
        b.set(&title, b.get(&name)),
        b.declare_with(
            &total,
            Some(b.bin(BinaryOp::Mul, b.get(&score), b.int("2"))),
            true,
            true,
        ),
        b.print_with(
            vec![
                b.get(&score),
                b.get(&rate),
                b.get(&name),
                b.get(&title),
                b.get(&done),
                b.get(&letter),
                b.get(&total),
            ],
            PrintSeparator::Space,
            true,
            OutputStream::Out,
        ),
    ]);
    check("variables", &b.program(vec![main]));
}

#[test]
fn assignments() {
    let b = Builder::new();
    let x = b.var("x", Type::Int);
    let d = b.var("d", Type::Double);
    let change = |target, op, value| b.stmt(StmtKind::CompoundAssign { target, op, value });
    let main = b.main(vec![
        b.declare(&x, Some(b.int("10"))),
        b.declare(&d, Some(b.float("1.5"))),
        change(x.clone(), CompoundOp::Add, b.int("5")),
        change(x.clone(), CompoundOp::Add, b.int("1")),
        change(x.clone(), CompoundOp::Sub, b.int("1")),
        change(x.clone(), CompoundOp::Mul, b.int("2")),
        change(x.clone(), CompoundOp::Div, b.int("3")),
        change(x.clone(), CompoundOp::Mod, b.int("4")),
        change(x.clone(), CompoundOp::Add, b.un(UnaryOp::Neg, b.int("1"))),
        change(d.clone(), CompoundOp::Add, b.int("1")),
        change(d.clone(), CompoundOp::Mul, b.float("0.5")),
        b.set(&x, b.bin(BinaryOp::Mul, b.get(&x), b.get(&x))),
        b.print(vec![b.get(&x), b.get(&d)]),
    ]);
    check("assignments", &b.program(vec![main]));
}

#[test]
fn if_else() {
    let b = Builder::new();
    let n = b.var("n", Type::Int);
    let main = b.main(vec![
        b.declare(&n, Some(b.int("5"))),
        b.if_else(
            vec![
                (
                    b.bin(BinaryOp::Lt, b.get(&n), b.int("3")),
                    vec![b.print_text("small")],
                ),
                (
                    b.bin(BinaryOp::Lt, b.get(&n), b.int("7")),
                    vec![b.print_text("medium")],
                ),
                (
                    b.bin(BinaryOp::Lt, b.get(&n), b.int("9")),
                    vec![b.print_text("large")],
                ),
            ],
            Some(vec![b.print_text("huge")]),
        ),
        b.if_else(
            vec![(
                b.bin(BinaryOp::Eq, b.get(&n), b.int("5")),
                vec![b.print_text("five")],
            )],
            None,
        ),
        b.if_else(
            vec![(
                b.bin(BinaryOp::Gt, b.get(&n), b.int("0")),
                vec![b.if_else(
                    vec![(b.boolean(true), vec![b.print_text("nested")])],
                    Some(Vec::new()),
                )],
            )],
            Some(vec![b.print_text("not positive")]),
        ),
        b.if_else(vec![(b.boolean(false), Vec::new())], None),
    ]);
    check("if_else", &b.program(vec![main]));
}

#[test]
fn while_loops() {
    let b = Builder::new();
    let n = b.var("n", Type::Int);
    let level = b.var("level", Type::Double);
    let done = b.var("done", Type::Bool);
    let tired = b.var("tired", Type::Bool);
    let name = b.var("name", Type::String);
    let until = |cond, body| {
        b.stmt(StmtKind::While {
            cond,
            until: true,
            body: block(body),
        })
    };
    let main = b.main(vec![
        b.declare(&n, Some(b.int("10"))),
        b.declare(&level, Some(b.float("5.0"))),
        b.declare(&done, Some(b.boolean(false))),
        b.declare(&tired, Some(b.boolean(false))),
        b.declare(&name, Some(b.str("x"))),
        b.stmt(StmtKind::While {
            cond: b.bin(BinaryOp::Gt, b.get(&n), b.int("0")),
            until: false,
            body: block(vec![b.stmt(StmtKind::CompoundAssign {
                target: n.clone(),
                op: CompoundOp::Sub,
                value: b.int("3"),
            })]),
        }),
        until(
            b.bin(BinaryOp::Eq, b.get(&n), b.int("0")),
            vec![b.set(&n, b.int("0"))],
        ),
        until(
            b.bin(BinaryOp::Ge, b.get(&n), b.int("5")),
            vec![b.set(&n, b.int("5"))],
        ),
        until(
            b.bin(BinaryOp::Lt, b.get(&level), b.float("1.0")),
            vec![b.set(&level, b.float("0.5"))],
        ),
        until(
            b.bin(BinaryOp::Ne, b.get(&level), b.float("0.5")),
            vec![b.set(&level, b.float("0.5"))],
        ),
        until(
            b.bin(BinaryOp::Gt, b.get(&name), b.str("w")),
            vec![b.set(&name, b.str("y"))],
        ),
        until(b.get(&done), vec![b.set(&done, b.boolean(true))]),
        until(
            b.un(UnaryOp::Not, b.get(&done)),
            vec![b.set(&done, b.boolean(false))],
        ),
        until(
            b.bin(BinaryOp::And, b.get(&done), b.get(&tired)),
            vec![b.set(&tired, b.boolean(true))],
        ),
        until(b.boolean(true), Vec::new()),
        until(
            b.bin(
                BinaryOp::Eq,
                b.bin(BinaryOp::Eq, b.get(&n), b.int("5")),
                b.get(&done),
            ),
            vec![b.set(&done, b.boolean(true))],
        ),
        b.print(vec![
            b.get(&n),
            b.get(&level),
            b.get(&done),
            b.get(&tired),
            b.get(&name),
        ]),
    ]);
    check("while_loops", &b.program(vec![main]));
}

#[test]
fn repeat_loops() {
    let b = Builder::new();
    let i = b.var("i", Type::Int);
    let n = b.var("n", Type::Int);
    let j = b.var("j", Type::Int);
    let main = b.main(vec![
        b.declare(&i, Some(b.int("2"))),
        b.declare(&n, Some(b.int("3"))),
        // `i` is taken by the user, so the counter is `j`.
        b.repeat(b.int("3"), vec![b.print_text("a")]),
        // Nested: `j` outside, `k` inside.
        b.repeat(
            b.int("2"),
            vec![b.repeat(b.get(&i), vec![b.print(vec![b.get(&i)])])],
        ),
        // The body changes `n`, so the count is evaluated once.
        b.repeat(
            b.get(&n),
            vec![b.stmt(StmtKind::CompoundAssign {
                target: n.clone(),
                op: CompoundOp::Add,
                value: b.int("1"),
            })],
        ),
        // A random count is evaluated once too.
        b.repeat(b.random(b.int("1"), b.int("3")), vec![b.print_text("r")]),
        // The body declares `j`, so the counter skips it.
        b.repeat(
            b.int("2"),
            vec![b.declare(&j, Some(b.int("5"))), b.print(vec![b.get(&j)])],
        ),
        b.print(vec![b.get(&i), b.get(&n)]),
    ]);
    check("repeat_loops", &b.program(vec![main]));
}

#[test]
fn repeat_counters_avoid_every_visible_name() {
    let b = Builder::new();
    let f = b.function("k", Type::Int, &[]);
    let k_def = b.define(&f, vec![b.ret(Some(b.int("1")))]);
    let i = b.var("i", Type::Int);
    let j = b.var("j", Type::Int);
    let i2 = b.var("i2", Type::Int);
    let main = b.main(vec![
        b.declare(&i, Some(b.int("1"))),
        b.declare(&j, Some(b.int("2"))),
        b.declare(&i2, Some(b.int("3"))),
        b.repeat(
            b.call(&f, Vec::new()),
            vec![b.print(vec![b.get(&i), b.get(&j), b.get(&i2)])],
        ),
    ]);
    check("repeat_counter_names", &b.program(vec![main, k_def]));
}

#[test]
fn for_loops() {
    let b = Builder::new();
    let total = b.var("total", Type::Int);
    let s = b.var("s", Type::Int);
    let limit = b.var("limit", Type::Int);
    let add_to_total = |var| {
        b.stmt(StmtKind::CompoundAssign {
            target: total.clone(),
            op: CompoundOp::Add,
            value: var,
        })
    };
    let (i1, i2, i3, i4, i5, i6, i7) = (
        b.loop_var("i"),
        b.loop_var("i"),
        b.loop_var("i"),
        b.loop_var("i"),
        b.loop_var("i"),
        b.loop_var("i"),
        b.loop_var("i"),
    );
    let main = b.main(vec![
        b.declare(&total, Some(b.int("0"))),
        b.declare(&s, Some(b.int("3"))),
        b.declare(&limit, Some(b.int("4"))),
        b.for_range(
            &i1,
            b.int("0"),
            b.int("5"),
            None,
            RangeDirection::UpExclusive,
            vec![add_to_total(b.get(&i1))],
        ),
        b.for_range(
            &i2,
            b.int("1"),
            b.int("9"),
            Some(b.int("2")),
            RangeDirection::UpInclusive,
            vec![add_to_total(b.get(&i2))],
        ),
        b.for_range(
            &i3,
            b.int("10"),
            b.int("1"),
            None,
            RangeDirection::DownInclusive,
            vec![add_to_total(b.get(&i3))],
        ),
        b.for_range(
            &i4,
            b.int("20"),
            b.int("0"),
            Some(b.get(&s)),
            RangeDirection::DownInclusive,
            vec![add_to_total(b.get(&i4))],
        ),
        // The body changes the end, so it is evaluated once.
        b.for_range(
            &i5,
            b.int("0"),
            b.get(&limit),
            None,
            RangeDirection::UpInclusive,
            vec![b.stmt(StmtKind::CompoundAssign {
                target: limit.clone(),
                op: CompoundOp::Sub,
                value: b.int("1"),
            })],
        ),
        // A random step is evaluated once.
        b.for_range(
            &i6,
            b.int("0"),
            b.int("10"),
            Some(b.random(b.int("1"), b.int("3"))),
            RangeDirection::UpExclusive,
            vec![add_to_total(b.get(&i6))],
        ),
        b.for_range(
            &i7,
            b.int("3"),
            b.int("1"),
            Some(b.int("1")),
            RangeDirection::DownInclusive,
            vec![add_to_total(b.get(&i7))],
        ),
        b.print(vec![b.get(&total), b.get(&limit)]),
    ]);
    check("for_loops", &b.program(vec![main]));
}

#[test]
fn forever_break_continue() {
    let b = Builder::new();
    let count = b.var("count", Type::Int);
    let main = b.main(vec![
        b.declare(&count, Some(b.int("0"))),
        b.stmt(StmtKind::Forever {
            body: block(vec![
                b.stmt(StmtKind::CompoundAssign {
                    target: count.clone(),
                    op: CompoundOp::Add,
                    value: b.int("1"),
                }),
                b.if_else(
                    vec![(
                        b.bin(BinaryOp::Lt, b.get(&count), b.int("3")),
                        vec![b.stmt(StmtKind::Continue)],
                    )],
                    None,
                ),
                b.if_else(
                    vec![(
                        b.bin(BinaryOp::Ge, b.get(&count), b.int("5")),
                        vec![b.stmt(StmtKind::Break)],
                    )],
                    None,
                ),
                b.print(vec![b.get(&count)]),
            ]),
        }),
        b.print_text("done"),
    ]);
    check("forever_break_continue", &b.program(vec![main]));
}

#[test]
fn return_and_exit() {
    let b = Builder::new();
    let check_fn = b.function("check", Type::Int, &[("value", Type::Int, PassMode::Copy)]);
    let report = b.function("report", Type::Void, &[("value", Type::Int, PassMode::Copy)]);
    let value = check_fn.params[0].clone();
    let report_value = report.params[0].clone();
    let exit = |code, in_main| b.stmt(StmtKind::Exit { code, in_main });
    let check_def = b.define(
        &check_fn,
        vec![
            b.if_else(
                vec![(
                    b.bin(BinaryOp::Lt, b.get(&value), b.int("0")),
                    vec![exit(Some(b.int("2")), false)],
                )],
                None,
            ),
            b.ret(Some(b.bin(BinaryOp::Mul, b.get(&value), b.int("2")))),
        ],
    );
    let report_def = b.define(
        &report,
        vec![
            b.if_else(
                vec![(
                    b.bin(BinaryOp::Gt, b.get(&report_value), b.int("100")),
                    vec![b.ret(None)],
                )],
                None,
            ),
            b.if_else(
                vec![(
                    b.bin(BinaryOp::Eq, b.get(&report_value), b.int("7")),
                    vec![exit(None, false)],
                )],
                None,
            ),
            b.print(vec![b.get(&report_value)]),
        ],
    );
    let result = b.var("result", Type::Int);
    let main = b.main(vec![
        b.declare(&result, Some(b.call(&check_fn, vec![b.int("21")]))),
        b.eval(b.call(&report, vec![b.get(&result)])),
        b.if_else(
            vec![(
                b.bin(BinaryOp::Ne, b.get(&result), b.int("42")),
                vec![exit(Some(b.int("1")), true)],
            )],
            None,
        ),
        exit(None, true),
    ]);
    check("return_and_exit", &b.program(vec![main, check_def, report_def]));
}

#[test]
fn eval_call() {
    let b = Builder::new();
    let greet = b.function("greet", Type::Void, &[]);
    let twice = b.function("twice", Type::Int, &[("x", Type::Int, PassMode::Copy)]);
    let x = twice.params[0].clone();
    let greet_def = b.define(&greet, vec![b.print_text("Hi")]);
    let twice_def = b.define(
        &twice,
        vec![b.ret(Some(b.bin(BinaryOp::Mul, b.get(&x), b.int("2"))))],
    );
    let r = b.var("r", Type::Int);
    let main = b.main(vec![
        b.eval(b.call(&greet, Vec::new())),
        b.eval(b.call(&twice, vec![b.int("4")])),
        b.declare(&r, Some(b.call(&twice, vec![b.call(&twice, vec![b.int("5")])]))),
        b.print(vec![b.get(&r)]),
    ]);
    check("eval_call", &b.program(vec![twice_def, main, greet_def]));
}

#[test]
#[allow(clippy::many_single_char_names)] // the names mirror the generated C++
fn print_variants() {
    let b = Builder::new();
    let a = b.var("a", Type::Int);
    let c = b.var("c", Type::Int);
    let ok = b.var("ok", Type::Bool);
    let letter = b.var("letter", Type::Char);
    let d = b.var("d", Type::Double);
    let s = b.var("s", Type::String);
    let main = b.main(vec![
        b.declare(&a, Some(b.int("3"))),
        b.declare(&c, Some(b.int("4"))),
        b.declare(&ok, Some(b.boolean(true))),
        b.declare(&letter, Some(b.chr("z"))),
        b.declare(&d, Some(b.float("0.5"))),
        b.declare(&s, Some(b.str("text"))),
        b.print_text("Hello, world!"),
        b.print_with(
            vec![b.get(&a), b.get(&c)],
            PrintSeparator::None,
            false,
            OutputStream::Out,
        ),
        b.print_with(
            vec![b.get(&a), b.get(&c), b.get(&letter)],
            PrintSeparator::Comma,
            true,
            OutputStream::Out,
        ),
        b.print_with(vec![b.str("Oops")], PrintSeparator::None, true, OutputStream::Err),
        b.print_with(
            vec![
                b.get(&ok),
                b.boolean(true),
                b.bin(BinaryOp::Lt, b.get(&a), b.get(&c)),
                b.boolean(false),
            ],
            PrintSeparator::Space,
            true,
            OutputStream::Out,
        ),
        b.print(vec![
            b.bin(BinaryOp::Add, b.get(&a), b.get(&c)),
            b.bin(BinaryOp::Mul, b.get(&a), b.get(&c)),
            b.cond(b.bin(BinaryOp::Gt, b.get(&a), b.get(&c)), b.get(&a), b.get(&c)),
        ]),
        b.print_with(
            vec![b.get(&d), b.get(&s)],
            PrintSeparator::Space,
            true,
            OutputStream::Err,
        ),
        b.print(Vec::new()),
    ]);
    check("print_variants", &b.program(vec![main]));
}

#[test]
fn ask_variants() {
    let b = Builder::new();
    let name = b.var("name", Type::String);
    let city = b.var("city", Type::String);
    let motto = b.var("motto", Type::String);
    let age = b.var("age", Type::Int);
    let height = b.var("height", Type::Double);
    let grade = b.var("grade", Type::Char);
    let likes = b.var("likes", Type::Bool);
    let lucky = b.var("lucky", Type::Int);
    let count = b.var("count", Type::Int);
    let extra = b.var("extra", Type::Int);
    let keep = AskMode::KeepAsking;
    let simple = AskMode::Simple;
    let vars = [
        &name, &city, &motto, &age, &height, &grade, &likes, &lucky, &count, &extra,
    ];
    let mut body: Vec<_> = vars.iter().map(|v| b.declare(v, None)).collect();
    body.extend([
        b.ask(Some(b.str("Name? ")), &name, keep),
        b.ask(None, &city, keep),
        b.ask(Some(b.str("Motto? ")), &motto, simple),
        b.ask(Some(b.str("Age? ")), &age, keep),
        b.ask(None, &height, keep),
        b.ask(Some(b.chr("?")), &grade, keep),
        b.ask(
            Some(b.join(vec![b.str("Do you like "), b.get(&city), b.str("? ")])),
            &likes,
            keep,
        ),
        b.ask(None, &lucky, simple),
        b.ask(Some(b.str("Count? ")), &count, simple),
        b.ask(Some(b.get(&grade)), &extra, keep),
        b.ask(Some(b.get(&age)), &name, keep),
        b.print_with(
            vars.iter().map(|v| b.get(v)).collect(),
            PrintSeparator::Space,
            true,
            OutputStream::Out,
        ),
    ]);
    let main = b.main(body);
    check("ask_variants", &b.program(vec![main]));
}

#[test]
#[allow(clippy::many_single_char_names)] // the names mirror the generated C++
#[allow(clippy::too_many_lines)] // a literal test program, clearest in one piece
fn expressions() {
    let b = Builder::new();
    let a = b.var("a", Type::Int);
    let c = b.var("c", Type::Int);
    let e = b.var("e", Type::Int);
    let d = b.var("d", Type::Double);
    let p = b.var("p", Type::Bool);
    let q = b.var("q", Type::Bool);
    let (ga, gc, ge, gd, gp, gq) = (
        || b.get(&a),
        || b.get(&c),
        || b.get(&e),
        || b.get(&d),
        || b.get(&p),
        || b.get(&q),
    );
    let bin = |op, l, r| b.bin(op, l, r);
    let un = |op, x| b.un(op, x);
    let results: Vec<(&str, Type, b2c_ir::sast::Expr)> = vec![
        (
            "e1",
            Type::Int,
            bin(BinaryOp::Mul, bin(BinaryOp::Add, ga(), gc()), ge()),
        ),
        (
            "e2",
            Type::Int,
            bin(BinaryOp::Sub, ga(), bin(BinaryOp::Sub, gc(), ge())),
        ),
        (
            "e3",
            Type::Int,
            bin(BinaryOp::Sub, bin(BinaryOp::Sub, ga(), gc()), ge()),
        ),
        ("e4", Type::Int, un(UnaryOp::Neg, bin(BinaryOp::Add, ga(), gc()))),
        ("e5", Type::Int, un(UnaryOp::Neg, un(UnaryOp::Neg, ga()))),
        (
            "e6",
            Type::Int,
            bin(
                BinaryOp::Add,
                bin(BinaryOp::Mod, ga(), gc()),
                bin(BinaryOp::Div, ga(), gc()),
            ),
        ),
        ("e7", Type::Bool, un(UnaryOp::Not, bin(BinaryOp::Lt, ga(), gc()))),
        (
            "e8",
            Type::Bool,
            bin(BinaryOp::Eq, bin(BinaryOp::Lt, ga(), gc()), gp()),
        ),
        (
            "e9",
            Type::Bool,
            bin(BinaryOp::Or, gp(), bin(BinaryOp::And, gq(), gp())),
        ),
        (
            "e10",
            Type::Bool,
            bin(BinaryOp::And, bin(BinaryOp::Or, gp(), gq()), gp()),
        ),
        (
            "e11",
            Type::Bool,
            bin(
                BinaryOp::And,
                bin(BinaryOp::And, gp(), gq()),
                bin(BinaryOp::And, gq(), gp()),
            ),
        ),
        ("e12", Type::Bool, bin(BinaryOp::Eq, un(UnaryOp::Not, gp()), gq())),
        ("e13", Type::Int, b.cond(gp(), ga(), gc())),
        (
            "e14",
            Type::Int,
            bin(BinaryOp::Add, b.cond(gp(), ga(), gc()), b.int("1")),
        ),
        ("e15", Type::Int, b.cond(gp(), ga(), b.cond(gq(), gc(), ge()))),
        ("e16", Type::Int, b.cond(b.cond(gp(), gq(), gp()), ga(), gc())),
        (
            "e17",
            Type::Int,
            bin(BinaryOp::Mul, b.convert(Type::Int, gd()), b.int("2")),
        ),
        (
            "e18",
            Type::Double,
            bin(BinaryOp::Div, b.convert(Type::Double, ga()), gc()),
        ),
        ("e19", Type::Int, b.random(b.int("1"), ga())),
        (
            "e20",
            Type::Bool,
            bin(BinaryOp::Ne, bin(BinaryOp::Le, ga(), gc()), gp()),
        ),
        (
            "e21",
            Type::Double,
            bin(BinaryOp::Add, un(UnaryOp::Neg, gd()), un(UnaryOp::Plus, gd())),
        ),
        ("e22", Type::Int, un(UnaryOp::Plus, un(UnaryOp::Plus, ga()))),
        ("e23", Type::Int, bin(BinaryOp::Sub, ga(), un(UnaryOp::Neg, gc()))),
        ("e24", Type::Bool, un(UnaryOp::Not, un(UnaryOp::Not, gp()))),
        ("e25", Type::Double, b.float("1e-7")),
        (
            "e26",
            Type::Int,
            bin(BinaryOp::Mul, ga(), b.random(b.int("1"), b.int("2"))),
        ),
    ];
    let mut body = vec![
        b.declare(&a, Some(b.int("7"))),
        b.declare(&c, Some(b.int("3"))),
        b.declare(&e, Some(b.int("2"))),
        b.declare(&d, Some(b.float("2.5"))),
        b.declare(&p, Some(b.boolean(true))),
        b.declare(&q, Some(b.boolean(false))),
    ];
    let mut printed = Vec::new();
    for (name, ty, value) in results {
        let var = b.var(name, ty);
        body.push(b.declare(&var, Some(value)));
        printed.push(b.get(&var));
    }
    body.push(b.print_with(printed, PrintSeparator::Space, true, OutputStream::Out));
    let main = b.main(body);
    check("expressions", &b.program(vec![main]));
}

#[test]
fn join_variants() {
    let b = Builder::new();
    let score = b.var("score", Type::Int);
    let name = b.var("name", Type::String);
    let letter = b.var("letter", Type::Char);
    let done = b.var("done", Type::Bool);
    let ratio = b.var("ratio", Type::Double);
    let results: Vec<(&str, Type, bool, b2c_ir::sast::Expr)> = vec![
        (
            "j1",
            Type::String,
            false,
            b.join(vec![b.str("Score: "), b.get(&score)]),
        ),
        (
            "j2",
            Type::String,
            false,
            b.join(vec![b.get(&name), b.str(" "), b.get(&letter)]),
        ),
        (
            "j3",
            Type::String,
            false,
            b.join(vec![b.get(&letter), b.str("!")]),
        ),
        (
            "j4",
            Type::String,
            false,
            b.join(vec![b.get(&score), b.str(" points")]),
        ),
        ("j5", Type::String, false, b.join(vec![b.get(&done), b.str("?")])),
        (
            "j6",
            Type::String,
            false,
            b.join(vec![b.str("x"), b.get(&done), b.boolean(true), b.chr("c")]),
        ),
        ("j7", Type::String, false, b.join(vec![b.get(&name)])),
        (
            "j8",
            Type::String,
            false,
            b.join(vec![b.str("Hi "), b.join(vec![b.get(&name), b.str("!")])]),
        ),
        (
            "j9",
            Type::String,
            false,
            b.bin(BinaryOp::Add, b.str("a"), b.str("b")),
        ),
        (
            "j10",
            Type::Bool,
            false,
            b.bin(BinaryOp::Lt, b.str("a"), b.str("b")),
        ),
        (
            "j11",
            Type::Bool,
            false,
            b.bin(BinaryOp::Eq, b.str("Ada"), b.get(&name)),
        ),
        (
            "j12",
            Type::String,
            false,
            b.join(vec![b.get(&ratio), b.get(&score)]),
        ),
        (
            "j13",
            Type::String,
            false,
            b.join(vec![b.cond(b.get(&done), b.str("yes"), b.str("no"))]),
        ),
        ("j14", Type::String, true, b.str("auto text")),
        ("j15", Type::String, false, b.join(Vec::new())),
        (
            "j16",
            Type::String,
            true,
            b.cond(b.get(&done), b.str("on"), b.str("off")),
        ),
    ];
    let mut body = vec![
        b.declare(&score, Some(b.int("42"))),
        b.declare(&name, Some(b.str("Ada"))),
        b.declare(&letter, Some(b.chr("A"))),
        b.declare(&done, Some(b.boolean(true))),
        b.declare(&ratio, Some(b.float("0.5"))),
    ];
    let mut printed = Vec::new();
    for (name, ty, auto, value) in results {
        let var = b.var(name, ty);
        body.push(b.declare_with(&var, Some(value), false, auto));
        printed.push(b.get(&var));
    }
    body.push(b.print_with(printed, PrintSeparator::Comma, true, OutputStream::Out));
    let main = b.main(body);
    check("join_variants", &b.program(vec![main]));
}

#[test]
fn comments() {
    let b = Builder::new();
    let bump = b.function("bump", Type::Void, &[("score", Type::Int, PassMode::Editable)]);
    let score_param = bump.params[0].clone();
    let bump_def = commented_item(
        b.define(
            &bump,
            vec![commented(
                b.if_else(
                    vec![(
                        b.bin(BinaryOp::Lt, b.get(&score_param), b.int("10")),
                        vec![commented(
                            b.stmt(StmtKind::CompoundAssign {
                                target: score_param.clone(),
                                op: CompoundOp::Add,
                                value: b.int("1"),
                            }),
                            "Nested comments are indented.",
                        )],
                    )],
                    None,
                ),
                "Only below ten",
            )],
        ),
        "Adds one to the score.\n\nKeeps the empty line above.",
    );
    let score = b.var("score", Type::Int);
    let main = commented_item(
        b.main(vec![
            commented(b.declare(&score, Some(b.int("0"))), "Count the score"),
            commented(
                b.eval(b.call(&bump, vec![b.get(&score)])),
                "First line\nSecond line",
            ),
            commented(
                b.eval(b.call(&bump, vec![b.get(&score)])),
                "Looks like code: */ std::system(\"rm -rf /\"); /*\nx = 2;\r\nstd::system(\"evil\");",
            ),
            commented(b.eval(b.call(&bump, vec![b.get(&score)])), "A path: C:\\temp\\"),
            commented(
                b.eval(b.call(&bump, vec![b.get(&score)])),
                "trailing backslash and spaces \\   ",
            ),
            commented(
                b.print(vec![b.get(&score)]),
                "bidi: \u{202E}txt\u{2028}after a line separator\u{85}and NEL",
            ),
            commented(b.print(vec![b.get(&score)]), "what??/"),
            commented(
                b.print(vec![b.get(&score)]),
                "tab\there, vertical tab\u{0B}form feed\u{0C}end",
            ),
        ]),
        "Shows how block comments become C++ comments.",
    );
    check("comments", &b.program(vec![main, bump_def]));
}

#[test]
fn helpers_compile_under_every_supported_standard() {
    let b = Builder::new();
    let n = b.var("n", Type::Int);
    let name = b.var("name", Type::String);
    let yes = b.var("yes", Type::Bool);
    let main = b.main(vec![
        b.declare(&n, Some(b.random(b.int("1"), b.int("6")))),
        b.declare(&name, None),
        b.declare(&yes, None),
        b.ask(Some(b.str("n: ")), &n, AskMode::KeepAsking),
        b.ask(Some(b.str("name: ")), &name, AskMode::KeepAsking),
        b.ask(Some(b.str("yes: ")), &yes, AskMode::KeepAsking),
        b.print(vec![b.get(&n), b.get(&name), b.get(&yes)]),
    ]);
    let project = generate_checked(&b.program(vec![main]), &options("Standards"));
    for standard in ["c++17", "c++20", "c++23"] {
        gxx::syntax_check(&project, standard);
    }
}

#[test]
fn header_placement() {
    let options = CodegenOptions {
        helper_placement: HelperPlacement::Header,
        ..options("Guessing Game")
    };
    check_with("header_placement", &examples::guessing_game(), &options);
}

#[test]
fn style_options() {
    let options = CodegenOptions {
        indent_width: 2,
        do_not_edit_banner: false,
        ..options("Quiz \"2\"\nsecond line \u{202E}")
    };
    check_with("style_options", &examples::guessing_game(), &options);
}

#[test]
fn two_modules_share_the_support_header() {
    let b = Builder::new();
    let roll = b.function("roll", Type::Int, &[]);
    let roll_def = b.define(&roll, vec![b.ret(Some(b.random(b.int("1"), b.int("6"))))]);
    let n = b.var("n", Type::Int);
    let main = b.main(vec![
        b.declare(&n, Some(b.random(b.int("1"), b.int("6")))),
        b.ask(Some(b.str("Number? ")), &n, AskMode::KeepAsking),
        b.print(vec![b.get(&n)]),
    ]);
    let mut program = b.program(vec![main]);
    program.modules.push(Module {
        id: ModuleId::new("mod_dice").unwrap(),
        name: String::from("dice"),
        items: vec![roll_def],
    });
    let options = CodegenOptions {
        helper_placement: HelperPlacement::Header,
        ..options("Dice")
    };
    check_with("two_modules_header", &program, &options);
}
