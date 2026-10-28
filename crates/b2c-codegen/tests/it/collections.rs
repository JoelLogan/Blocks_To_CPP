//! The collection nodes of W2 (docs/plans/m3-contracts/w2.md §1.2): every
//! change, value and loop form by collection type, snapshotted under C++17
//! and C++20, built and run; and the casts of brace items (slots.md §3.1),
//! every case compiled under both standards, also with `-funsigned-char`.

use b2c_ir::ids::SymbolId;
use b2c_ir::sast::{
    CollectionChange, CppStandard, Expr, ExprKind, OutputStream, PrintSeparator, Program, RangeMode, Stmt,
    StmtKind, UnaryOp,
};
use b2c_ir::types::{LibType, Type};

use crate::builder::{Builder, block};
use crate::{generate_checked, gxx, options, render};

fn lib(ty: LibType, args: Vec<Type>) -> Type {
    Type::lib(ty, args).unwrap()
}

fn list(of: Type) -> Type {
    lib(LibType::Vector, vec![of])
}

/// The extra warnings of the *strict* level that the helpers' set lacks.
const STRICT: &[&str] = &["-Wsign-conversion"];

/// Builders for the collection nodes.
trait Collections {
    fn op(&self, collection: &SymbolId, op: CollectionChange) -> Stmt;
    fn list_of(&self, ty: &Type, items: Vec<Expr>) -> Expr;
    fn item(&self, collection: &SymbolId, index: Expr) -> Expr;
    fn whole(&self, collection: &SymbolId, kind: fn(Box<Expr>) -> ExprKind, ty: Type) -> Expr;
    fn contains(&self, collection: &SymbolId, value: Expr) -> Expr;
    fn each(&self, vars: Vec<SymbolId>, range: Expr, mode: RangeMode, body: Vec<Stmt>) -> Stmt;
    fn line(&self, items: Vec<Expr>) -> Stmt;
}

impl Collections for Builder {
    fn op(&self, collection: &SymbolId, op: CollectionChange) -> Stmt {
        self.stmt(StmtKind::CollectionOp {
            collection: collection.clone(),
            op,
        })
    }

    fn list_of(&self, ty: &Type, items: Vec<Expr>) -> Expr {
        self.expr(ExprKind::ListOf(items), ty.clone())
    }

    fn item(&self, collection: &SymbolId, index: Expr) -> Expr {
        let collection = self.get(collection);
        let ty = match &collection.ty {
            Type::Lib(app) => app.args().last().cloned().unwrap(),
            _ => Type::Error,
        };
        self.expr(
            ExprKind::Item {
                collection: Box::new(collection),
                index: Box::new(index),
            },
            ty,
        )
    }

    fn whole(&self, collection: &SymbolId, kind: fn(Box<Expr>) -> ExprKind, ty: Type) -> Expr {
        self.expr(kind(Box::new(self.get(collection))), ty)
    }

    fn contains(&self, collection: &SymbolId, value: Expr) -> Expr {
        self.expr(
            ExprKind::Contains {
                collection: Box::new(self.get(collection)),
                value: Box::new(value),
            },
            Type::Bool,
        )
    }

    fn each(&self, vars: Vec<SymbolId>, range: Expr, mode: RangeMode, body: Vec<Stmt>) -> Stmt {
        self.stmt(StmtKind::RangeFor {
            vars,
            range,
            mode,
            body: block(body),
        })
    }

    /// A print of the items separated by spaces.
    fn line(&self, items: Vec<Expr>) -> Stmt {
        self.print_with(items, PrintSeparator::Space, true, OutputStream::Out)
    }
}

/// A program using every collection node, with what it prints.
#[allow(
    clippy::too_many_lines,
    reason = "one program of every node, clearest in one piece"
)]
fn forms_program() -> Program {
    let b = Builder::new();
    let int = Type::Int;
    let xs = b.var("xs", list(int.clone()));
    let words = b.var("words", lib(LibType::Set, vec![Type::String]));
    let ages = b.var("ages", lib(LibType::Map, vec![Type::String, int.clone()]));
    let waiting = b.var("waiting", lib(LibType::Queue, vec![int.clone()]));
    let pile = b.var("pile", lib(LibType::Stack, vec![int.clone()]));
    let best = b.var("best", lib(LibType::PriorityQueue, vec![int.clone()]));
    let letters = b.var("letters", lib(LibType::Deque, vec![Type::Char]));
    let grid = b.var("grid", list(list(int.clone())));
    let i = b.var("i", int.clone());
    let x = b.loop_var("x");
    let copy = b.var("copy", int.clone());
    let own = b.var("own", int.clone());
    let unused = b.loop_var("unused");
    let key = b.loop_item("key", Type::String);
    let value = b.loop_item("value", int.clone());
    let letter = b.loop_item("letter", Type::Char);
    let three = [b.int("3"), b.int("1"), b.int("2")].to_vec();
    let body = vec![
        // A list: add, set item, remove item at, sort, contains, item, length.
        b.declare(&xs, Some(b.list_of(&list(int.clone()), three))),
        b.declare(&i, Some(b.int("1"))),
        b.op(&xs, CollectionChange::Add(b.int("5"))),
        b.op(
            &xs,
            CollectionChange::SetItem {
                index: b.int("0"),
                value: b.int("4"),
            },
        ),
        b.op(&xs, CollectionChange::RemoveAt(b.get(&i))),
        b.op(&xs, CollectionChange::Sort { descending: false }),
        b.line(vec![
            b.item(&xs, b.get(&i)),
            b.whole(&xs, ExprKind::Size, int.clone()),
            b.whole(&xs, ExprKind::IsEmpty, Type::Bool),
            b.contains(&xs, b.int("4")),
        ]),
        // *for each*: a copy, an editable item, a read-only one, and one the
        // body does not read.
        b.each(
            vec![copy.clone()],
            b.get(&xs),
            RangeMode::Copy,
            vec![
                b.stmt(StmtKind::CompoundAssign {
                    target: copy.clone(),
                    op: b2c_ir::sast::CompoundOp::Add,
                    value: b.int("1"),
                }),
                b.line(vec![b.get(&copy)]),
            ],
        ),
        b.each(
            vec![own.clone()],
            b.get(&xs),
            RangeMode::Editable,
            vec![b.stmt(StmtKind::CompoundAssign {
                target: own.clone(),
                op: b2c_ir::sast::CompoundOp::Mul,
                value: b.int("2"),
            })],
        ),
        b.op(&xs, CollectionChange::Sort { descending: true }),
        b.each(
            vec![x.clone()],
            b.get(&xs),
            RangeMode::ReadOnly,
            vec![b.line(vec![b.get(&x)])],
        ),
        b.each(
            vec![unused],
            b.get(&xs),
            RangeMode::ReadOnly,
            vec![b.print_text("*")],
        ),
        b.op(&xs, CollectionChange::Clear),
        b.line(vec![b.whole(&xs, ExprKind::IsEmpty, Type::Bool)]),
        // A set and a map.
        b.declare(&words, None),
        b.op(&words, CollectionChange::Add(b.str("pear"))),
        b.op(&words, CollectionChange::Add(b.str("fig"))),
        b.op(&words, CollectionChange::RemoveValue(b.str("pear"))),
        b.line(vec![
            b.contains(&words, b.str("fig")),
            b.whole(&words, ExprKind::Size, int.clone()),
        ]),
        b.declare(&ages, None),
        b.op(
            &ages,
            CollectionChange::SetItem {
                index: b.str("Ann"),
                value: b.int("30"),
            },
        ),
        b.op(
            &ages,
            CollectionChange::SetItem {
                index: b.str("Bob"),
                value: b.int("25"),
            },
        ),
        b.line(vec![b.item(&ages, b.str("Bob")), b.contains(&ages, b.str("Ann"))]),
        b.op(&ages, CollectionChange::RemoveValue(b.str("Bob"))),
        b.each(
            vec![key.clone(), value.clone()],
            b.get(&ages),
            RangeMode::ReadOnly,
            vec![b.line(vec![b.get(&key), b.get(&value)])],
        ),
        // The adaptors.
        b.declare(&waiting, None),
        b.op(&waiting, CollectionChange::Add(b.int("1"))),
        b.op(&waiting, CollectionChange::Add(b.int("2"))),
        b.op(&waiting, CollectionChange::RemoveNext),
        b.line(vec![b.whole(&waiting, ExprKind::Next, int.clone())]),
        b.op(&waiting, CollectionChange::Clear),
        b.declare(&pile, None),
        b.op(&pile, CollectionChange::Add(b.int("1"))),
        b.op(&pile, CollectionChange::Add(b.int("2"))),
        b.declare(&best, None),
        b.op(&best, CollectionChange::Add(b.int("3"))),
        b.op(&best, CollectionChange::Add(b.int("9"))),
        b.op(&best, CollectionChange::Add(b.int("1"))),
        b.line(vec![
            b.whole(&pile, ExprKind::Next, int.clone()),
            b.whole(&best, ExprKind::Next, int.clone()),
            b.whole(&waiting, ExprKind::Size, int.clone()),
        ]),
        // A deque and text.
        b.declare(
            &letters,
            Some(b.list_of(
                &lib(LibType::Deque, vec![Type::Char]),
                vec![b.chr("a"), b.chr("b")],
            )),
        ),
        b.op(&letters, CollectionChange::Add(b.chr("c"))),
        b.line(vec![b.item(&letters, b.int("2"))]),
        b.each(
            vec![letter.clone()],
            b.str("hi"),
            RangeMode::ReadOnly,
            vec![b.line(vec![b.get(&letter)])],
        ),
        // Lists of lists, with *list of* in *add* and in *contains*.
        b.declare(&grid, None),
        b.op(
            &grid,
            CollectionChange::Add(b.list_of(&list(int.clone()), vec![b.int("1"), b.int("2")])),
        ),
        b.line(vec![
            b.contains(&grid, b.list_of(&list(int.clone()), vec![b.int("1"), b.int("2")])),
            b.whole(&grid, ExprKind::Size, int),
        ]),
    ];
    let main = b.main(body);
    b.program(vec![main])
}

/// What [`forms_program`] prints.
const FORMS_OUTPUT: &str = "\
4 3 false true
3
5
6
10
8
4
*
*
*
true
true 1
25 true
Ann 30
2
2 9 0
c
h
i
true 1
";

#[test]
fn collection_forms_print_and_run_under_both_standards() {
    for (standard, name, flag) in [
        (CppStandard::Cpp20, "collection_forms_cpp20", "-std=c++20"),
        (CppStandard::Cpp17, "collection_forms_cpp17", "-std=c++17"),
    ] {
        let mut program = forms_program();
        program.standard = standard;
        let project = generate_checked(&program, &options("Collections"));
        insta::assert_snapshot!(name, render(&project));
        let mut extra = vec![flag];
        extra.extend(STRICT);
        if let Some(program) = gxx::build(&project, &extra) {
            let run = gxx::run(&program, b"");
            assert!(run.status.success(), "{}", run.stderr());
            assert_eq!(run.stdout(), FORMS_OUTPUT, "{flag}");
        }
    }
}

/// Builds and runs `program` under C++17 and C++20 at the strict level,
/// checking that every source holds `form` and that it prints `expected`.
fn builds_and_prints(program: &Program, form: &str, expected: &str) {
    for (standard, flag) in [
        (CppStandard::Cpp17, "-std=c++17"),
        (CppStandard::Cpp20, "-std=c++20"),
    ] {
        let mut program = program.clone();
        program.standard = standard;
        let project = generate_checked(&program, &options("Collections"));
        let source = render(&project);
        assert!(source.contains(form), "{flag}: no `{form}` in\n{source}");
        let mut extra = vec![flag];
        extra.extend(STRICT);
        if let Some(built) = gxx::build(&project, &extra) {
            let run = gxx::run(&built, b"");
            assert!(run.status.success(), "{flag}: {}", run.stderr());
            assert_eq!(run.stdout(), expected, "{flag}\n{source}");
        }
    }
}

#[test]
fn a_copy_of_a_true_false_item_is_a_bool_of_its_own() {
    // `auto` over a `std::vector<bool>` is the list's proxy reference: a
    // change to the "copy" changes the list, and the copy can't be passed
    // to a `bool&` input.
    let b = Builder::new();
    let flags = b.var("flags", list(Type::Bool));
    let flag = b.var("flag", Type::Bool);
    let reset = b.function(
        "reset",
        Type::Void,
        &[("value", Type::Bool, b2c_ir::sast::PassMode::Editable)],
    );
    let value = reset.params[0].clone();
    let body = vec![
        b.declare(
            &flags,
            Some(b.list_of(&list(Type::Bool), vec![b.boolean(true), b.boolean(true)])),
        ),
        b.each(
            vec![flag.clone()],
            b.get(&flags),
            RangeMode::Copy,
            vec![
                b.stmt(StmtKind::Flip { target: flag.clone() }),
                b.eval(b.call(&reset, vec![b.get(&flag)])),
            ],
        ),
        b.line(vec![b.item(&flags, b.int("0")), b.item(&flags, b.int("1"))]),
    ];
    let define = b.define(&reset, vec![b.set(&value, b.boolean(false))]);
    let main = b.main(body);
    let program = b.program(vec![define, main]);
    builds_and_prints(&program, "for (bool flag : flags)", "true true\n");
}

/// Every case of slots.md §3.1: each element type (`int`, `double`,
/// `char`, `bool`) from literals that fit as written, literals that do not,
/// and variables of each type, in a list, a deque and a set.
fn casts_program() -> Program {
    let b = Builder::new();
    let i = b.var("i", Type::Int);
    let d = b.var("d", Type::Double);
    let c = b.var("c", Type::Char);
    let flag = b.var("flag", Type::Bool);
    let negative = |text: &str| b.un(UnaryOp::Neg, b.int(text));
    let mut body = vec![
        b.declare(&i, Some(b.int("65"))),
        b.declare(&d, Some(b.float("2.5"))),
        b.declare(&c, Some(b.chr("z"))),
        b.declare(&flag, Some(b.boolean(true))),
    ];
    let cases: Vec<(&str, Type, Vec<Expr>)> = vec![
        (
            "ints",
            Type::Int,
            vec![
                b.int("5"),
                negative("3"),
                b.chr("a"),
                b.boolean(true),
                b.float("2.0"),
                b.float("1e3"),
                b.get(&d),
                b.get(&c),
                b.get(&flag),
            ],
        ),
        (
            "doubles",
            Type::Double,
            vec![
                b.int("1"),
                negative("2000000"),
                b.float("2.5"),
                b.chr("a"),
                b.boolean(false),
                b.get(&i),
                b.get(&c),
                b.get(&flag),
            ],
        ),
        (
            "chars",
            Type::Char,
            vec![
                b.int("65"),
                b.int("127"),
                b.int("0"),
                b.int("200"),
                negative("5"),
                b.chr("z"),
                b.boolean(true),
                b.float("66.0"),
                b.get(&i),
                b.get(&d),
            ],
        ),
        (
            "flags",
            Type::Bool,
            vec![
                b.int("1"),
                b.int("0"),
                b.int("2"),
                b.chr("a"),
                b.float("1.0"),
                b.get(&i),
                b.get(&flag),
            ],
        ),
    ];
    for (name, elem, items) in cases {
        for container in [LibType::Vector, LibType::Deque, LibType::Set] {
            let ty = lib(container, vec![elem.clone()]);
            let var = b.var(&format!("{name}_{}", container_name(container)), ty.clone());
            body.push(b.declare(&var, Some(b.list_of(&ty, items.clone()))));
            body.push(b.line(vec![b.whole(&var, ExprKind::Size, Type::Int)]));
        }
    }
    let main = b.main(body);
    b.program(vec![main])
}

fn container_name(ty: LibType) -> &'static str {
    match ty {
        LibType::Vector => "list",
        LibType::Deque => "deque",
        _ => "set",
    }
}

#[test]
fn brace_items_are_cast_unless_they_fit_on_every_target() {
    let project = generate_checked(&casts_program(), &options("Casts"));
    let text = render(&project);
    for line in [
        "std::vector<int> ints_list = {5, -3, 'a', true, static_cast<int>(2.0), static_cast<int>(1000.0), \
         static_cast<int>(d), static_cast<int>(c), static_cast<int>(flag)};",
        "std::vector<double> doubles_list = {1, -2000000, 2.5, 'a', false, static_cast<double>(i), \
         static_cast<double>(c), static_cast<double>(flag)};",
        "std::vector<char> chars_list = {65, 127, 0, static_cast<char>(200), static_cast<char>(-5), 'z', \
         true, static_cast<char>(66.0), static_cast<char>(i), static_cast<char>(d)};",
        "std::vector<bool> flags_list = {1, 0, static_cast<bool>(2), static_cast<bool>('a'), \
         static_cast<bool>(1.0), static_cast<bool>(i), flag};",
    ] {
        let flat: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
        assert!(flat.contains(line), "missing {line}\n{text}");
    }
    // Clean under both standards, where `char` is signed and where it is
    // not (aarch64 Linux).
    for standard in ["-std=c++17", "-std=c++20"] {
        for signedness in [None, Some("-funsigned-char")] {
            let mut extra = vec![standard];
            extra.extend(STRICT);
            extra.extend(signedness);
            if let Some(program) = gxx::build(&project, &extra) {
                let run = gxx::run(&program, b"");
                assert!(run.status.success(), "{}", run.stderr());
            }
        }
    }
}

/// A *list of* in a function argument, a `return` and an assignment: the
/// places other than *create* where its braces stand.
#[test]
fn list_of_stands_in_any_place_that_gives_its_type() {
    let b = Builder::new();
    let ty = list(Type::Double);
    let take = b.function(
        "total",
        Type::Double,
        &[("values", ty.clone(), b2c_ir::sast::PassMode::ReadOnly)],
    );
    let make = b.function("make", ty.clone(), &[]);
    let values = take.params[0].clone();
    let sum = b.var("sum", Type::Double);
    let v = b.loop_item("v", Type::Double);
    let total_def = b.define(
        &take,
        vec![
            b.declare(&sum, Some(b.float("0.0"))),
            b.each(
                vec![v.clone()],
                b.get(&values),
                RangeMode::ReadOnly,
                vec![b.stmt(StmtKind::CompoundAssign {
                    target: sum.clone(),
                    op: b2c_ir::sast::CompoundOp::Add,
                    value: b.get(&v),
                })],
            ),
            b.ret(Some(b.get(&sum))),
        ],
    );
    let make_def = b.define(
        &make,
        vec![b.ret(Some(b.list_of(&ty, vec![b.int("1"), b.float("0.5")])))],
    );
    let kept = b.var("kept", ty.clone());
    let main = b.main(vec![
        b.declare(&kept, Some(b.call(&make, Vec::new()))),
        b.set(&kept, b.list_of(&ty, vec![b.float("2.0"), b.int("3")])),
        b.line(vec![
            b.call(&take, vec![b.list_of(&ty, vec![b.int("4")])]),
            b.call(&take, vec![b.get(&kept)]),
        ]),
    ]);
    let project = generate_checked(&b.program(vec![main, total_def, make_def]), &options("Places"));
    let text = render(&project);
    for line in [
        "return {1, 0.5};",
        "kept = {2.0, 3};",
        "std::cout << total({4}) << \" \" << total(kept) << '\\n';",
    ] {
        assert!(text.contains(line), "missing {line}\n{text}");
    }
    let mut extra = vec!["-std=c++17"];
    extra.extend(STRICT);
    if let Some(program) = gxx::build(&project, &extra) {
        let run = gxx::run(&program, b"");
        assert_eq!(run.stdout(), "4 5\n");
    }
}
