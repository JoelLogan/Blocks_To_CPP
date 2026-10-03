//! Robustness: `generate` never panics, whatever the program, and still
//! produces well-formed files and source maps (best-effort output with
//! `0 /* error */` and `/* error */;` placeholders).

use b2c_codegen::{CodegenOptions, generate};
use b2c_ir::diag::Part;
use b2c_ir::ids::{BlockId, ModuleId, SymbolId};
use b2c_ir::sast::{
    AskMode, BinaryOp, Block, CompoundOp, CppStandard, Expr, ExprKind, FunctionDef, IfBranch, Item, ItemKind,
    MainDef, Module, Origin, OutputStream, Param, PassMode, PrintSeparator, Program, RangeDirection, Stmt,
    StmtKind, SymbolTable, UnaryOp, VarDecl,
};
use b2c_ir::text::{CharLit, Comment, NumLit, NumType, StrLit};
use b2c_ir::types::Type;
use proptest::prelude::*;

use crate::builder::{Builder, block};
use crate::{assert_style, generate_checked, options, render, source_map};

fn missing() -> SymbolId {
    SymbolId::new("sym_missing").unwrap()
}

#[test]
fn dangling_symbols_become_error_placeholders() {
    let b = Builder::new();
    let x = b.var("x", Type::Int);
    let f = b.function("f", Type::Int, &[("a", Type::Int, PassMode::Copy)]);
    let typed = |kind, ty| b.expr(kind, ty);
    let main = b.main(vec![
        b.declare(&missing(), Some(b.int("1"))),
        b.declare(&x, Some(typed(ExprKind::Var(missing()), Type::Int))),
        b.set(&missing(), b.int("2")),
        b.stmt(StmtKind::CompoundAssign {
            target: missing(),
            op: CompoundOp::Add,
            value: b.int("1"),
        }),
        b.ask(None, &missing(), AskMode::KeepAsking),
        b.for_range(
            &missing(),
            b.int("0"),
            b.int("3"),
            None,
            RangeDirection::UpExclusive,
            Vec::new(),
        ),
        b.print(vec![typed(
            ExprKind::Call {
                function: missing(),
                args: vec![b.int("1")],
            },
            Type::Int,
        )]),
        // A variable used as a function, and a function used as a variable.
        b.print(vec![typed(
            ExprKind::Call {
                function: x.clone(),
                args: Vec::new(),
            },
            Type::Int,
        )]),
        b.print(vec![typed(ExprKind::Var(f.id.clone()), Type::Int)]),
        b.print(vec![b.get(&x)]),
    ]);
    // A function whose symbol is missing is left out; a missing parameter
    // symbol is skipped.
    let mut ghost = b.define(&f, Vec::new());
    if let ItemKind::Function(def) = &mut ghost.kind {
        def.symbol = missing();
    }
    let mut lost_param = b.define(&f, vec![b.ret(Some(b.int("0")))]);
    if let ItemKind::Function(def) = &mut lost_param.kind {
        def.params[0].symbol = missing();
    }
    let project = generate_checked(&b.program(vec![main, ghost, lost_param]), &options("Broken"));
    let text = render(&project);
    assert_eq!(
        text.lines().filter(|l| l.trim() == "/* error */;").count(),
        5,
        "{text}"
    );
    assert_eq!(text.matches("0 /* error */").count(), 4, "{text}");
    assert!(text.contains("int f();"), "{text}");
    assert!(text.contains("int x = 0 /* error */;"), "{text}");
}

#[test]
fn error_typed_expressions_become_error_placeholders() {
    let b = Builder::new();
    let x = b.var("x", Type::Error);
    let y = b.var("y", Type::Void);
    let s = b.var("s", Type::String);
    let broken = b.expr(
        ExprKind::Binary {
            op: BinaryOp::Add,
            lhs: Box::new(b.int("1")),
            rhs: Box::new(b.str("a")),
        },
        Type::Error,
    );
    let main = b.main(vec![
        b.declare(&x, Some(broken)),
        b.declare(&y, None),
        b.declare(&s, None),
        b.if_else(Vec::new(), None),
        b.if_else(Vec::new(), Some(vec![b.print_text("else only")])),
        b.print_with(Vec::new(), PrintSeparator::None, false, OutputStream::Out),
        b.print(vec![b.convert(Type::String, b.int("1"))]),
        b.ask(None, &y, AskMode::KeepAsking),
        b.ask(None, &y, AskMode::Simple),
        b.stmt(StmtKind::While {
            cond: b.expr(ExprKind::Bool(true), Type::Error),
            until: true,
            body: Block::default(),
        }),
        b.stmt(StmtKind::Return { value: None }),
    ]);
    let project = generate_checked(&b.program(vec![main]), &options("Broken"));
    let text = render(&project);
    assert!(text.contains("auto x = 0 /* error */;"), "{text}");
    assert!(text.contains("auto y = 0 /* error */;"), "{text}");
    assert!(text.contains("std::string s{};"), "{text}");
    assert!(
        text.contains("    {\n        std::cout << \"else only\" << '\\n';\n    }"),
        "{text}"
    );
    assert!(text.contains("std::cout << 0 /* error */ << '\\n';"), "{text}");
    assert!(text.contains("while (!0 /* error */) {"), "{text}");
    assert!(
        text.ends_with("    return 0;\n}\n"),
        "`return;` in main returns 0: {text}"
    );
    assert_eq!(text.matches("return 0;").count(), 1, "{text}");
}

/// Runs `f` on a thread with a large stack (building and dropping very deep
/// trees recurses in the test itself).
fn with_big_stack(f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn very_deep_programs_are_cut_off_instead_of_overflowing_the_stack() {
    with_big_stack(|| {
        let b = Builder::new();
        let mut expr = b.int("1");
        for _ in 0..20_000 {
            expr = b.un(UnaryOp::Neg, expr);
        }
        let mut stmt = b.print(vec![expr]);
        for _ in 0..20_000 {
            stmt = b.stmt(StmtKind::Forever {
                body: block(vec![stmt]),
            });
        }
        let main = b.main(vec![stmt]);
        let program = b.program(vec![main]);
        let project = generate(&program, &CodegenOptions::default());
        assert_style(&project);
        source_map::assert_well_formed(&project);
        assert!(project.files[0].contents.contains("/* error */;"));
    });
}

#[test]
fn odd_options_and_names_are_handled() {
    let b = Builder::new();
    let main = b.main(vec![b.print_text("hi")]);
    let mut program = b.program(vec![main]);
    program.modules[0].name = String::from("../../evil\nname");
    program.standard = CppStandard::Cpp17;
    let options = CodegenOptions {
        project_name: "x".repeat(100_000),
        app_version: String::new(),
        do_not_edit_banner: true,
        indent_width: 0,
        helper_placement: b2c_codegen::HelperPlacement::Header,
    };
    let project = generate_checked(&program, &options);
    assert_eq!(project.files[0].path, "mod_main.cpp");
    let first_line = project.files[0].contents.lines().next().unwrap();
    assert!(
        first_line.starts_with("// Generated by Blocks2Cpp from project \"xxx"),
        "{first_line}"
    );
    assert!(first_line.len() < 400);
    assert!(
        project.files[0]
            .contents
            .contains("module \"../../evil\n// name\".")
    );
    assert!(
        project.files[0]
            .contents
            .contains("\nstd::cout << \"hi\" << '\\n';\n"),
        "indent width 0"
    );
}

// ---- random programs --------------------------------------------------------

fn origin() -> Origin {
    Origin {
        module: ModuleId::new("mod_main").unwrap(),
        block: BlockId::new("blk_any").unwrap(),
        part: Part::Whole,
    }
}

/// A symbol table with a variable of each type, a parameter, a loop variable
/// and two functions; plus IDs that resolve to nothing.
fn symbols() -> (SymbolTable, Vec<SymbolId>) {
    let b = Builder::new();
    let mut ids: Vec<SymbolId> = [
        Type::Int,
        Type::Double,
        Type::Bool,
        Type::Char,
        Type::String,
        Type::Error,
        Type::Void,
    ]
    .into_iter()
    .enumerate()
    .map(|(i, ty)| b.var(&format!("v{i}"), ty))
    .collect();
    ids.push(b.loop_var("i"));
    let f = b.function("f", Type::Int, &[("p", Type::Int, PassMode::Editable)]);
    let g = b.function("g", Type::Void, &[]);
    ids.extend([f.id, g.id, f.params[0].clone(), missing()]);
    let program = b.program(Vec::new());
    (program.symbols, ids)
}

fn any_type() -> impl Strategy<Value = Type> {
    prop::sample::select(vec![
        Type::Void,
        Type::Bool,
        Type::Char,
        Type::Int,
        Type::Double,
        Type::String,
        Type::Error,
    ])
}

fn any_symbol(ids: Vec<SymbolId>) -> impl Strategy<Value = SymbolId> {
    prop::sample::select(ids)
}

fn any_expr(ids: Vec<SymbolId>) -> BoxedStrategy<Expr> {
    let leaf = prop_oneof![
        (0u32..1000).prop_map(|v| ExprKind::Int(NumLit::parse(&v.to_string(), NumType::Int).unwrap())),
        (0.0f64..1e6).prop_map(|v| ExprKind::Float(NumLit::from_f64(v).unwrap())),
        any::<bool>().prop_map(ExprKind::Bool),
        "[^\\x00]{0,12}".prop_map(|s| ExprKind::Str(StrLit::new(&s).unwrap())),
        "[\\x01-\\x7f]".prop_map(|s| ExprKind::Char(CharLit::new(&s).unwrap())),
        any_symbol(ids.clone()).prop_map(ExprKind::Var),
    ];
    let leaf = (leaf, any_type()).prop_map(|(kind, ty)| Expr {
        kind,
        ty,
        origin: origin(),
    });
    leaf.prop_recursive(5, 48, 4, move |inner| {
        let unary = prop::sample::select(vec![UnaryOp::Neg, UnaryOp::Plus, UnaryOp::Not]);
        let binary = prop::sample::select(vec![
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
            BinaryOp::And,
            BinaryOp::Or,
        ]);
        let boxed = || inner.clone().prop_map(Box::new);
        let kind = prop_oneof![
            (unary, boxed()).prop_map(|(op, operand)| ExprKind::Unary { op, operand }),
            (binary, boxed(), boxed()).prop_map(|(op, lhs, rhs)| ExprKind::Binary { op, lhs, rhs }),
            (boxed(), boxed(), boxed()).prop_map(|(cond, then_value, else_value)| ExprKind::Conditional {
                cond,
                then_value,
                else_value
            }),
            (
                any_symbol(ids.clone()),
                prop::collection::vec(inner.clone(), 0..3)
            )
                .prop_map(|(function, args)| ExprKind::Call { function, args }),
            prop::collection::vec(inner.clone(), 0..4).prop_map(ExprKind::Join),
            (boxed(), boxed()).prop_map(|(low, high)| ExprKind::RandomInt { low, high }),
            (any_type(), boxed()).prop_map(|(to, value)| ExprKind::Convert { to, value }),
        ];
        (kind, any_type()).prop_map(|(kind, ty)| Expr {
            kind,
            ty,
            origin: origin(),
        })
    })
    .boxed()
}

#[allow(clippy::too_many_lines)] // a literal test program, clearest in one piece
fn any_stmt(ids: Vec<SymbolId>) -> impl Strategy<Value = Stmt> {
    let expr = any_expr(ids.clone());
    let opt_expr = prop::option::of(expr.clone());
    let comment = prop::option::of(".{0,20}".prop_map(|s| Comment::new(&s).unwrap()));
    let leaf = prop_oneof![
        (
            any_symbol(ids.clone()),
            opt_expr.clone(),
            any::<bool>(),
            any::<bool>()
        )
            .prop_map(
                |(symbol, init, is_const, written_auto)| StmtKind::VarDecl(VarDecl {
                    symbol,
                    init,
                    is_const,
                    written_auto
                })
            ),
        (any_symbol(ids.clone()), expr.clone())
            .prop_map(|(target, value)| StmtKind::Assign { target, value }),
        (any_symbol(ids.clone()), expr.clone()).prop_map(|(target, value)| StmtKind::CompoundAssign {
            target,
            op: CompoundOp::Add,
            value
        }),
        Just(StmtKind::Break),
        Just(StmtKind::Continue),
        opt_expr.clone().prop_map(|value| StmtKind::Return { value }),
        (opt_expr.clone(), any::<bool>()).prop_map(|(code, in_main)| StmtKind::Exit { code, in_main }),
        expr.clone().prop_map(|expr| StmtKind::Eval { expr }),
        (
            prop::collection::vec(expr.clone(), 0..3),
            any::<bool>(),
            any::<bool>()
        )
            .prop_map(|(items, newline, err)| {
                StmtKind::Print {
                    items,
                    separator: PrintSeparator::Comma,
                    newline,
                    stream: if err { OutputStream::Err } else { OutputStream::Out },
                }
            }),
        (opt_expr.clone(), any_symbol(ids.clone()), any::<bool>()).prop_map(|(prompt, target, keep)| {
            StmtKind::Ask {
                prompt,
                target,
                mode: if keep {
                    AskMode::KeepAsking
                } else {
                    AskMode::Simple
                },
            }
        }),
    ];
    let leaf = (leaf, comment.clone()).prop_map(|(kind, comment)| Stmt {
        kind,
        origin: origin(),
        comment,
    });
    leaf.prop_recursive(4, 32, 3, move |inner| {
        let body = || prop::collection::vec(inner.clone(), 0..3).prop_map(|stmts| Block { stmts });
        let kind = prop_oneof![
            (
                prop::collection::vec((expr.clone(), body()), 0..3),
                prop::option::of(body())
            )
                .prop_map(|(branches, else_body)| StmtKind::If {
                    branches: branches
                        .into_iter()
                        .map(|(cond, body)| IfBranch { cond, body })
                        .collect(),
                    else_body,
                }),
            (expr.clone(), any::<bool>(), body()).prop_map(|(cond, until, body)| StmtKind::While {
                cond,
                until,
                body
            }),
            (expr.clone(), body()).prop_map(|(count, body)| StmtKind::Repeat { count, body }),
            (
                any_symbol(ids.clone()),
                expr.clone(),
                expr.clone(),
                opt_expr.clone(),
                body()
            )
                .prop_map(|(var, from, to, step, body)| StmtKind::ForRange {
                    var,
                    from,
                    to,
                    step,
                    direction: RangeDirection::DownInclusive,
                    body,
                }),
            body().prop_map(|body| StmtKind::Forever { body }),
        ];
        (kind, comment.clone()).prop_map(|(kind, comment)| Stmt {
            kind,
            origin: origin(),
            comment,
        })
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn random_programs_never_panic(
        main_body in prop::collection::vec(any_stmt(symbols().1), 0..6),
        function_body in prop::collection::vec(any_stmt(symbols().1), 0..4),
        function in any_symbol(symbols().1),
        ret in any_type(),
    ) {
        let (symbols, ids) = symbols();
        let params = vec![Param { symbol: ids[ids.len() - 2].clone(), mode: PassMode::ReadOnly, origin: origin() }];
        let items = vec![
            Item { kind: ItemKind::Main(MainDef { body: Block { stmts: main_body } }), origin: origin(), comment: None },
            Item {
                kind: ItemKind::Function(FunctionDef { symbol: function, params, ret, body: Block { stmts: function_body } }),
                origin: origin(),
                comment: None,
            },
        ];
        let module = Module { id: ModuleId::new("mod_main").unwrap(), name: String::from("main"), items };
        let program = Program { standard: CppStandard::Cpp20, modules: vec![module], symbols };
        let first = generate(&program, &options("Random"));
        assert_style(&first);
        source_map::assert_well_formed(&first);
        prop_assert_eq!(generate(&program, &options("Random")), first);
    }
}

#[test]
fn placeholders_are_counted() {
    let b = Builder::new();
    let x = b.var("x", Type::Error);
    let broken = b.expr(ExprKind::Var(missing()), Type::Int);
    let main = b.main(vec![b.declare(&x, Some(broken.clone())), b.print(vec![broken])]);
    let program = b.program(vec![main]);
    let report = b2c_codegen::generate_with_report(&program, &options("Broken"));
    let text = render(&report.project);
    assert!(report.placeholders > 0, "{text}");
    assert_eq!(report.placeholders, text.matches("/* error */").count(), "{text}");
    assert_eq!(report.project, generate(&program, &options("Broken")));
}

#[test]
fn user_text_that_looks_like_a_placeholder_is_not_counted() {
    let b = Builder::new();
    let main = b.main(vec![crate::builder::commented(
        b.print_text("/* error */"),
        "/* error */ here",
    )]);
    let report = b2c_codegen::generate_with_report(&b.program(vec![main]), &options("/* error */"));
    let text = render(&report.project);
    assert_eq!(text.matches("/* error */").count(), 3, "{text}");
    assert_eq!(report.placeholders, 0, "{text}");
}
