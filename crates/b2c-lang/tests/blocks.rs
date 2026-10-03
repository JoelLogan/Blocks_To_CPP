//! Lowering of every M1 block type to the Semantic AST.

// Test code: unwrap/expect/panic and printing to stderr are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use b2c_ir::diag::Part;
use b2c_ir::sast::{
    AskMode, BinaryOp, CompoundOp, CppStandard, Expr, ExprKind, ItemKind, OutputStream, PassMode,
    PrintSeparator, RangeDirection, Stmt, StmtKind, SymbolKind, UnaryOp,
};
use b2c_ir::types::Type;
use b2c_lang::{Analysis, analyze};
use common::*;
use serde_json::json;

fn main_stmts(a: &Analysis) -> &[Stmt] {
    for item in &a.program.modules[0].items {
        if let ItemKind::Main(main) = &item.kind {
            return &main.body.stmts;
        }
    }
    panic!("no main")
}

/// The initial value of the `n`th declaration in main.
fn init(a: &Analysis, n: usize) -> &Expr {
    match &main_stmts(a)[n].kind {
        StmtKind::VarDecl(decl) => decl.init.as_ref().expect("init"),
        other => panic!("not a declaration: {other:?}"),
    }
}

fn sym_ty(a: &Analysis, name: &str) -> Type {
    let id = b2c_ir::ids::SymbolId::new(&sym(name)).expect("id");
    a.program.symbols.get(&id).expect("symbol").ty.clone()
}

#[test]
fn empty_main_and_standard() {
    let a = run_main(vec![]);
    assert_clean(&a);
    assert_eq!(a.program.standard, CppStandard::Cpp20);
    assert_eq!(a.program.modules.len(), 1);
    assert_eq!(a.program.modules[0].name, "main");
    let item = &a.program.modules[0].items[0];
    assert!(matches!(item.kind, ItemKind::Main(_)));
    assert_eq!(item.origin.block.as_str(), "main");
    assert_eq!(item.origin.part, Part::Whole);
}

#[test]
fn declarations() {
    let a = run_main(vec![
        declare("int", "a", Some(num("1"))),
        declare("double", "b", Some(num("2.5"))),
        declare("bool", "c", Some(boolean(true))),
        declare("char", "d", Some(chr("x"))),
        declare("std::string", "e", Some(text("hi"))),
        declare("auto", "f", Some(num("1e3"))),
        declare("int", "g", None),
        declare_const("int", "h", num("7")),
    ]);
    assert_clean(&a);
    let stmts = main_stmts(&a);
    assert_eq!(stmts.len(), 8);
    for (name, ty) in [
        ("a", Type::Int),
        ("b", Type::Double),
        ("c", Type::Bool),
        ("d", Type::Char),
        ("e", Type::String),
        ("f", Type::Double),
        ("g", Type::Int),
        ("h", Type::Int),
    ] {
        assert_eq!(sym_ty(&a, name), ty, "{name}");
    }
    let StmtKind::VarDecl(f) = &stmts[5].kind else {
        panic!()
    };
    assert!(f.written_auto && !f.is_const);
    let StmtKind::VarDecl(g) = &stmts[6].kind else {
        panic!()
    };
    assert!(g.init.is_none());
    let StmtKind::VarDecl(h) = &stmts[7].kind else {
        panic!()
    };
    assert!(h.is_const && !h.written_auto);
    let id = b2c_ir::ids::SymbolId::new("sym_h").expect("id");
    let symbol = a.program.symbols.get(&id).expect("symbol");
    assert_eq!(symbol.name.as_str(), "h");
    assert_eq!(symbol.kind, SymbolKind::Variable { is_const: true });
    assert_eq!(
        symbol.origin.part,
        Part::Field {
            name: String::from("NAME")
        }
    );
    // Statements carry the whole block as origin.
    assert_eq!(stmts[0].origin.part, Part::Whole);
}

#[test]
fn literals() {
    let a = run_main(vec![
        declare("int", "a", Some(num("42"))),
        declare("int", "b", Some(num("-5"))),
        declare("double", "c", Some(num("0.5"))),
        declare("int", "d", Some(num("0x1E"))),
        declare("std::string", "e", Some(text("a\"b"))),
        declare("char", "f", Some(chr("'"))),
        declare("bool", "g", Some(boolean(false))),
        declare("int", "h", Some(num(" 7 "))),
        declare("double", "i", Some(num("1e-3"))),
    ]);
    assert_clean(&a);
    assert!(matches!(&init(&a, 0).kind, ExprKind::Int(n) if n.as_str() == "42"));
    match &init(&a, 1).kind {
        ExprKind::Unary {
            op: UnaryOp::Neg,
            operand,
        } => {
            assert!(matches!(&operand.kind, ExprKind::Int(n) if n.as_str() == "5"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(init(&a, 1).ty, Type::Int);
    assert!(matches!(&init(&a, 2).kind, ExprKind::Float(n) if n.as_str() == "0.5"));
    assert!(
        matches!(&init(&a, 3).kind, ExprKind::Int(n) if n.as_str() == "30"),
        "hex with E is an int"
    );
    assert!(matches!(&init(&a, 4).kind, ExprKind::Str(s) if s.value() == "a\"b"));
    assert!(matches!(&init(&a, 5).kind, ExprKind::Char(c) if c.value() == '\''));
    assert!(matches!(&init(&a, 6).kind, ExprKind::Bool(false)));
    assert!(matches!(&init(&a, 7).kind, ExprKind::Int(n) if n.as_str() == "7"));
    assert_eq!(init(&a, 8).ty, Type::Double);
}

#[test]
fn set_change_update() {
    let a = run_main(vec![
        declare("int", "x", Some(num("0"))),
        set("x", num("3")),
        change("x", num("1")),
        update("x", "add", num("2")),
        update("x", "sub", num("2")),
        update("x", "mul", num("2")),
        update("x", "div", num("2")),
        update("x", "mod", num("2")),
    ]);
    assert_clean(&a);
    let stmts = main_stmts(&a);
    assert!(matches!(&stmts[1].kind, StmtKind::Assign { target, .. } if target.as_str() == "sym_x"));
    let ops: Vec<CompoundOp> = stmts[2..]
        .iter()
        .map(|s| match &s.kind {
            StmtKind::CompoundAssign { op, .. } => *op,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        ops,
        [
            CompoundOp::Add,
            CompoundOp::Add,
            CompoundOp::Sub,
            CompoundOp::Mul,
            CompoundOp::Div,
            CompoundOp::Mod
        ]
    );
}

#[test]
fn arithmetic_and_comparison_blocks() {
    let mut body = vec![
        declare("int", "x", Some(num("1"))),
        declare("double", "y", Some(num("1.5"))),
    ];
    for op in ["add", "sub", "mul", "div", "mod"] {
        body.push(declare(
            "int",
            &format!("i_{op}"),
            Some(arith(op, get("x"), num("2"))),
        ));
    }
    for op in ["lt", "le", "gt", "ge", "eq", "ne"] {
        body.push(declare(
            "bool",
            &format!("c_{op}"),
            Some(compare(op, get("x"), num("2"))),
        ));
    }
    body.push(declare("double", "mixed", Some(arith("mul", get("x"), get("y")))));
    let a = run_main(body);
    assert_clean(&a);
    let expected = [
        BinaryOp::Add,
        BinaryOp::Sub,
        BinaryOp::Mul,
        BinaryOp::Div,
        BinaryOp::Mod,
        BinaryOp::Lt,
        BinaryOp::Le,
        BinaryOp::Gt,
        BinaryOp::Ge,
        BinaryOp::Eq,
        BinaryOp::Ne,
    ];
    for (i, op) in expected.iter().enumerate() {
        let value = init(&a, i + 2);
        assert!(
            matches!(&value.kind, ExprKind::Binary { op: o, .. } if o == op),
            "{op:?}"
        );
        let ty = if i < 5 { Type::Int } else { Type::Bool };
        assert_eq!(value.ty, ty);
        assert_eq!(value.origin.part, Part::Whole);
    }
    assert_eq!(init(&a, 13).ty, Type::Double);
}

#[test]
fn random_and_convert() {
    let a = run_main(vec![
        declare("int", "r", Some(random_int(num("1"), num("6")))),
        declare("int", "i", Some(convert("int", num("2.7")))),
        declare("double", "d", Some(convert("double", num("2")))),
    ]);
    assert_clean(&a);
    assert!(matches!(&init(&a, 0).kind, ExprKind::RandomInt { .. }));
    assert_eq!(init(&a, 0).ty, Type::Int);
    assert!(matches!(
        &init(&a, 1).kind,
        ExprKind::Convert { to: Type::Int, .. }
    ));
    assert_eq!(init(&a, 2).ty, Type::Double);
}

#[test]
fn logic_blocks() {
    let a = run_main(vec![
        declare("bool", "p", Some(boolean(true))),
        declare(
            "bool",
            "q",
            Some(logic("and", vec![get("p"), boolean(false), get("p")])),
        ),
        declare("bool", "r", Some(logic("or", vec![get("p"), get("q")]))),
        declare("bool", "s", Some(not(get("p")))),
        declare("int", "t", Some(ternary(get("p"), num("1"), num("2")))),
    ]);
    assert_clean(&a);
    // (p && false) && p: left-associative.
    match &init(&a, 1).kind {
        ExprKind::Binary {
            op: BinaryOp::And,
            lhs,
            rhs,
        } => {
            assert!(matches!(
                &lhs.kind,
                ExprKind::Binary {
                    op: BinaryOp::And,
                    ..
                }
            ));
            assert!(matches!(&rhs.kind, ExprKind::Var(_)));
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        &init(&a, 2).kind,
        ExprKind::Binary { op: BinaryOp::Or, .. }
    ));
    assert!(matches!(
        &init(&a, 3).kind,
        ExprKind::Unary { op: UnaryOp::Not, .. }
    ));
    assert!(matches!(&init(&a, 4).kind, ExprKind::Conditional { .. }));
    assert_eq!(init(&a, 4).ty, Type::Int);
}

#[test]
fn text_join() {
    let a = run_main(vec![
        declare("int", "n", Some(num("3"))),
        declare(
            "std::string",
            "s",
            Some(join(vec![text("n = "), get("n"), chr("!"), boolean(true)])),
        ),
    ]);
    assert_clean(&a);
    match &init(&a, 1).kind {
        ExprKind::Join(items) => assert_eq!(items.len(), 4),
        other => panic!("{other:?}"),
    }
    assert_eq!(init(&a, 1).ty, Type::String);
}

#[test]
fn control_flow_blocks() {
    let a = run_main(vec![
        declare("int", "x", Some(num("1"))),
        if_else(
            vec![
                (e("x < 1"), vec![print(vec![text("a")])]),
                (e("x < 2"), vec![print(vec![text("b")])]),
            ],
            Some(vec![print(vec![text("c")])]),
        ),
        if_then(e("x == 1"), vec![]),
        while_loop("while", e("x < 10"), vec![change("x", num("1"))]),
        while_loop("until", e("x >= 20"), vec![change("x", num("1")), cont()]),
        repeat(num("3"), vec![brk()]),
        for_range("i", num("0"), "to", num("10"), None, vec![print(vec![get("i")])]),
        for_range("j", num("0"), "through", num("10"), Some(num("2")), vec![]),
        for_range("k", num("10"), "down_to", num("0"), None, vec![]),
        forever(vec![brk()]),
    ]);
    assert_clean(&a);
    let stmts = main_stmts(&a);
    match &stmts[1].kind {
        StmtKind::If { branches, else_body } => {
            assert_eq!(branches.len(), 2);
            assert!(else_body.is_some());
            assert_eq!(
                branches[1].cond.origin.part,
                Part::Tokens {
                    input: String::from("COND1"),
                    start: 0,
                    end: 3
                }
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(&stmts[2].kind, StmtKind::If { else_body: None, .. }));
    assert!(matches!(&stmts[3].kind, StmtKind::While { until: false, .. }));
    assert!(matches!(&stmts[4].kind, StmtKind::While { until: true, .. }));
    assert!(
        matches!(&stmts[5].kind, StmtKind::Repeat { body, .. } if matches!(body.stmts[0].kind, StmtKind::Break))
    );
    let directions: Vec<(RangeDirection, bool)> = stmts[6..9]
        .iter()
        .map(|s| match &s.kind {
            StmtKind::ForRange { direction, step, .. } => (*direction, step.is_some()),
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        directions,
        [
            (RangeDirection::UpExclusive, false),
            (RangeDirection::UpInclusive, true),
            (RangeDirection::DownInclusive, false)
        ]
    );
    assert!(matches!(&stmts[9].kind, StmtKind::Forever { .. }));
    let i = b2c_ir::ids::SymbolId::new("sym_i").expect("id");
    let counter = a.program.symbols.get(&i).expect("counter");
    assert_eq!(
        (&counter.kind, &counter.ty),
        (&SymbolKind::LoopVariable, &Type::Int)
    );
    assert_eq!(
        counter.origin.part,
        Part::Field {
            name: String::from("VAR")
        }
    );
}

#[test]
fn print_and_ask() {
    let mut fancy = print(vec![text("a"), num("1")]);
    fancy["fields"] = json!({"SEP": "comma", "NEWLINE": false, "STREAM": "err"});
    let mut simple = ask("n", None);
    simple["fields"]["MODE"] = json!("simple");
    let a = run_main(vec![
        declare("int", "n", Some(num("0"))),
        declare("std::string", "name", Some(text(""))),
        print(vec![text("Hello")]),
        fancy,
        ask("name", Some(text("Name? "))),
        ask("n", Some(chr("?"))),
        simple,
    ]);
    assert_clean(&a);
    let stmts = main_stmts(&a);
    assert!(matches!(
        &stmts[2].kind,
        StmtKind::Print { items, separator: PrintSeparator::None, newline: true, stream: OutputStream::Out } if items.len() == 1
    ));
    assert!(matches!(
        &stmts[3].kind,
        StmtKind::Print { items, separator: PrintSeparator::Comma, newline: false, stream: OutputStream::Err } if items.len() == 2
    ));
    assert!(matches!(
        &stmts[4].kind,
        StmtKind::Ask {
            prompt: Some(_),
            mode: AskMode::KeepAsking,
            ..
        }
    ));
    assert!(matches!(
        &stmts[6].kind,
        StmtKind::Ask {
            prompt: None,
            mode: AskMode::Simple,
            ..
        }
    ));
}

#[test]
fn functions_calls_and_returns() {
    let a = run(vec![
        main(vec![
            declare("int", "total", Some(num("0"))),
            call_stmt("greet", vec![text("Ada")]),
            declare("int", "sq", Some(call("square", vec![num("4")]))),
            call_stmt("square", vec![num("2")]),
            call_stmt("add_to", vec![get("total"), num("5")]),
            declare("int", "via_slot", Some(e("square(sq) + 1"))),
        ]),
        func(
            "greet",
            "void",
            &[("who", "std::string", "read_only")],
            vec![print(vec![get("who")]), ret(None)],
        ),
        func(
            "square",
            "int",
            &[("v", "int", "copy")],
            vec![ret(Some(e("v * v")))],
        ),
        func(
            "add_to",
            "void",
            &[("acc", "int", "editable"), ("amount", "int", "copy")],
            vec![change("acc", get("amount"))],
        ),
    ]);
    assert_clean(&a);
    let items = &a.program.modules[0].items;
    // Items are sorted by block ID: fn_add_to, fn_greet, fn_square, main.
    let ids: Vec<&str> = items.iter().map(|i| i.origin.block.as_str()).collect();
    assert_eq!(ids, ["fn_add_to", "fn_greet", "fn_square", "main"]);
    let ItemKind::Function(add_to) = &items[0].kind else {
        panic!()
    };
    assert_eq!(add_to.ret, Type::Void);
    assert_eq!(add_to.params.len(), 2);
    assert_eq!(add_to.params[0].mode, PassMode::Editable);
    assert_eq!(
        add_to.params[0].origin.part,
        Part::Field {
            name: String::from("params[0]")
        }
    );
    let ItemKind::Function(square) = &items[2].kind else {
        panic!()
    };
    assert_eq!(square.ret, Type::Int);
    let fn_sym = a.program.symbols.get(&square.symbol).expect("function symbol");
    assert!(matches!(&fn_sym.kind, SymbolKind::Function { params } if params.len() == 1));
    assert_eq!(fn_sym.ty, Type::Int);

    let stmts = main_stmts(&a);
    assert!(matches!(&stmts[1].kind, StmtKind::Eval { expr } if expr.ty == Type::Void));
    assert!(matches!(&stmts[3].kind, StmtKind::Eval { expr } if expr.ty == Type::Int));
    match &init(&a, 2).kind {
        ExprKind::Call { function, args } => {
            assert_eq!(function.as_str(), "sym_square");
            assert_eq!(args.len(), 1);
        }
        other => panic!("{other:?}"),
    }
    match &init(&a, 5).kind {
        ExprKind::Binary { lhs, .. } => {
            assert!(matches!(&lhs.kind, ExprKind::Call { .. }));
            assert_eq!(
                lhs.origin.part,
                Part::Tokens {
                    input: String::from("VALUE"),
                    start: 0,
                    end: 4
                }
            );
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn exit_in_main_and_in_function() {
    let a = run(vec![
        main(vec![exit(num("0"))]),
        func("bail", "void", &[], vec![exit(num("2"))]),
    ]);
    assert_clean(&a);
    let stmts = main_stmts(&a);
    assert!(matches!(
        &stmts[0].kind,
        StmtKind::Exit {
            code: Some(_),
            in_main: true
        }
    ));
    let ItemKind::Function(bail) = &a.program.modules[0].items[0].kind else {
        panic!()
    };
    assert!(matches!(
        &bail.body.stmts[0].kind,
        StmtKind::Exit { in_main: false, .. }
    ));

    // `stop program` deep inside main is still in main.
    let a = run_main(vec![forever(vec![if_then(e("true"), vec![exit(num("1"))])])]);
    assert_clean(&a);
    let StmtKind::Forever { body } = &main_stmts(&a)[0].kind else {
        panic!()
    };
    let StmtKind::If { branches, .. } = &body.stmts[0].kind else {
        panic!()
    };
    assert!(matches!(
        &branches[0].body.stmts[0].kind,
        StmtKind::Exit { in_main: true, .. }
    ));

    // A missing exit code means 0.
    let a = run_main(vec![json!({"id": "x", "type": "program.exit", "v": 1})]);
    assert_clean(&a);
    assert!(matches!(
        &main_stmts(&a)[0].kind,
        StmtKind::Exit {
            code: None,
            in_main: true
        }
    ));
}

#[test]
fn return_in_main_with_value_ends_program() {
    let a = run_main(vec![ret(Some(num("3")))]);
    assert_clean(&a);
    assert!(matches!(
        &main_stmts(&a)[0].kind,
        StmtKind::Return { value: Some(_) }
    ));
}

#[test]
fn comments_are_attached() {
    let mut stmt = print(vec![text("x")]);
    stmt["comment"] = json!({"text": "Say hello"});
    let mut empty = print(vec![text("y")]);
    empty["comment"] = json!({"text": "   "});
    let mut m = main(vec![stmt, empty]);
    m["comment"] = json!({"text": "Entry point", "pinned": true});
    let a = run(vec![m]);
    assert_clean(&a);
    let item = &a.program.modules[0].items[0];
    assert_eq!(
        item.comment.as_ref().map(b2c_ir::text::Comment::text),
        Some("Entry point")
    );
    let stmts = main_stmts(&a);
    assert_eq!(
        stmts[0].comment.as_ref().map(b2c_ir::text::Comment::text),
        Some("Say hello")
    );
    assert!(stmts[1].comment.is_none(), "blank comments are dropped");
}

#[test]
fn too_long_comment_is_dropped_with_warning() {
    let mut stmt = print(vec![text("x")]);
    stmt["comment"] = json!({"text": "x".repeat(b2c_ir::text::MAX_TEXT_LEN + 1)});
    let a = run_main(vec![stmt]);
    assert_codes(&a, &["B2C-W0521"]);
    assert!(main_stmts(&a)[0].comment.is_none());
}

#[test]
fn disabled_blocks_are_dropped() {
    let mut off = print(vec![text("no")]);
    off["disabled"] = json!(true);
    let mut off_main = main(vec![]);
    off_main["id"] = json!("other_main");
    off_main["disabled"] = json!(true);
    let mut off_fn = func("unused", "void", &[], vec![]);
    off_fn["disabled"] = json!(true);
    let a = run(vec![main(vec![print(vec![text("yes")]), off]), off_main, off_fn]);
    assert_clean(&a);
    assert_eq!(main_stmts(&a).len(), 1);
    assert_eq!(a.program.modules[0].items.len(), 1);
}

#[test]
fn unknown_blocks_are_skipped_silently() {
    let a = run(vec![
        main(vec![
            json!({"id": "u1", "type": "pack.unknown", "v": 1}),
            json!({"id": "u2", "type": "math.number", "v": 1}),
            declare(
                "int",
                "x",
                Some(input(json!({"id": "u3", "type": "pack.thing", "v": 1}))),
            ),
        ]),
        json!({"id": "loose", "type": "io.print", "v": 1}),
    ]);
    assert_clean(&a);
    assert_eq!(main_stmts(&a).len(), 1);
    assert_eq!(init(&a, 0).ty, Type::Error);
}

#[test]
fn modules_and_items_in_order() {
    let d = doc_modules(vec![
        (
            "main",
            vec![
                func("zeta", "void", &[], vec![]),
                main(vec![]),
                func("alpha", "void", &[], vec![]),
            ],
        ),
        ("util", vec![func("helper", "void", &[], vec![])]),
    ]);
    let a = analyze(&d);
    assert_clean(&a);
    let names: Vec<&str> = a.program.modules.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(names, ["main", "util"]);
    let ids: Vec<&str> = a.program.modules[0]
        .items
        .iter()
        .map(|i| i.origin.block.as_str())
        .collect();
    assert_eq!(ids, ["fn_alpha", "fn_zeta", "main"]);
    assert_eq!(a.program.modules[1].items.len(), 1);
}

#[test]
fn standard_comes_from_the_project() {
    let mut d = doc(vec![main(vec![])]);
    d.project.language.standard = CppStandard::Cpp17;
    assert_eq!(analyze(&d).program.standard, CppStandard::Cpp17);
}
