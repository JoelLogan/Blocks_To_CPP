//! Library and user types (types.md §1 and §5): their spellings, `const T&`
//! parameters, the headers of the types and values a file's code uses, and
//! the catalog's headers of both item parts. M2's snapshots and golden
//! files stay byte-identical (`snapshots.rs`, `examples.rs`).

use b2c_codegen::generate;
use b2c_ir::headers::{Header, ItemHeaders};
use b2c_ir::ids::ModuleId;
use b2c_ir::layout::Layout;
use b2c_ir::sast::{BinaryOp, Module, PassMode, StmtKind};
use b2c_ir::types::{LibType, Type};

use crate::builder::{Builder, block};
use crate::{generate_checked, gxx, options, render};

fn lib(ty: LibType, args: Vec<Type>) -> Type {
    Type::lib(ty, args).unwrap()
}

fn list(of: Type) -> Type {
    lib(LibType::Vector, vec![of])
}

/// The `#include` lines of a generated file.
fn includes(contents: &str) -> Vec<&str> {
    contents
        .lines()
        .filter_map(|line| line.strip_prefix("#include "))
        .collect()
}

/// A variable of every library value type of M3's samples and the two file
/// streams, read-only, editable and copied parameters of library types, a
/// library return type, and nested types, compiled under C++17 and C++20.
#[test]
fn library_types_print_in_full_and_compile() {
    let b = Builder::new();
    let map = |k: Type, v: Type| lib(LibType::Map, vec![k, v]);
    let grid_ty = list(list(Type::Int));
    let take = b.function(
        "take",
        grid_ty.clone(),
        &[
            ("numbers", list(Type::Int), PassMode::ReadOnly),
            (
                "readings",
                map(Type::String, list(Type::Double)),
                PassMode::Editable,
            ),
            ("words", lib(LibType::Set, vec![Type::String]), PassMode::Copy),
            (
                "best",
                lib(LibType::PriorityQueue, vec![Type::Double]),
                PassMode::ReadOnly,
            ),
        ],
    );
    // Each parameter is used, so `-Wunused-parameter` stays quiet.
    let copies: Vec<_> = take
        .params
        .iter()
        .zip(["numbers_copy", "readings_copy", "words_copy", "best_copy"])
        .map(|(param, name)| {
            let copy = b.var(name, b.ty(param));
            b.declare(&copy, Some(b.get(param)))
        })
        .collect();
    let grid = b.var("grid", grid_ty);
    let mut body = copies;
    body.push(b.declare(&grid, None));
    body.push(b.ret(Some(b.get(&grid))));
    let take_def = b.define(&take, body);

    let variables = [
        ("numbers", list(Type::Int)),
        ("averages", list(Type::Double)),
        ("lines", list(Type::String)),
        ("letters", lib(LibType::Deque, vec![Type::Char])),
        ("ids", lib(LibType::Set, vec![Type::Int])),
        ("words", lib(LibType::Set, vec![Type::String])),
        ("ages", map(Type::String, Type::Int)),
        ("readings", map(Type::String, list(Type::Double))),
        ("waiting", lib(LibType::Queue, vec![Type::Int])),
        ("history", lib(LibType::Stack, vec![Type::String])),
        ("best", lib(LibType::PriorityQueue, vec![Type::Double])),
        ("input", lib(LibType::IfStream, Vec::new())),
        ("output", lib(LibType::OfStream, Vec::new())),
    ];
    let ids: Vec<_> = variables
        .iter()
        .map(|(name, ty)| b.var(name, ty.clone()))
        .collect();
    let mut main_body: Vec<_> = ids.iter().map(|id| b.declare(id, None)).collect();
    let result = b.var("result", list(list(Type::Int)));
    let call = b.call(
        &take,
        vec![b.get(&ids[0]), b.get(&ids[7]), b.get(&ids[5]), b.get(&ids[10])],
    );
    main_body.push(b.declare(&result, Some(call)));
    let main = b.main(main_body);
    let program = b.program(vec![main, take_def]);
    let project = generate_checked(&program, &options("Library Types"));
    insta::assert_snapshot!("library_types", render(&project));
    for standard in ["c++17", "c++20"] {
        gxx::syntax_check(&project, standard);
    }
    let contents = &project.files[0].contents;
    assert!(contents.contains(
        "std::vector<std::vector<int>> take(const std::vector<int>& numbers, \
         std::map<std::string, std::vector<double>>& readings, std::set<std::string> words, \
         const std::priority_queue<double>& best)"
    ));
    assert_eq!(
        includes(contents),
        [
            "<deque>",
            "<fstream>",
            "<map>",
            "<queue>",
            "<set>",
            "<stack>",
            "<string>",
            "<vector>"
        ]
    );
}

/// User types print by their name. A read-only struct is `const T&`; an
/// enum is a scalar, so a read-only enum is a copy, as `int` is.
#[test]
fn user_types_print_by_name() {
    let b = Builder::new();
    let point = b.structure("Point", &[("x", Type::Int)], true, true);
    let color = b.enumeration("Color", &["Red", "Green"]);
    let place = b.function(
        "place",
        point.clone(),
        &[
            ("at", point.clone(), PassMode::ReadOnly),
            ("tint", color.clone(), PassMode::ReadOnly),
            ("moved", point.clone(), PassMode::Editable),
            ("copy", point.clone(), PassMode::Copy),
            ("all", list(point.clone()), PassMode::ReadOnly),
        ],
    );
    let place_def = b.define(&place, vec![b.ret(Some(b.get(&place.params[0])))]);
    let p = b.var("p", point.clone());
    let c = b.var("c", color);
    let all = b.var("all", list(point));
    let main = b.main(vec![
        b.declare(&p, None),
        b.declare(&c, None),
        b.declare(&all, None),
    ]);
    let program = b.program(vec![main, place_def]);
    let project = generate_checked(&program, &options("User Types"));
    insta::assert_snapshot!("user_types", render(&project));
    let contents = &project.files[0].contents;
    assert!(contents.contains(
        "Point place(const Point& at, Color tint, Point& moved, Point copy, const std::vector<Point>& all);"
    ));
    assert_eq!(
        includes(contents),
        ["<vector>"],
        "user types need no standard header"
    );
    // Each type is printed through its symbol: a user type whose symbol is
    // missing is an error placeholder, not a guessed name.
    let missing = Type::Named(b2c_ir::ids::SymbolId::new("sym_missing").unwrap());
    let b = Builder::new();
    let v = b.var("v", list(missing));
    let main = b.main(vec![b.declare(&v, None)]);
    let program = b.program(vec![main]);
    let generation = b2c_codegen::generate_with_report(&program, &options("Broken"));
    assert_eq!(generation.placeholders, 1);
    assert!(
        generation.project.files[0]
            .contents
            .contains("auto v = 0 /* error */;")
    );
}

/// Each module's file includes the headers of the types its own code prints,
/// and of the values it uses through their type, such as a returned list.
#[test]
fn type_headers_land_in_the_file_that_prints_the_type() {
    let b = Builder::new();
    let table = b.var("table", lib(LibType::Map, vec![Type::String, list(Type::Int)]));
    let main = b.main(vec![b.declare(&table, None)]);
    let make = b.function("make", list(Type::Double), &[]);
    let values = b.var("values", list(Type::Double));
    let make_def = b.define(&make, vec![b.declare(&values, None), b.ret(Some(b.get(&values)))]);
    let count = b.function("count", Type::Int, &[]);
    let count_def = b.define(&count, vec![b.ret(Some(b.int("0")))]);
    let user = b.function("user", Type::Void, &[]);
    let user_def = b.define(&user, vec![b.eval(b.call(&make, Vec::new()))]);
    let mut program = b.program(vec![main]);
    for (id, name, items) in [
        ("mod_lists", "lists", vec![make_def]),
        ("mod_plain", "plain", vec![count_def]),
        ("mod_user", "user", vec![user_def]),
    ] {
        program.modules.push(Module {
            id: ModuleId::new(id).unwrap(),
            name: String::from(name),
            items,
        });
    }
    program.layout = Layout::flat(&program.modules, &program.symbols);
    let project = generate(&program, &options("Headers"));
    let files: Vec<(&str, Vec<&str>)> = project
        .files
        .iter()
        .map(|file| (file.path.as_str(), includes(&file.contents)))
        .collect();
    assert_eq!(
        files,
        [
            ("main.cpp", vec!["<map>", "<string>", "<vector>"]),
            ("lists.cpp", vec!["<vector>"]),
            ("plain.cpp", vec![]),
            // `make()`'s list is used here, though no type is named.
            ("user.cpp", vec!["<vector>"]),
        ]
    );
}

/// The catalog's headers of both parts of an item reach the file that prints
/// the item; an item that is not printed brings none.
#[test]
fn catalog_headers_of_both_item_parts_are_included() {
    let b = Builder::new();
    let f = b.function("f", Type::Int, &[]);
    let mut f_def = b.define(&f, vec![b.ret(Some(b.int("1")))]);
    f_def.headers = ItemHeaders {
        decl: [Header::standard("<cmath>").unwrap()].into(),
        body: [Header::standard("<algorithm>").unwrap()].into(),
    };
    let mut main = b.main(vec![b.print_text("hi")]);
    main.headers.body.insert(Header::standard("<iomanip>").unwrap());
    let mut orphan = b.define(&b.function("orphan", Type::Void, &[]), Vec::new());
    orphan.headers.body.insert(Header::standard("<numeric>").unwrap());
    if let b2c_ir::sast::ItemKind::Function(def) = &mut orphan.kind {
        def.symbol = b2c_ir::ids::SymbolId::new("sym_missing").unwrap();
    }
    let program = b.program(vec![main, f_def, orphan]);
    let project = generate_checked(&program, &options("Catalog Headers"));
    gxx::syntax_check(&project, "c++20");
    assert_eq!(
        includes(&project.files[0].contents),
        ["<algorithm>", "<cmath>", "<iomanip>", "<iostream>"]
    );
}

/// `join` writes an enum with its generated `::to_string` (types.md §5.2), and
/// `repeat until` turns `==` of two library values into `!=`, as for numbers.
#[test]
fn enums_join_by_to_string_and_library_equality_inverts() {
    let b = Builder::new();
    let color = b.enumeration("Color", &["Red"]);
    let c = b.var("c", color);
    let text = b.var("text", Type::String);
    let first = b.var("first", list(Type::Int));
    let second = b.var("second", list(Type::Int));
    let until = b.stmt(StmtKind::While {
        cond: b.bin(BinaryOp::Eq, b.get(&first), b.get(&second)),
        until: true,
        body: block(Vec::new()),
    });
    let ordered = b.stmt(StmtKind::While {
        cond: b.bin(BinaryOp::Lt, b.get(&first), b.get(&second)),
        until: true,
        body: block(Vec::new()),
    });
    let main = b.main(vec![
        b.declare(&c, None),
        b.declare(&text, Some(b.join(vec![b.str("colour: "), b.get(&c)]))),
        b.declare(&first, None),
        b.declare(&second, None),
        until,
        ordered,
    ]);
    let program = b.program(vec![main]);
    let contents = generate_checked(&program, &options("Enums"))
        .files
        .remove(0)
        .contents;
    assert!(
        contents.contains("std::string text = std::string(\"colour: \") + ::to_string(c);"),
        "{contents}"
    );
    assert!(contents.contains("while (first != second) {"), "{contents}");
    // Ordering may meet NaN inside a list of `double`, so it is not inverted.
    assert!(contents.contains("while (!(first < second)) {"), "{contents}");
}

/// Overloads of library and user types come in the order `Layout::flat`
/// lists them: by name, then each parameter's spelling and passing style.
#[test]
fn overloads_of_library_and_user_types_follow_the_flat_layout() {
    let b = Builder::new();
    let point = b.structure("Point", &[("x", Type::Int)], false, false);
    let color = b.enumeration("Color", &["Red"]);
    let signatures = [
        vec![("v", list(Type::Int), PassMode::ReadOnly)],
        vec![("v", list(Type::Int), PassMode::Copy)],
        vec![("v", list(Type::Int), PassMode::Editable)],
        vec![("p", point.clone(), PassMode::ReadOnly)],
        vec![("p", point, PassMode::Copy)],
        vec![("c", color.clone(), PassMode::ReadOnly)],
        vec![("n", Type::Int, PassMode::ReadOnly)],
        vec![("v", list(Type::String), PassMode::ReadOnly)],
    ];
    let items: Vec<_> = signatures
        .iter()
        .map(|params| {
            let f = b.function("f", Type::Void, params);
            b.define(&f, Vec::new())
        })
        .collect();
    let program = b.program(items);
    let contents = generate(&program, &options("Overloads")).files.remove(0).contents;
    let declared: Vec<&str> = contents.lines().filter(|line| line.ends_with(");")).collect();
    assert_eq!(
        declared,
        [
            "void f(Color c);",
            "void f(Point p);",
            "void f(const Point& p);",
            "void f(int n);",
            "void f(std::vector<int> v);",
            "void f(std::vector<int>& v);",
            "void f(const std::vector<int>& v);",
            "void f(const std::vector<std::string>& v);",
        ]
    );
    // `Layout::flat` lists the items in the same order (indices into
    // `signatures`).
    assert_eq!(program.layout.modules[0].source_order, [5, 4, 3, 6, 1, 2, 0, 7]);
}
