//! Expression slots (spec §3.4): parsing, precedence, origins, errors, limits.

// Test code: unwrap/expect/panic and printing to stderr are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use b2c_ir::diag::Part;
use b2c_ir::sast::{BinaryOp, Expr, ExprKind, ItemKind, StmtKind, UnaryOp};
use b2c_ir::types::Type;
use b2c_lang::Analysis;
use b2c_model::limits::{MAX_EXPR_DEPTH, MAX_EXPR_TOKENS};
use common::*;
use serde_json::json;

fn vars() -> Vec<serde_json::Value> {
    vec![
        declare("int", "a", Some(num("1"))),
        declare("int", "b", Some(num("2"))),
        declare("int", "c", Some(num("3"))),
        declare("bool", "p", Some(boolean(true))),
        declare("bool", "q", Some(boolean(false))),
    ]
}

fn analyse(ty: &str, src: &str) -> Analysis {
    let mut body = vars();
    body.push(declare(ty, "r", Some(e(src))));
    run_main(body)
}

fn value(a: &Analysis) -> &Expr {
    let ItemKind::Main(main) = &a.program.modules[0].items[0].kind else {
        panic!()
    };
    let StmtKind::VarDecl(decl) = &main.body.stmts[5].kind else {
        panic!()
    };
    decl.init.as_ref().expect("init")
}

/// Renders an expression with full parentheses.
fn show(expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Int(n) | ExprKind::Float(n) => n.as_str().to_owned(),
        ExprKind::Bool(b) => b.to_string(),
        ExprKind::Str(s) => format!("{:?}", s.value()),
        ExprKind::Char(c) => format!("'{}'", c.value()),
        ExprKind::Var(s) => s.as_str().trim_start_matches("sym_").to_owned(),
        ExprKind::Unary { op, operand } => {
            let op = match op {
                UnaryOp::Neg => "-",
                UnaryOp::Plus => "+",
                UnaryOp::Not => "!",
            };
            format!("({op}{})", show(operand))
        }
        ExprKind::Binary { op, lhs, rhs } => {
            let op = match op {
                BinaryOp::Add => "+",
                BinaryOp::Sub => "-",
                BinaryOp::Mul => "*",
                BinaryOp::Div => "/",
                BinaryOp::Mod => "%",
                BinaryOp::Lt => "<",
                BinaryOp::Le => "<=",
                BinaryOp::Gt => ">",
                BinaryOp::Ge => ">=",
                BinaryOp::Eq => "==",
                BinaryOp::Ne => "!=",
                BinaryOp::And => "&&",
                BinaryOp::Or => "||",
            };
            format!("({} {op} {})", show(lhs), show(rhs))
        }
        ExprKind::Conditional {
            cond,
            then_value,
            else_value,
        } => {
            format!("({} ? {} : {})", show(cond), show(then_value), show(else_value))
        }
        ExprKind::Call { function, args } => format!(
            "{}({})",
            function.as_str().trim_start_matches("sym_"),
            args.iter().map(show).collect::<Vec<_>>().join(", ")
        ),
        other => format!("{other:?}"),
    }
}

#[track_caller]
fn parses_as(ty: &str, src: &str, expected: &str) {
    let a = analyse(ty, src);
    assert_clean(&a);
    assert_eq!(show(value(&a)), expected, "{src}");
}

#[test]
fn precedence_and_associativity() {
    parses_as("int", "a + b * c", "(a + (b * c))");
    parses_as("int", "a * b + c", "((a * b) + c)");
    parses_as("int", "a - b - c", "((a - b) - c)");
    parses_as("int", "a / b / c", "((a / b) / c)");
    parses_as("int", "a % b * c", "((a % b) * c)");
    parses_as("int", "(a + b) * c", "((a + b) * c)");
    parses_as("int", "a - (b - c)", "(a - (b - c))");
    parses_as("int", "-a * -b", "((-a) * (-b))");
    parses_as("int", "- - a", "(-(-a))");
    parses_as("int", "+a", "(+a)");
    parses_as("bool", "a + 1 < b * 2", "((a + 1) < (b * 2))");
    parses_as("bool", "a < b == b < c", "((a < b) == (b < c))");
    parses_as("bool", "p || q && p", "(p || (q && p))");
    parses_as("bool", "p and q or not p", "((p && q) || (!p))");
    parses_as("bool", "!p == q", "((!p) == q)");
    parses_as(
        "bool",
        "a != b && b <= c || c >= a",
        "(((a != b) && (b <= c)) || (c >= a))",
    );
    parses_as("int", "p ? a : q ? b : c", "(p ? a : (q ? b : c))");
    parses_as("int", "p ? q ? a : b : c", "(p ? (q ? a : b) : c)");
    parses_as("int", "p || q ? a + 1 : b", "((p || q) ? (a + 1) : b)");
    parses_as("double", "1.5 * a", "(1.5 * a)");
    parses_as("std::string", "p ? \"yes\" : \"no\"", "(p ? \"yes\" : \"no\")");
    parses_as("char", "'x'", "'x'");
}

#[test]
fn origins_cover_the_tokens() {
    let a = analyse("int", "a + (b * c)");
    let v = value(&a);
    let part = |start, end| Part::Tokens {
        input: String::from("VALUE"),
        start,
        end,
    };
    assert_eq!(v.origin.part, part(0, 7));
    let ExprKind::Binary { lhs, rhs, .. } = &v.kind else {
        panic!()
    };
    assert_eq!(lhs.origin.part, part(0, 1));
    assert_eq!(
        rhs.origin.part,
        part(2, 7),
        "parentheses belong to the sub-expression"
    );
    let ExprKind::Binary { lhs: b, rhs: c, .. } = &rhs.kind else {
        panic!()
    };
    assert_eq!((&b.origin.part, &c.origin.part), (&part(3, 4), &part(5, 6)));
    // Every node of a slot points at the block that holds the slot.
    let ItemKind::Main(main_def) = &a.program.modules[0].items[0].kind else {
        panic!()
    };
    let declaring_block = &main_def.body.stmts[5].origin.block;
    assert!(
        [v, lhs, rhs, b, c]
            .iter()
            .all(|x| &x.origin.block == declaring_block)
    );
}

#[test]
fn calls_in_slots() {
    let a = run(vec![
        main(vec![declare(
            "int",
            "x",
            Some(e("twice(1 + 2) + add(twice(3), 4) * none()")),
        )]),
        func(
            "twice",
            "int",
            &[("n", "int", "copy")],
            vec![ret(Some(e("n * 2")))],
        ),
        func(
            "add",
            "int",
            &[("l", "int", "copy"), ("r", "int", "copy")],
            vec![ret(Some(e("l + r")))],
        ),
        func("none", "int", &[], vec![ret(Some(num("0")))]),
    ]);
    assert_clean(&a);
    let ItemKind::Main(main_def) = &a.program.modules[0].items[3].kind else {
        panic!()
    };
    let StmtKind::VarDecl(decl) = &main_def.body.stmts[0].kind else {
        panic!()
    };
    assert_eq!(
        show(decl.init.as_ref().expect("init")),
        "(twice((1 + 2)) + (add(twice(3), 4) * none()))"
    );

    // Argument checks apply to slot calls too.
    let a = run(vec![
        main(vec![print(vec![e("twice()")])]),
        func(
            "twice",
            "int",
            &[("n", "int", "copy")],
            vec![ret(Some(e("n * 2")))],
        ),
    ]);
    assert_codes(&a, &["B2C-E0306"]);
    // The callee's problems point at the callee token.
    let a = run_main(vec![print(vec![e("1 + nothing(2)")])]);
    let d = only(&a, "B2C-E0201");
    assert_eq!(
        d.primary.part,
        Part::Tokens {
            input: String::from("ITEM0"),
            start: 2,
            end: 3
        }
    );
}

#[track_caller]
fn syntax_error(src: &str, code: &str, range: (u32, u32), needle: &str) {
    let a = analyse("int", src);
    let d = only(&a, code);
    assert_eq!(
        d.primary.part,
        Part::Tokens {
            input: String::from("VALUE"),
            start: range.0,
            end: range.1
        },
        "{src}: {}",
        d.message
    );
    assert!(d.message.contains(needle), "{src}: {}", d.message);
    assert_eq!(a.diagnostics.len(), 1, "{src}: {}", render_all(&a));
    assert_eq!(value(&a).ty, Type::Error);
}

#[test]
fn parse_errors() {
    syntax_error("a +", "B2C-E0440", (1, 2), "missing after `+`");
    syntax_error("a b", "B2C-E0440", (1, 2), "`b` can't follow the value before it");
    syntax_error("(a + b", "B2C-E0440", (0, 1), "never closed");
    syntax_error("a + b)", "B2C-E0440", (3, 4), "no matching `(`");
    syntax_error("p ? a", "B2C-E0440", (1, 2), "needs a matching `:`");
    syntax_error("a : b", "B2C-E0440", (1, 2), "no matching `?`");
    syntax_error(
        "a , b",
        "B2C-E0440",
        (1, 2),
        "separate the values given to a function",
    );
    syntax_error("* a", "B2C-E0440", (0, 1), "A value is missing before `*`");
    syntax_error("5 (3)", "B2C-E0440", (1, 2), "Only functions can be called");
    syntax_error("a = b", "B2C-E0440", (1, 2), "'set' block");
    syntax_error("a += 1", "B2C-E0440", (1, 2), "'change' block");
    syntax_error("a << 1", "B2C-E0440", (1, 2), "bit operator");
    syntax_error("a . b", "B2C-E0440", (1, 2), "can't be used in expressions yet");
    syntax_error(
        "nullptr",
        "B2C-E0440",
        (0, 1),
        "only keywords allowed are `true` and `false`",
    );
    syntax_error("a + \"x\" \"y\"", "B2C-E0440", (3, 4), "the text \"y\"");
    syntax_error("a + $foo", "B2C-E0441", (2, 3), "`foo` isn't understood here");
}

#[test]
fn drafts_and_empty_slots() {
    let mut body = vars();
    body.push(declare("int", "r", Some(draft("a +"))));
    let a = run_main(body);
    let d = only(&a, "B2C-E0441");
    assert!(d.message.contains("isn't finished"), "{}", d.message);
    assert_eq!(
        d.primary.part,
        Part::Tokens {
            input: String::from("VALUE"),
            start: 0,
            end: 2
        }
    );

    // An empty slot is a missing value (or no value where that is allowed).
    let a = run_main(vec![print(vec![json!({"expr": []})])]);
    let d = only(&a, "B2C-E0430");
    assert_eq!(
        d.primary.part,
        Part::Input {
            name: String::from("ITEM0")
        }
    );
    let a = run_main(vec![
        declare("int", "x", Some(json!({"expr": []}))),
        set("x", num("1")),
    ]);
    assert_clean(&a);
}

#[test]
fn limits() {
    // Too many tokens.
    let mut src = vec!["1"; MAX_EXPR_TOKENS / 2 + 1].join(" + ");
    let a = analyse("int", &src);
    let d = only(&a, "B2C-E0442");
    assert!(d.message.contains("too long"), "{}", d.message);
    // Just under the limit is fine when the expression is not too deep:
    // 32 groups of 15 tokens, joined by 31 `+`.
    let group = format!("({})", ["1"; 7].join(" + "));
    src = vec![group; 32].join(" + ");
    assert_eq!(tokens(&src).len(), MAX_EXPR_TOKENS - 1);
    assert_clean(&analyse("int", &src));

    // A flat chain is one level deeper per operator: 64 operators are fine,
    // the 65th is reported.
    src = vec!["1"; MAX_EXPR_DEPTH + 1].join(" + ");
    assert_clean(&analyse("int", &src));
    src = vec!["1"; MAX_EXPR_DEPTH + 2].join(" + ");
    let a = analyse("int", &src);
    let d = only(&a, "B2C-E0443");
    let at = u32::try_from(2 * MAX_EXPR_DEPTH + 1).expect("small");
    assert_eq!(
        d.primary.part,
        Part::Tokens {
            input: String::from("VALUE"),
            start: at,
            end: at + 1
        }
    );
    assert!(
        d.message.contains("each operator counts as a level"),
        "{}",
        d.message
    );

    // Too deep.
    let deep = format!(
        "{}a{}",
        "(".repeat(MAX_EXPR_DEPTH + 1),
        ")".repeat(MAX_EXPR_DEPTH + 1)
    );
    let a = analyse("int", &deep);
    let d = only(&a, "B2C-E0443");
    assert!(d.message.contains("more than 64 levels"), "{}", d.message);
    let ok = format!("{}a{}", "(".repeat(MAX_EXPR_DEPTH), ")".repeat(MAX_EXPR_DEPTH));
    assert_clean(&analyse("int", &ok));
    let negations = format!("{}a", "- ".repeat(MAX_EXPR_DEPTH + 1));
    only(&analyse("int", &negations), "B2C-E0443");
}

#[test]
fn literal_tokens() {
    parses_as("int", "0x2A + 0b101", "(42 + 5)");
    parses_as("double", "1e3 + .5", "(1000.0 + 0.5)");
    parses_as("int", "1'000", "1000");
    let a = analyse("int", "0x");
    only(&a, "B2C-E0310");
    let a = analyse("std::string", "\"bad\u{0}\"");
    only(&a, "B2C-E0312");
}

#[test]
fn every_slot_input_is_parsed() {
    // A slot in every kind of input position, with the catalog defaults.
    let a = run(vec![
        main(vec![
            declare("int", "n", Some(e("1"))),
            set("n", e("n + 1")),
            change("n", e("2")),
            update("n", "mul", e("3")),
            if_else(vec![(e("n > 1"), vec![]), (e("n > 2"), vec![])], None),
            while_loop("while", e("n < 0"), vec![]),
            repeat(e("n"), vec![]),
            for_range("i", e("0"), "to", e("n"), Some(e("1")), vec![]),
            print(vec![e("n"), e("\"x\""), e("'y'")]),
            ask("n", Some(e("\"? \""))),
            call_stmt("f", vec![e("n * 2")]),
            exit(e("0")),
        ]),
        func("f", "int", &[("v", "int", "copy")], vec![ret(Some(e("v")))]),
    ]);
    assert_clean(&a);
}
