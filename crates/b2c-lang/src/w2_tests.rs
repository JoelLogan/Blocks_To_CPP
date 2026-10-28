//! Tests of the flow and constant checks of W2's statements (w2.md §2.9,
//! §2.13), on SAST built by hand: the blocks that lower to them come with
//! their packages, which test them from documents.

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::ids::{BlockId, ModuleId, SymbolId};
use b2c_ir::sast::{
    BinaryOp, Block, Catch, Expr, ExprKind, FunctionDef, IfBranch, Item, ItemKind, MainDef, Origin, Stmt,
    StmtKind, SwitchCase, Symbol, SymbolKind, SymbolTable, VarDecl,
};
use b2c_ir::text::{CharLit, Ident, NumLit, StrLit};
use b2c_ir::types::{LibType, Type};

use crate::codes;
use crate::messages::Diags;

// ---- building SAST ---------------------------------------------------------

fn origin(block: &str) -> Origin {
    Origin::whole(
        ModuleId::new("mod_0").expect("id"),
        BlockId::new(block).expect("id"),
    )
}

fn id(name: &str) -> SymbolId {
    SymbolId::new(&format!("sym_{name}")).expect("id")
}

/// `x` (an `int` variable), `K` (an `int` constant) and the enum
/// `Color { Red, Green }`.
fn symbols() -> SymbolTable {
    let mut table = SymbolTable::default();
    let mut add = |name: &str, kind: SymbolKind, ty: Type| {
        let symbol = Symbol {
            name: Ident::new(name).expect("name"),
            kind,
            ty,
            origin: origin(&format!("decl_{name}")),
        };
        table.symbols.insert(id(name), symbol);
    };
    add("x", SymbolKind::Variable { is_const: false }, Type::Int);
    add("K", SymbolKind::Variable { is_const: true }, Type::Int);
    let color = Type::Named(id("Color"));
    add(
        "Color",
        SymbolKind::Enum {
            values: vec![id("Red"), id("Green")],
        },
        color.clone(),
    );
    for value in ["Red", "Green"] {
        add(
            value,
            SymbolKind::Enumerator { owner: id("Color") },
            color.clone(),
        );
    }
    table
}

fn expr(kind: ExprKind, ty: Type, block: &str) -> Expr {
    Expr {
        kind,
        ty,
        origin: origin(block),
    }
}

fn int(value: i32, block: &str) -> Expr {
    expr(ExprKind::Int(NumLit::int(value)), Type::Int, block)
}

fn boolean(value: bool, block: &str) -> Expr {
    expr(ExprKind::Bool(value), Type::Bool, block)
}

fn var(name: &str, block: &str) -> Expr {
    expr(ExprKind::Var(id(name)), Type::Int, block)
}

fn stmt(kind: StmtKind, block: &str) -> Stmt {
    Stmt {
        kind,
        origin: origin(block),
        comment: None,
    }
}

fn body(stmts: Vec<Stmt>) -> Block {
    Block { stmts }
}

fn ret(value: i32, block: &str) -> Stmt {
    stmt(
        StmtKind::Return {
            value: Some(int(value, block)),
        },
        block,
    )
}

fn fall(block: &str) -> Stmt {
    stmt(StmtKind::FallThrough, block)
}

fn case(label: Expr, stmts: Vec<Stmt>) -> SwitchCase {
    SwitchCase {
        label,
        body: body(stmts),
    }
}

fn switch(cases: Vec<SwitchCase>, default: Option<Vec<Stmt>>) -> Stmt {
    stmt(
        StmtKind::Switch {
            value: var("x", "sw_value"),
            cases,
            default: default.map(body),
        },
        "sw",
    )
}

fn item(kind: ItemKind) -> Item {
    Item {
        kind,
        origin: origin("item"),
        comment: None,
        headers: b2c_ir::headers::ItemHeaders::default(),
        shared: false,
    }
}

fn main_item(stmts: Vec<Stmt>) -> Item {
    item(ItemKind::Main(MainDef {
        body: body(stmts),
        args: None,
    }))
}

/// A function that must give back an `int`.
fn function(stmts: Vec<Stmt>) -> Item {
    item(ItemKind::Function(FunctionDef {
        symbol: id("answer"),
        params: Vec::new(),
        ret: Type::Int,
        body: body(stmts),
    }))
}

/// The codes and messages the flow checks report for `item`.
fn flow(item: &Item) -> Vec<(String, String)> {
    let mut diags = Diags::default();
    crate::flow::check_item(item, &symbols(), &mut diags);
    reported(&diags)
}

/// The codes and messages the constant checks report for `item`.
fn consts(item: &Item) -> Vec<(String, String)> {
    let mut diags = Diags::default();
    let mut constants = BTreeMap::new();
    crate::consts::check_item(item, &symbols(), &BTreeSet::new(), &mut diags, &mut constants);
    reported(&diags)
}

fn reported(diags: &Diags) -> Vec<(String, String)> {
    diags
        .list
        .iter()
        .map(|d| (d.code.0.clone(), d.message.clone()))
        .collect()
}

fn codes_of(reported: &[(String, String)]) -> Vec<&str> {
    reported.iter().map(|(code, _)| code.as_str()).collect()
}

// ---- flow ------------------------------------------------------------------

#[test]
fn a_switch_completes_without_a_default_or_when_a_part_does() {
    // Every part returns, and there is a default: the end is not reached.
    let every_part = switch(
        vec![case(int(1, "l1"), vec![ret(1, "r1")])],
        Some(vec![ret(2, "r2")]),
    );
    assert_eq!(flow(&function(vec![every_part])), Vec::new());
    // Without a default, no part may run.
    let no_default = switch(vec![case(int(1, "l1"), vec![ret(1, "r1")])], None);
    assert_eq!(
        codes_of(&flow(&function(vec![no_default]))),
        [codes::MISSING_RETURN]
    );
    // A part that goes on into the next does not reach the switch's end.
    let falls_on = switch(
        vec![
            case(int(1, "l1"), vec![fall("f1")]),
            case(int(2, "l2"), vec![ret(1, "r1")]),
        ],
        Some(vec![ret(2, "r2")]),
    );
    assert_eq!(flow(&function(vec![falls_on])), Vec::new());
    // A part that ends normally does.
    let ends = switch(
        vec![case(int(1, "l1"), vec![stmt(StmtKind::Break, "b1")])],
        Some(vec![ret(2, "r2")]),
    );
    assert_eq!(codes_of(&flow(&function(vec![ends]))), [codes::MISSING_RETURN]);
}

#[test]
fn go_on_into_the_next_case_must_end_a_case_that_another_part_follows() {
    let last_part = switch(
        vec![case(int(1, "l1"), vec![fall("ok")])],
        Some(vec![fall("in_default")]),
    );
    // In the last part but not at its end: moving it to the end would not help.
    let mid_last = switch(
        vec![case(int(1, "l1"), vec![fall("ok2")])],
        Some(vec![fall("mid_default"), stmt(StmtKind::Break, "b0")]),
    );
    let not_last = switch(
        vec![
            case(int(1, "l1"), vec![fall("early"), stmt(StmtKind::Break, "b1")]),
            case(int(2, "l2"), Vec::new()),
        ],
        None,
    );
    let nested = switch(
        vec![
            case(
                int(1, "l1"),
                vec![stmt(
                    StmtKind::If {
                        branches: vec![IfBranch {
                            cond: boolean(true, "c"),
                            body: body(vec![fall("in_if")]),
                        }],
                        else_body: None,
                    },
                    "if",
                )],
            ),
            case(int(2, "l2"), Vec::new()),
        ],
        None,
    );
    let outside = fall("outside");
    let item = main_item(vec![last_part, mid_last, not_last, nested, outside]);
    let mut diags = Diags::default();
    crate::flow::check_item(&item, &symbols(), &mut diags);
    let blocks: Vec<(&str, Option<&str>)> = diags
        .list
        .iter()
        .map(|d| (d.code.0.as_str(), d.primary.block.as_ref().map(BlockId::as_str)))
        .collect();
    assert_eq!(
        blocks,
        [
            (codes::MISPLACED_FALLTHROUGH, Some("in_default")),
            (codes::MISPLACED_FALLTHROUGH, Some("mid_default")),
            (codes::MISPLACED_FALLTHROUGH, Some("early")),
            (codes::MISPLACED_FALLTHROUGH, Some("in_if")),
            (codes::MISPLACED_FALLTHROUGH, Some("outside")),
        ]
    );
    for d in &diags.list[..2] {
        assert_eq!(
            d.message,
            "'go on into the next case' can't be used in the last part of a switch: there is no next case to \
             go on into. Remove it."
        );
    }
    for d in &diags.list[2..] {
        assert_eq!(
            d.message,
            "'go on into the next case' must be the last block of a case, directly in the case (not inside \
             another block). Move it to the end of the case."
        );
    }
}

#[test]
fn throw_ends_its_path_like_return() {
    let throw = || {
        stmt(
            StmtKind::Throw {
                error: Type::lib(LibType::RuntimeError, Vec::new()).expect("type"),
                message: expr(
                    ExprKind::Str(StrLit::new("bad").expect("text")),
                    Type::String,
                    "msg",
                ),
            },
            "throw",
        )
    };
    assert_eq!(flow(&function(vec![throw()])), Vec::new());
    let after = flow(&function(vec![throw(), ret(1, "after")]));
    assert_eq!(codes_of(&after), [codes::UNREACHABLE]);
    assert_eq!(
        flow(&function(vec![stmt(StmtKind::Rethrow, "again")])),
        Vec::new()
    );
}

#[test]
fn a_do_while_runs_its_body_at_least_once() {
    let do_while = |stmts: Vec<Stmt>, cond: Expr| {
        stmt(
            StmtKind::DoWhile {
                body: body(stmts),
                cond,
                until: false,
            },
            "do",
        )
    };
    // A body that always returns never reaches the condition.
    let returns = do_while(vec![ret(1, "r")], var("x", "c"));
    assert_eq!(flow(&function(vec![returns])), Vec::new());
    // `while (true)` with nothing to leave it never ends.
    let endless = do_while(Vec::new(), boolean(true, "c"));
    assert_eq!(flow(&function(vec![endless])), Vec::new());
    // `until (true)` ends after one round.
    let until = stmt(
        StmtKind::DoWhile {
            body: Block::default(),
            cond: boolean(true, "c"),
            until: true,
        },
        "do",
    );
    assert_eq!(codes_of(&flow(&function(vec![until]))), [codes::MISSING_RETURN]);
    // A condition that may be false ends the loop.
    let ends = do_while(Vec::new(), var("x", "c"));
    assert_eq!(codes_of(&flow(&function(vec![ends]))), [codes::MISSING_RETURN]);
    // *leave loop* ends it too.
    let leaves = do_while(vec![stmt(StmtKind::Break, "b")], boolean(true, "c"));
    assert_eq!(codes_of(&flow(&function(vec![leaves]))), [codes::MISSING_RETURN]);
}

#[test]
fn a_try_completes_when_its_try_part_or_a_catch_part_does() {
    let try_stmt = |try_part: Vec<Stmt>, catch_part: Vec<Stmt>| {
        stmt(
            StmtKind::Try {
                body: body(try_part),
                catches: vec![Catch {
                    error: Type::lib(LibType::Exception, Vec::new()).expect("type"),
                    var: id("x"),
                    body: body(catch_part),
                    origin: origin("catch"),
                }],
            },
            "try",
        )
    };
    let both = try_stmt(vec![ret(1, "r1")], vec![ret(2, "r2")]);
    assert_eq!(flow(&function(vec![both])), Vec::new());
    let catch_ends = try_stmt(vec![ret(1, "r1")], Vec::new());
    assert_eq!(
        codes_of(&flow(&function(vec![catch_ends]))),
        [codes::MISSING_RETURN]
    );
    let try_ends = try_stmt(Vec::new(), vec![ret(2, "r2")]);
    assert_eq!(
        codes_of(&flow(&function(vec![try_ends]))),
        [codes::MISSING_RETURN]
    );
}

// ---- constants ---------------------------------------------------------------

#[test]
fn case_labels_are_constant_and_different() {
    let k = stmt(
        StmtKind::VarDecl(VarDecl {
            symbol: id("K"),
            init: Some(int(5, "k_value")),
            is_const: true,
            written_auto: false,
        }),
        "k",
    );
    let letter = expr(
        ExprKind::Char(CharLit::new("A").expect("char")),
        Type::Char,
        "letter",
    );
    let color =
        |value: &str, block: &str| expr(ExprKind::Enumerator(id(value)), Type::Named(id("Color")), block);
    let double = expr(
        ExprKind::Float(NumLit::parse("1.5", b2c_ir::text::NumType::Double).expect("number")),
        Type::Double,
        "real",
    );
    let labels = switch(
        vec![
            case(int(65, "sixty_five"), Vec::new()),
            case(letter, Vec::new()),
            case(var("K", "constant"), Vec::new()),
            case(int(5, "five"), Vec::new()),
            case(var("x", "variable"), Vec::new()),
            case(double, Vec::new()),
        ],
        None,
    );
    let enums = switch(
        vec![
            case(color("Red", "red"), Vec::new()),
            case(color("Green", "green"), Vec::new()),
            case(color("Red", "red_again"), Vec::new()),
        ],
        None,
    );
    let reported = consts(&main_item(vec![k, labels, enums]));
    let not_constant = "A 'case' label must be a value that is fixed when the program is built: a whole \
                        number, a character, a value of an enum, or a constant whole number or character \
                        with a fixed value.";
    let same = |shown: &str| {
        format!(
            "Another 'case' of this switch has the same value ({shown}). Each case needs its own value: \
             remove one of them, or put their blocks together in one case."
        )
    };
    let expected = vec![
        (String::from(codes::DUPLICATE_CASE), same("65")),
        (String::from(codes::DUPLICATE_CASE), same("5")),
        (String::from(codes::NOT_CONSTANT), String::from(not_constant)),
        (String::from(codes::NOT_CONSTANT), String::from(not_constant)),
        (String::from(codes::DUPLICATE_CASE), same("Red")),
    ];
    assert_eq!(reported, expected);
}

#[test]
fn check_at_compile_time_needs_a_constant_condition_that_holds() {
    let check = |cond: Expr| {
        stmt(
            StmtKind::StaticCheck {
                cond,
                message: StrLit::new("too small").expect("text"),
            },
            "check",
        )
    };
    let holds = expr(
        ExprKind::Binary {
            op: BinaryOp::Lt,
            lhs: Box::new(int(1, "one")),
            rhs: Box::new(int(2, "two")),
        },
        Type::Bool,
        "less",
    );
    let reported = consts(&main_item(vec![
        check(holds),
        check(boolean(true, "yes")),
        check(boolean(false, "no")),
        check(expr(
            ExprKind::Binary {
                op: BinaryOp::Lt,
                lhs: Box::new(var("x", "x")),
                rhs: Box::new(int(2, "two")),
            },
            Type::Bool,
            "variable",
        )),
    ]));
    assert_eq!(
        reported,
        vec![
            (
                String::from(codes::CHECK_FAILED),
                String::from(
                    "This check fails when the program is built: its condition is false. Fix the values it \
                     checks, or remove the check."
                ),
            ),
            (
                String::from(codes::NOT_CONSTANT),
                String::from(
                    "The condition of 'check at compile time' must be fixed when the program is built: use \
                     numbers, characters, true/false values and constants with fixed values, not variables or \
                     the answers of functions."
                ),
            ),
        ]
    );
}
