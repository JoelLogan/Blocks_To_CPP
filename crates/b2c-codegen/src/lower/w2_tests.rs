//! Tests of the CAST nodes, printing and support helpers W2 adds (w2.md
//! §2.13): each node printed alone, a program made of all of them compiled
//! clean with every g++ under C++17 and C++20 at the *strict* level and run,
//! and the support header with every helper compiled and run the same way.
//! The lowering that makes these nodes from blocks is tested by its package.

use std::collections::BTreeSet;
use std::path::Path;

use b2c_ir::ids::{BlockId, ModuleId};
use b2c_ir::sast::{CppStandard, Origin};
use b2c_ir::text::{CharLit, Comment, Ident, NumLit, NumType, StrLit};
use b2c_ir::types::LibType;

use super::tests::{STRICT, command, compilers};
use super::{int_literal, name_expr};
use crate::CodegenOptions;
use crate::cast::{
    AssignOp, BinOp, BindingStyle, CBlock, CCase, CCatch, CExpr, CExprKind, CGlobal, CStmt, CStmtKind, CType,
    IncOp, Init, Qualifier, RangeBinding,
};
use crate::emit;
use crate::helpers::Helper;

// ---- building CAST --------------------------------------------------------

fn ident(name: &str) -> Ident {
    Ident::new(name).unwrap()
}

fn name(text: &str) -> CExpr {
    name_expr(ident(text))
}

fn text(value: &str) -> CExpr {
    CExpr::new(CExprKind::Str(StrLit::new(value).unwrap()))
}

fn letter(value: &str) -> CExpr {
    CExpr::new(CExprKind::Char(CharLit::new(value).unwrap()))
}

fn real(value: &str) -> CExpr {
    CExpr::new(CExprKind::Num(NumLit::parse(value, NumType::Double).unwrap()))
}

fn block(stmts: Vec<CStmt>) -> CBlock {
    CBlock { stmts }
}

fn stmt(kind: CStmtKind) -> CStmt {
    CStmt::new(kind)
}

/// `std::cout << items… << '\n';`, the items separated by `' '`.
fn out(items: Vec<CExpr>) -> CStmt {
    let mut chain = CExpr::fixed("std::cout");
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 {
            chain = CExpr::binary(BinOp::Shl, chain, CExpr::fixed("' '"));
        }
        chain = CExpr::binary(BinOp::Shl, chain, item);
    }
    stmt(CStmtKind::Expr(CExpr::binary(
        BinOp::Shl,
        chain,
        CExpr::fixed("'\\n'"),
    )))
}

fn student() -> CType {
    CType::Named(vec![ident("Student")])
}

fn range_for(
    binding: RangeBinding,
    style: BindingStyle,
    maybe_unused: bool,
    range: CExpr,
    body: Vec<CStmt>,
) -> CStmtKind {
    CStmtKind::RangeFor {
        binding,
        style,
        maybe_unused,
        range,
        body: block(body),
    }
}

fn global(qualifier: Qualifier, ty: CType, global_name: &str, init: Init) -> CGlobal {
    CGlobal {
        qualifier,
        ty,
        name: ident(global_name),
        init,
        origin: Origin::whole(
            ModuleId::new("mod_main").unwrap(),
            BlockId::new("b_global").unwrap(),
        ),
        comment: None,
    }
}

// ---- each node printed ---------------------------------------------------

/// An expression printed alone, without the final line break.
fn expr_text(expr: &CExpr) -> String {
    emit::expr_text(expr).trim_end_matches('\n').to_owned()
}

/// A statement printed alone, without the final line break.
fn stmt_text(stmt: &CStmtKind) -> String {
    emit::stmt_text(stmt).trim_end_matches('\n').to_owned()
}

#[test]
fn new_expressions_print() {
    let items = name("items");
    assert_eq!(
        expr_text(&CExpr::index(name("ages"), text("Ann"))),
        "ages[\"Ann\"]"
    );
    // A lower-precedence object is parenthesised.
    let sum = CExpr::binary(BinOp::Add, items.clone(), int_literal(1));
    assert_eq!(expr_text(&CExpr::index(sum, int_literal(0))), "(items + 1)[0]");
    assert_eq!(
        expr_text(&CExpr::new(CExprKind::Braces(vec![
            int_literal(1),
            int_literal(2)
        ]))),
        "{1, 2}"
    );
    assert_eq!(expr_text(&CExpr::new(CExprKind::Braces(Vec::new()))), "{}");
    let parts = |designated: bool| {
        let field = |field: &str| designated.then(|| ident(field));
        CExpr::new(CExprKind::Init {
            ty: student(),
            items: vec![(field("name"), text("Ann")), (field("mark"), int_literal(30))],
        })
    };
    assert_eq!(expr_text(&parts(true)), "Student{.name = \"Ann\", .mark = 30}");
    assert_eq!(expr_text(&parts(false)), "Student{\"Ann\", 30}");
    assert_eq!(
        expr_text(&CExpr::call_template(
            "std::max",
            vec![CType::Double],
            vec![real("1.5"), int_literal(2)]
        )),
        "std::max<double>(1.5, 2)"
    );
    assert_eq!(
        expr_text(&CExpr::call_template(
            "std::clamp",
            vec![CType::Int],
            vec![name("x"), int_literal(0), int_literal(9)]
        )),
        "std::clamp<int>(x, 0, 9)"
    );
}

#[test]
fn compound_assignments_print_every_operator() {
    for (op, token) in [
        (AssignOp::BitAnd, "&="),
        (AssignOp::BitOr, "|="),
        (AssignOp::BitXor, "^="),
        (AssignOp::Shl, "<<="),
        (AssignOp::Shr, ">>="),
    ] {
        let assign = CStmtKind::Assign {
            target: ident("bits"),
            op,
            value: int_literal(1),
        };
        assert_eq!(stmt_text(&assign), format!("bits {token} 1;"));
    }
    let at = CStmtKind::AssignTo {
        target: CExpr::method(name("items"), "at", vec![int_literal(1)]),
        op: AssignOp::Set,
        value: int_literal(20),
    };
    assert_eq!(stmt_text(&at), "items.at(1) = 20;");
    let index = CStmtKind::AssignTo {
        target: CExpr::index(name("ages"), text("Ann")),
        op: AssignOp::Add,
        value: int_literal(5),
    };
    assert_eq!(stmt_text(&index), "ages[\"Ann\"] += 5;");
    let field = CStmtKind::AssignTo {
        target: CExpr::field(name("first"), ident("mark")),
        op: AssignOp::Set,
        value: int_literal(31),
    };
    assert_eq!(stmt_text(&field), "first.mark = 31;");
}

#[test]
fn range_for_prints_each_binding_style() {
    let body = || vec![out(vec![name("item")])];
    let one = || RangeBinding::One(ident("item"));
    assert_eq!(
        stmt_text(&range_for(
            one(),
            BindingStyle::ConstRef,
            false,
            name("items"),
            body()
        )),
        "for (const auto& item : items) {\n    std::cout << item << '\\n';\n}"
    );
    assert_eq!(
        stmt_text(&range_for(
            one(),
            BindingStyle::Copy,
            false,
            name("items"),
            body()
        )),
        "for (auto item : items) {\n    std::cout << item << '\\n';\n}"
    );
    assert_eq!(
        stmt_text(&range_for(
            one(),
            BindingStyle::Ref,
            false,
            name("items"),
            Vec::new()
        )),
        "for (auto& item : items) {\n}"
    );
    assert_eq!(
        stmt_text(&range_for(
            one(),
            BindingStyle::ConstRef,
            true,
            name("items"),
            Vec::new()
        )),
        "for ([[maybe_unused]] const auto& item : items) {\n}"
    );
    let pair = RangeBinding::Pair(ident("key"), ident("value"));
    assert_eq!(
        stmt_text(&range_for(
            pair,
            BindingStyle::Ref,
            true,
            name("ages"),
            Vec::new()
        )),
        "for ([[maybe_unused]] auto& [key, value] : ages) {\n}"
    );
}

#[test]
fn switch_prints_cases_blocks_and_default() {
    let switch = CStmtKind::Switch {
        value: name("n"),
        cases: vec![
            CCase {
                label: int_literal(1),
                body: vec![stmt(CStmtKind::Block(block(vec![
                    out(vec![text("one")]),
                    stmt(CStmtKind::Break),
                ])))],
            },
            CCase {
                label: int_literal(2),
                body: vec![out(vec![text("two")]), stmt(CStmtKind::FallThrough)],
            },
            CCase {
                label: int_literal(3),
                body: vec![stmt(CStmtKind::Break)],
            },
        ],
        default: Some(vec![stmt(CStmtKind::Block(block(vec![stmt(CStmtKind::Break)])))]),
    };
    assert_eq!(
        stmt_text(&switch),
        "switch (n) {
    case 1: {
        std::cout << \"one\" << '\\n';
        break;
    }
    case 2:
        std::cout << \"two\" << '\\n';
        [[fallthrough]];
    case 3:
        break;
    default: {
        break;
    }
}"
    );
    let plain_default = CStmtKind::Switch {
        value: name("n"),
        cases: Vec::new(),
        default: Some(vec![stmt(CStmtKind::Break)]),
    };
    assert_eq!(
        stmt_text(&plain_default),
        "switch (n) {\n    default:\n        break;\n}"
    );
}

#[test]
fn do_while_try_throw_and_static_assert_print() {
    let do_while = CStmtKind::DoWhile {
        body: block(vec![stmt(CStmtKind::Inc {
            target: ident("count"),
            op: IncOp::Inc,
        })]),
        cond: CExpr::binary(BinOp::Lt, name("count"), int_literal(3)),
    };
    assert_eq!(stmt_text(&do_while), "do {\n    ++count;\n} while (count < 3);");
    let try_stmt = CStmtKind::Try {
        body: block(vec![stmt(CStmtKind::Throw(Some(CExpr::call_fixed(
            "std::runtime_error",
            vec![text("bad")],
        ))))]),
        catches: vec![
            CCatch {
                ty: CType::Lib(LibType::RuntimeError, Vec::new()),
                name: ident("error"),
                body: block(vec![stmt(CStmtKind::Throw(None))]),
                origin: None,
            },
            CCatch {
                ty: CType::Lib(LibType::Exception, Vec::new()),
                name: ident("other"),
                body: block(Vec::new()),
                origin: None,
            },
        ],
    };
    assert_eq!(
        stmt_text(&try_stmt),
        "try {
    throw std::runtime_error(\"bad\");
} catch (const std::runtime_error& error) {
    throw;
} catch (const std::exception& other) {
}"
    );
    let check = CStmtKind::StaticAssert {
        cond: CExpr::binary(BinOp::Ge, CExpr::fixed("sizeof(int)"), int_literal(2)),
        message: StrLit::new("int is too small").unwrap(),
    };
    assert_eq!(
        stmt_text(&check),
        "static_assert(sizeof(int) >= 2, \"int is too small\");"
    );
}

#[test]
fn declarations_print_constructor_arguments_and_globals() {
    let decl = CStmtKind::Decl {
        is_const: false,
        ty: CType::String,
        name: ident("line"),
        init: Init::Args(vec![int_literal(3), letter("x")]),
    };
    assert_eq!(stmt_text(&decl), "std::string line(3, 'x');");
    let count = global(Qualifier::None, CType::Int, "count", Init::Value(int_literal(0)));
    assert_eq!(emit::global_text(&count), "int count = 0;\n");
    let half = global(
        Qualifier::Constexpr,
        CType::Double,
        "kHalf",
        Init::Value(real("0.5")),
    );
    assert_eq!(emit::global_text(&half), "constexpr double kHalf = 0.5;\n");
    let mut named = global(Qualifier::Const, CType::String, "kName", Init::Value(text("Ann")));
    named.comment = Some(Comment::new("Who is greeted").unwrap());
    assert_eq!(
        emit::global_text(&named),
        "// Who is greeted\nconst std::string kName = \"Ann\";\n"
    );
    let list = global(
        Qualifier::None,
        CType::Lib(LibType::Vector, vec![CType::Int]),
        "items",
        Init::Braces,
    );
    assert_eq!(emit::global_text(&list), "std::vector<int> items{};\n");
}

// ---- a program of every node, compiled and run --------------------------

/// What [`nodes_program`] prints.
const NODES_OUTPUT: &str = "3
three
four
bad
again bad
4
42
8
21
Ann 35
6
5
Ann 31
2
xxx
1 Ann 0
";

/// A program made of every statement and expression node W2 adds, with
/// globals of each qualifier. It prints [`NODES_OUTPUT`].
#[allow(clippy::too_many_lines, reason = "one program of every node")]
fn nodes_program(standard: CppStandard) -> String {
    let items = || name("items");
    let ages = || name("ages");
    let count = || name("count");
    let inc_count = || {
        stmt(CStmtKind::Inc {
            target: ident("count"),
            op: IncOp::Inc,
        })
    };
    let item = || RangeBinding::One(ident("item"));
    let mut body = vec![
        stmt(CStmtKind::StaticAssert {
            cond: CExpr::binary(BinOp::Ge, CExpr::fixed("sizeof(int)"), int_literal(2)),
            message: StrLit::new("int is too small").unwrap(),
        }),
        stmt(CStmtKind::DoWhile {
            body: block(vec![inc_count()]),
            cond: CExpr::binary(BinOp::Lt, count(), int_literal(3)),
        }),
        out(vec![count()]),
        stmt(CStmtKind::Switch {
            value: count(),
            cases: vec![
                CCase {
                    label: int_literal(1),
                    body: vec![stmt(CStmtKind::Block(block(vec![
                        out(vec![text("one")]),
                        stmt(CStmtKind::Break),
                    ])))],
                },
                CCase {
                    label: int_literal(3),
                    body: vec![out(vec![text("three")]), stmt(CStmtKind::FallThrough)],
                },
                CCase {
                    label: int_literal(4),
                    body: vec![out(vec![text("four")]), stmt(CStmtKind::Break)],
                },
            ],
            default: Some(vec![stmt(CStmtKind::Block(block(vec![
                out(vec![text("other")]),
                stmt(CStmtKind::Break),
            ])))]),
        }),
    ];
    // An error thrown, caught, thrown again and caught outside.
    let what = |error: &str| CExpr::method(name(error), "what", Vec::new());
    let inner = CStmtKind::Try {
        body: block(vec![stmt(CStmtKind::Throw(Some(CExpr::call_fixed(
            "std::runtime_error",
            vec![text("bad")],
        ))))]),
        catches: vec![CCatch {
            ty: CType::Lib(LibType::RuntimeError, Vec::new()),
            name: ident("inner"),
            body: block(vec![out(vec![what("inner")]), stmt(CStmtKind::Throw(None))]),
            origin: None,
        }],
    };
    body.push(stmt(CStmtKind::Try {
        body: block(vec![stmt(inner)]),
        catches: vec![CCatch {
            ty: CType::Lib(LibType::Exception, Vec::new()),
            name: ident("outer"),
            body: block(vec![out(vec![text("again"), what("outer")])]),
            origin: None,
        }],
    }));
    // A list changed by position, by an editable item and read by copies.
    let int_list = CType::Lib(LibType::Vector, vec![CType::Int]);
    body.extend([
        stmt(CStmtKind::Decl {
            is_const: false,
            ty: int_list,
            name: ident("items"),
            init: Init::Value(CExpr::new(CExprKind::Braces(vec![
                int_literal(1),
                int_literal(2),
                int_literal(3),
            ]))),
        }),
        stmt(CStmtKind::AssignTo {
            target: CExpr::method(items(), "at", vec![int_literal(1)]),
            op: AssignOp::Set,
            value: int_literal(20),
        }),
        stmt(range_for(
            item(),
            BindingStyle::Ref,
            false,
            items(),
            vec![stmt(CStmtKind::Assign {
                target: ident("item"),
                op: AssignOp::Add,
                value: int_literal(1),
            })],
        )),
        stmt(range_for(
            item(),
            BindingStyle::Copy,
            false,
            items(),
            vec![
                stmt(CStmtKind::Assign {
                    target: ident("item"),
                    op: AssignOp::Mul,
                    value: int_literal(2),
                }),
                out(vec![name("item")]),
            ],
        )),
        out(vec![CExpr::method(items(), "at", vec![int_literal(1)])]),
    ]);
    // A map changed by key and read by key and value.
    let map = CType::Lib(LibType::Map, vec![CType::String, CType::Int]);
    body.extend([
        stmt(CStmtKind::Decl {
            is_const: false,
            ty: map,
            name: ident("ages"),
            init: Init::Braces,
        }),
        stmt(CStmtKind::AssignTo {
            target: CExpr::index(ages(), text("Ann")),
            op: AssignOp::Set,
            value: int_literal(30),
        }),
        stmt(CStmtKind::AssignTo {
            target: CExpr::index(ages(), text("Ann")),
            op: AssignOp::Add,
            value: int_literal(5),
        }),
        stmt(range_for(
            RangeBinding::Pair(ident("key"), ident("value")),
            BindingStyle::ConstRef,
            false,
            ages(),
            vec![out(vec![name("key"), name("value")])],
        )),
        // Items counted without reading them.
        stmt(range_for(
            item(),
            BindingStyle::ConstRef,
            true,
            items(),
            vec![inc_count()],
        )),
        stmt(range_for(
            RangeBinding::Pair(ident("key"), ident("value")),
            BindingStyle::ConstRef,
            true,
            ages(),
            vec![inc_count()],
        )),
        stmt(CStmtKind::Assign {
            target: ident("count"),
            op: AssignOp::Sub,
            value: int_literal(1),
        }),
        out(vec![count()]),
    ]);
    // Every new compound assignment: 12 & 10 | 1 ^ 2 << 1 >> 2 is 5.
    body.push(stmt(CStmtKind::Decl {
        is_const: false,
        ty: CType::Int,
        name: ident("bits"),
        init: Init::Value(int_literal(12)),
    }));
    for (op, value) in [
        (AssignOp::BitAnd, 10),
        (AssignOp::BitOr, 1),
        (AssignOp::BitXor, 2),
        (AssignOp::Shl, 1),
        (AssignOp::Shr, 2),
    ] {
        body.push(stmt(CStmtKind::Assign {
            target: ident("bits"),
            op,
            value: int_literal(value),
        }));
    }
    body.push(out(vec![name("bits")]));
    // A struct value from its fields, designated in C++20 only.
    let designated = standard == CppStandard::Cpp20;
    let field = |field: &str| designated.then(|| ident(field));
    body.extend([
        stmt(CStmtKind::Decl {
            is_const: false,
            ty: student(),
            name: ident("first"),
            init: Init::Value(CExpr::new(CExprKind::Init {
                ty: student(),
                items: vec![(field("name"), text("Ann")), (field("mark"), int_literal(30))],
            })),
        }),
        stmt(CStmtKind::AssignTo {
            target: CExpr::field(name("first"), ident("mark")),
            op: AssignOp::Set,
            value: int_literal(31),
        }),
        out(vec![
            CExpr::field(name("first"), ident("name")),
            CExpr::field(name("first"), ident("mark")),
        ]),
        out(vec![CExpr::call_template(
            "std::max",
            vec![CType::Double],
            vec![real("1.5"), int_literal(2)],
        )]),
        stmt(CStmtKind::Decl {
            is_const: false,
            ty: CType::String,
            name: ident("line"),
            init: Init::Args(vec![int_literal(3), letter("x")]),
        }),
        out(vec![name("line")]),
        out(vec![
            CExpr::binary(BinOp::Mul, name("kHalf"), int_literal(2)),
            name("kName"),
            CExpr::method(name("empty"), "size", Vec::new()),
        ]),
    ]);

    let globals = [
        global(Qualifier::None, CType::Int, "count", Init::Value(int_literal(0))),
        global(
            Qualifier::Constexpr,
            CType::Double,
            "kHalf",
            Init::Value(real("0.5")),
        ),
        global(Qualifier::Const, CType::String, "kName", Init::Value(text("Ann"))),
        global(
            Qualifier::None,
            CType::Lib(LibType::Vector, vec![CType::Int]),
            "empty",
            Init::Braces,
        ),
    ];
    let mut source = String::from(
        "#include <algorithm>
#include <exception>
#include <iostream>
#include <map>
#include <stdexcept>
#include <string>
#include <vector>

struct Student {
    std::string name;
    int mark;
};

",
    );
    for global in &globals {
        source.push_str(&emit::global_text(global));
    }
    source.push_str("\nint main() ");
    source.push_str(&emit::stmt_text(&CStmtKind::Block(block(body))));
    source.push('\n');
    source
}

/// Compiles `source` with every g++ under `flag` at the *strict* level, then
/// builds it with the first and returns what it prints.
fn compile_and_run(dir: &Path, stem: &str, flag: &str, source: &str) -> String {
    let compilers = compilers();
    let path = dir.join(format!("{stem}.cpp"));
    std::fs::write(&path, source).unwrap();
    for gxx in &compilers {
        let output = command(gxx)
            .arg(flag)
            .args(STRICT)
            .arg("-fsyntax-only")
            .arg(&path)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {flag} rejected the program:\n{}\n--- source ---\n{source}",
            gxx.display(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let program = dir.join(stem);
    let output = command(&compilers[0])
        .arg(flag)
        .args(STRICT)
        .arg(&path)
        .arg("-o")
        .arg(&program)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let run = command(&program).output().unwrap();
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n")
}

#[test]
fn the_nodes_program_compiles_clean_and_runs_on_every_gxx() {
    if compilers().is_empty() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    for (standard, flag) in [
        (CppStandard::Cpp17, "-std=c++17"),
        (CppStandard::Cpp20, "-std=c++20"),
    ] {
        let source = nodes_program(standard);
        assert_eq!(
            compile_and_run(dir.path(), "nodes", flag, &source),
            NODES_OUTPUT,
            "{flag}\n{source}"
        );
    }
}

// ---- the support helpers, compiled and run ------------------------------

/// What [`HELPERS_MAIN`] prints.
const HELPERS_OUTPUT: &str = "4: [a] [] [b] [c]
3: [] [x] []
3: [a] [b] [c]
1: []
hello, world! 123
HELLO, WORLD! 123
[a b]
[]
[x]
bXbXb
aaa
aXaXa
1 0 1 1 0
1 0 1 1 0
ok
1 3
caught
[one][two][][last] 4
";

/// A program that uses every helper W2 adds (w2.md §2.12) and prints
/// [`HELPERS_OUTPUT`]; `b2c_support.hpp` holds them all.
const HELPERS_MAIN: &str = r#"#include <deque>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

#include "b2c_support.hpp"

namespace {

void show(const std::vector<std::string>& parts) {
    std::cout << parts.size() << ':';
    for (const auto& part : parts) {
        std::cout << " [" << part << ']';
    }
    std::cout << '\n';
}

}  // namespace

int main() {
    show(b2c::split("a,,b,c", ","));
    show(b2c::split("--x--", "--"));
    show(b2c::split("abc", ""));
    show(b2c::split("", ","));
    std::cout << b2c::to_lower("Hello, WORLD! 123") << '\n';
    std::cout << b2c::to_upper("Hello, world! 123") << '\n';
    std::cout << '[' << b2c::trim(" \t a b\t ") << "]\n";
    std::cout << '[' << b2c::trim(" \t ") << "]\n";
    std::cout << '[' << b2c::trim("x") << "]\n";
    std::cout << b2c::replace_all("babab", "a", "X") << '\n';
    std::cout << b2c::replace_all("aaa", "", "X") << '\n';
    std::cout << b2c::replace_all("aaa", "a", "aX").substr(0, 5) << '\n';
    std::cout << b2c::starts_with("hello", "he") << ' ' << b2c::starts_with("hello", "lo") << ' '
              << b2c::starts_with("hello", "") << ' ' << b2c::starts_with("hello", "hello") << ' '
              << b2c::starts_with("he", "hello") << '\n';
    std::cout << b2c::ends_with("hello", "lo") << ' ' << b2c::ends_with("hello", "he") << ' '
              << b2c::ends_with("hello", "") << ' ' << b2c::ends_with("hello", "hello") << ' '
              << b2c::ends_with("lo", "hello") << '\n';
    bool in_range = true;
    for (int i = 0; i < 1000; ++i) {
        const double low = b2c::random_real(1.5, 2.5);
        const double high = b2c::random_real(2.5, 1.5);
        in_range = in_range && low >= 1.5 && low < 2.5 && high >= 1.5 && high < 2.5;
    }
    in_range = in_range && b2c::random_real(3.0, 3.0) >= 3.0;
    std::cout << (in_range ? "ok" : "out of range") << '\n';
    std::vector<int> numbers = {1, 2, 3};
    b2c::remove_at(numbers, 1);
    std::deque<int> queue = {4, 5};
    b2c::remove_at(queue, 0);
    std::cout << numbers.at(0) << ' ' << numbers.at(1) << '\n';
    try {
        b2c::remove_at(numbers, 2);
        std::cout << "not caught\n";
    } catch (const std::out_of_range&) {
        std::cout << (queue.size() == 1 && queue.front() == 5 ? "caught" : "wrong") << '\n';
    }
    std::istringstream text("one\r\ntwo\n\r\nlast\r");
    std::string line;
    int lines = 0;
    while (b2c::read_line(text, line)) {
        std::cout << '[' << line << ']';
        ++lines;
    }
    std::cout << ' ' << lines << '\n';
}
"#;

#[test]
fn every_helper_compiles_clean_and_behaves_on_every_gxx() {
    if compilers().is_empty() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let all: BTreeSet<Helper> = Helper::ALL.into_iter().collect();
    let header = emit::support_header(&all, &CodegenOptions::default());
    std::fs::write(dir.path().join("b2c_support.hpp"), header).unwrap();
    for flag in ["-std=c++17", "-std=c++20"] {
        assert_eq!(
            compile_and_run(dir.path(), "helpers", flag, HELPERS_MAIN),
            HELPERS_OUTPUT,
            "{flag}"
        );
    }
}
