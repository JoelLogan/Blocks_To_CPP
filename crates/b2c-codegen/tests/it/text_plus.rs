//! `+` on text (L16, M3 contracts types.md §11.1): each form the analyser
//! accepts is printed as written when one side is a `std::string` object, and
//! with the left side made one when neither is (two quoted texts, or quoted
//! text and a character, which C++ would add as pointers). Every form compiles
//! with the strict warnings under C++17 and C++20, and the program prints the
//! joined text.

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{BinaryOp, Expr};
use b2c_ir::types::Type;

use crate::builder::Builder;
use crate::gxx;
use crate::{generate_checked, options, render};

/// The forms, each with its C++ and what the program prints for it, with
/// `name` = "Ada", `mark` = '!' and `flag` = true.
fn cases(b: &Builder, [name, mark, flag]: &[SymbolId; 3]) -> Vec<(Expr, &'static str, &'static str)> {
    let plus = |l: Expr, r: Expr| b.bin(BinaryOp::Add, l, r);
    vec![
        (plus(b.get(name), b.str("!")), "name + \"!\"", "Ada!"),
        (plus(b.str("Hi "), b.get(name)), "\"Hi \" + name", "Hi Ada"),
        (plus(b.get(name), b.get(name)), "name + name", "AdaAda"),
        (plus(b.get(name), b.get(mark)), "name + mark", "Ada!"),
        (plus(b.get(mark), b.get(name)), "mark + name", "!Ada"),
        (plus(b.chr("("), b.get(name)), "'(' + name", "(Ada"),
        (plus(b.str("a"), b.str("b")), "std::string(\"a\") + \"b\"", "ab"),
        (plus(b.str("a"), b.chr("b")), "std::string(\"a\") + 'b'", "ab"),
        (plus(b.chr("b"), b.str("a")), "std::string(1, 'b') + \"a\"", "ba"),
        (
            plus(plus(b.str("<"), b.get(name)), b.chr(">")),
            "\"<\" + name + '>'",
            "<Ada>",
        ),
        (
            plus(plus(b.str("a"), b.str("b")), b.str("c")),
            "std::string(\"a\") + \"b\" + \"c\"",
            "abc",
        ),
        (
            plus(b.cond(b.get(flag), b.str("x"), b.str("yy")), b.str("z")),
            "std::string(flag ? \"x\" : \"yy\") + \"z\"",
            "xz",
        ),
        (
            plus(b.get(name), b.cond(b.get(flag), b.chr("y"), b.chr("n"))),
            "name + (flag ? 'y' : 'n')",
            "Aday",
        ),
    ]
}

#[test]
fn text_plus_prints_each_form_and_joins_the_text() {
    let b = Builder::new();
    let name = b.var("name", Type::String);
    let mark = b.var("mark", Type::Char);
    let flag = b.var("flag", Type::Bool);
    let symbols = [name.clone(), mark.clone(), flag.clone()];
    let cases = cases(&b, &symbols);
    let mut body = vec![
        b.declare(&name, Some(b.str("Ada"))),
        b.declare(&mark, Some(b.chr("!"))),
        b.declare(&flag, Some(b.boolean(true))),
    ];
    let mut expected = String::new();
    for (expr, _, printed) in &cases {
        body.push(b.print(vec![expr.clone()]));
        expected.push_str(printed);
        expected.push('\n');
    }
    let main = b.main(body);
    let program = b.program(vec![main]);
    let project = generate_checked(&program, &options("Text Plus"));
    let cpp = render(&project);
    assert!(cpp.contains("#include <string>\n"), "{cpp}");
    for (_, form, _) in &cases {
        assert!(
            cpp.contains(&format!("std::cout << {form} << '\\n';")),
            "missing `{form}` in:\n{cpp}"
        );
    }
    gxx::syntax_check(&project, "c++17");
    gxx::syntax_check(&project, "c++20");
    let Some(executable) = gxx::build(&project, &[]) else {
        return;
    };
    let out = gxx::run(&executable, b"");
    assert!(out.status.success(), "{}", out.stderr());
    assert_eq!(out.stdout(), expected);
}
