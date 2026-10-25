//! Unit tests of the forms of types.md §5.2 that no SAST node reaches before
//! W2's blocks: items by position or key, sizes, range-`for`, and user types
//! with their generated functions, all from hand-built symbols and SAST.
//! One program that uses them all is compiled with the `g++` on `PATH` and
//! every `/usr/bin/g++-N`, under C++17 and C++20 with the *strict* warnings,
//! and run.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use b2c_ir::headers::{Header, ItemHeaders};
use b2c_ir::ids::{BlockId, ModuleId, SymbolId};
use b2c_ir::layout::Layout;
use b2c_ir::sast::{
    Block, CppStandard, Expr, ExprKind, FunctionDef, Item, ItemKind, MainDef, Module, Origin, OutputStream,
    Param, PassMode, PrintSeparator, Program, Stmt, StmtKind, Symbol, SymbolKind, SymbolTable, VarDecl,
};
use b2c_ir::text::{Ident, NumLit, NumType, StrLit};
use b2c_ir::types::{LibType, Type};

use super::{LoweredModule, Lowerer, name_expr};
use crate::CodegenOptions;
use crate::cast::{BinOp, CBlock, CExpr, CStmt, CStmtKind, CTypeDef};
use crate::emit;

// ---- building symbols and SAST ------------------------------------------

fn origin(block: &str) -> Origin {
    Origin::whole(ModuleId::new("mod_main").unwrap(), BlockId::new(block).unwrap())
}

fn header(name: &str) -> Header {
    Header::standard(name).unwrap()
}

fn headers(names: &[&str]) -> BTreeSet<Header> {
    names.iter().map(|name| header(name)).collect()
}

fn lib(ty: LibType, args: Vec<Type>) -> Type {
    Type::lib(ty, args).unwrap()
}

fn list(of: Type) -> Type {
    lib(LibType::Vector, vec![of])
}

/// A symbol table and the SAST around it.
#[derive(Default)]
struct Sast {
    symbols: SymbolTable,
    next: u32,
}

impl Sast {
    fn id(&mut self) -> SymbolId {
        self.next += 1;
        SymbolId::new(&format!("sym_{}", self.next)).unwrap()
    }

    fn insert(&mut self, id: &SymbolId, name: &str, kind: SymbolKind, ty: Type) {
        let symbol = Symbol {
            name: Ident::new(name).unwrap(),
            kind,
            ty,
            origin: origin(&format!("blk_{}", id.as_str())),
        };
        self.symbols.symbols.insert(id.clone(), symbol);
    }

    fn add(&mut self, name: &str, kind: SymbolKind, ty: Type) -> SymbolId {
        let id = self.id();
        self.insert(&id, name, kind, ty);
        id
    }

    fn var(&mut self, name: &str, ty: Type) -> SymbolId {
        self.add(name, SymbolKind::Variable { is_const: false }, ty)
    }

    fn structure(
        &mut self,
        name: &str,
        fields: &[(&str, Type)],
        comparable: bool,
        printable: bool,
    ) -> SymbolId {
        let id = self.id();
        let fields = fields
            .iter()
            .map(|(field, ty)| self.add(field, SymbolKind::Field { owner: id.clone() }, ty.clone()))
            .collect();
        let kind = SymbolKind::Struct {
            fields,
            comparable,
            printable,
        };
        self.insert(&id, name, kind, Type::Named(id.clone()));
        id
    }

    fn enumeration(&mut self, name: &str, values: &[&str]) -> SymbolId {
        let id = self.id();
        let values = values
            .iter()
            .map(|value| {
                self.add(
                    value,
                    SymbolKind::Enumerator { owner: id.clone() },
                    Type::Named(id.clone()),
                )
            })
            .collect();
        self.insert(&id, name, SymbolKind::Enum { values }, Type::Named(id.clone()));
        id
    }

    fn ty(&self, id: &SymbolId) -> Type {
        self.symbols.get(id).unwrap().ty.clone()
    }

    fn get(&self, id: &SymbolId) -> Expr {
        expr(ExprKind::Var(id.clone()), self.ty(id))
    }

    fn program(&self, standard: CppStandard, items: Vec<Item>) -> Program {
        let modules = vec![Module {
            id: ModuleId::new("mod_main").unwrap(),
            name: String::from("main"),
            items,
        }];
        Program {
            standard,
            layout: Layout::flat(&modules, &self.symbols),
            modules,
            symbols: self.symbols.clone(),
        }
    }
}

fn expr(kind: ExprKind, ty: Type) -> Expr {
    Expr {
        kind,
        ty,
        origin: origin("expr"),
    }
}

fn int(text: &str) -> Expr {
    expr(
        ExprKind::Int(NumLit::parse(text, NumType::Int).unwrap()),
        Type::Int,
    )
}

fn text(value: &str) -> Expr {
    expr(ExprKind::Str(StrLit::new(value).unwrap()), Type::String)
}

fn compare(op: b2c_ir::sast::BinaryOp, lhs: Expr, rhs: Expr) -> Expr {
    expr(
        ExprKind::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        },
        Type::Bool,
    )
}

fn stmt(kind: StmtKind) -> Stmt {
    Stmt {
        kind,
        origin: origin("stmt"),
        comment: None,
    }
}

fn declare(id: &SymbolId, init: Option<Expr>) -> Stmt {
    stmt(StmtKind::VarDecl(VarDecl {
        symbol: id.clone(),
        init,
        is_const: false,
        written_auto: false,
    }))
}

fn print(items: Vec<Expr>) -> Stmt {
    stmt(StmtKind::Print {
        items,
        separator: PrintSeparator::Space,
        newline: true,
        stream: OutputStream::Out,
    })
}

fn item(kind: ItemKind) -> Item {
    Item {
        kind,
        origin: origin("item"),
        comment: None,
        headers: ItemHeaders::default(),
        shared: false,
    }
}

/// A lowerer over `program` with an item started, its code in the body part.
fn body_lowerer(program: &Program) -> Lowerer<'_> {
    let mut lowerer = Lowerer::new(program);
    lowerer.start_item();
    lowerer.start_body();
    lowerer.scopes = vec![Vec::new()];
    lowerer
}

// ---- items, sizes and range-`for` ---------------------------------------

#[test]
fn positions_are_cast_to_size_but_literals_and_keys_are_not() {
    let mut sast = Sast::default();
    let numbers = sast.var("numbers", list(Type::Int));
    let letters = sast.var("letters", lib(LibType::Deque, vec![Type::Char]));
    let ages = sast.var("ages", lib(LibType::Map, vec![Type::String, Type::Int]));
    let word = sast.var("word", Type::String);
    let i = sast.var("i", Type::Int);
    let c = sast.var("c", Type::Char);
    let program = sast.program(CppStandard::Cpp20, Vec::new());

    let mut lowerer = body_lowerer(&program);
    let at = lowerer.element_at(&sast.get(&numbers), &sast.get(&i));
    assert_eq!(emit::expr_text(&at), "numbers.at(static_cast<std::size_t>(i))\n");
    assert_eq!(lowerer.item_headers.body, headers(&["<cstddef>", "<vector>"]));
    assert!(lowerer.item_headers.decl.is_empty());

    let mut lowerer = body_lowerer(&program);
    let at = lowerer.element_at(&sast.get(&numbers), &int("2"));
    assert_eq!(emit::expr_text(&at), "numbers.at(2)\n");
    assert_eq!(
        lowerer.item_headers.body,
        headers(&["<vector>"]),
        "no cast, no <cstddef>"
    );

    let mut lowerer = body_lowerer(&program);
    let cases = [
        (
            sast.get(&letters),
            sast.get(&c),
            "letters.at(static_cast<std::size_t>(c))",
        ),
        (sast.get(&ages), sast.get(&word), "ages.at(word)"),
        (sast.get(&ages), text("Ann"), "ages.at(\"Ann\")"),
        (
            sast.get(&word),
            sast.get(&i),
            "word.at(static_cast<std::size_t>(i))",
        ),
        (text("abc"), int("0"), "std::string(\"abc\").at(0)"),
    ];
    for (container, index, expected) in cases {
        let at = lowerer.element_at(&container, &index);
        assert_eq!(emit::expr_text(&at).trim_end(), expected);
    }
    assert_eq!(
        lowerer.item_headers.body,
        headers(&["<cstddef>", "<deque>", "<map>", "<string>"])
    );
}

#[test]
fn sizes_are_ints() {
    let mut sast = Sast::default();
    let ids = sast.var("ids", lib(LibType::Set, vec![Type::Int]));
    let program = sast.program(CppStandard::Cpp20, Vec::new());
    let mut lowerer = body_lowerer(&program);
    let size = lowerer.size_of(&sast.get(&ids));
    assert_eq!(emit::expr_text(&size), "static_cast<int>(ids.size())\n");
    let size = lowerer.size_of(&text("abc"));
    assert_eq!(
        emit::expr_text(&size),
        "static_cast<int>(std::string(\"abc\").size())\n"
    );
    assert_eq!(lowerer.item_headers.body, headers(&["<set>", "<string>"]));
}

#[test]
fn range_for_binds_an_item_or_a_key_and_a_value() {
    let mut sast = Sast::default();
    let numbers = sast.var("numbers", list(Type::Int));
    let ages = sast.var("ages", lib(LibType::Map, vec![Type::String, Type::Int]));
    let n = sast.var("n", Type::Int);
    let name = sast.var("name", Type::String);
    let age = sast.var("age", Type::Int);
    let program = sast.program(CppStandard::Cpp17, Vec::new());
    let mut lowerer = body_lowerer(&program);

    let body = Block {
        stmts: vec![print(vec![sast.get(&n)])],
    };
    let one = lowerer.range_for(std::slice::from_ref(&n), &sast.get(&numbers), &body);
    assert_eq!(
        emit::stmt_text(&one),
        "for (const auto& n : numbers) {\n    std::cout << n << '\\n';\n}\n"
    );
    let body = Block {
        stmts: vec![print(vec![sast.get(&name), sast.get(&age)])],
    };
    let pair = lowerer.range_for(&[name.clone(), age.clone()], &sast.get(&ages), &body);
    assert_eq!(
        emit::stmt_text(&pair),
        "for (const auto& [name, age] : ages) {\n    std::cout << name << \" \" << age << '\\n';\n}\n"
    );
    assert_eq!(
        lowerer.item_headers.body,
        headers(&["<iostream>", "<map>", "<string>", "<vector>"])
    );
    // The loop variables are visible in the body only.
    assert!(lowerer.scopes.iter().flatten().all(|ident| ident.as_str() != "n"));

    // One or two variables, each a variable symbol; anything else is an
    // error placeholder.
    let missing = SymbolId::new("sym_missing").unwrap();
    for vars in [
        Vec::new(),
        vec![n.clone(), name.clone(), age.clone()],
        vec![missing],
    ] {
        let kind = lowerer.range_for(&vars, &sast.get(&numbers), &Block::default());
        assert_eq!(emit::stmt_text(&kind), "/* error */;\n", "{vars:?}");
    }
}

// ---- structs and enums ----------------------------------------------------

/// A struct `Point { int x; std::vector<std::string> tags; }` and its
/// definition under `standard`, with the headers it recorded.
fn point(standard: CppStandard, comparable: bool, printable: bool) -> (String, ItemHeaders) {
    let mut sast = Sast::default();
    let fields = [("x", Type::Int), ("tags", list(Type::String))];
    let point = sast.structure("Point", &fields, comparable, printable);
    let program = sast.program(standard, Vec::new());
    let mut lowerer = Lowerer::new(&program);
    lowerer.start_item();
    let def = lowerer.struct_def(&point).unwrap();
    (
        emit::type_def_text(&CTypeDef::Struct(def)),
        std::mem::take(&mut lowerer.item_headers),
    )
}

#[test]
fn comparable_structs_default_the_spaceship_in_cpp20() {
    let (text, headers) = point(CppStandard::Cpp20, true, false);
    assert_eq!(
        text,
        "struct Point {
    int x{};
    std::vector<std::string> tags{};

    auto operator<=>(const Point&) const = default;
};
"
    );
    assert_eq!(
        headers.decl,
        self::headers(&["<compare>", "<string>", "<vector>"])
    );
    assert!(headers.body.is_empty(), "a type's definition is its declaration");
    for standard in [CppStandard::Cpp23, CppStandard::Cpp26] {
        assert_eq!(point(standard, true, false).0, text);
    }
}

#[test]
fn comparable_structs_get_six_hidden_friends_in_cpp17() {
    let (text, headers) = point(CppStandard::Cpp17, true, false);
    let mut expected = String::from("struct Point {\n    int x{};\n    std::vector<std::string> tags{};\n");
    for op in ["==", "!=", "<", ">", "<=", ">="] {
        write!(
            expected,
            "\n    friend bool operator{op}(const Point& lhs, const Point& rhs) {{
        return std::tie(lhs.x, lhs.tags) {op} std::tie(rhs.x, rhs.tags);
    }}\n"
        )
        .unwrap();
    }
    expected.push_str("};\n");
    assert_eq!(text, expected);
    assert_eq!(headers.decl, self::headers(&["<string>", "<tuple>", "<vector>"]));
}

#[test]
fn printable_structs_get_a_hidden_friend_operator() {
    let mut sast = Sast::default();
    let color = sast.enumeration("Color", &["Red"]);
    let fields = [
        ("name", Type::String),
        ("passed", Type::Bool),
        ("color", Type::Named(color)),
    ];
    let student = sast.structure("Student", &fields, false, true);
    let program = sast.program(CppStandard::Cpp20, Vec::new());
    let mut lowerer = Lowerer::new(&program);
    lowerer.start_item();
    let def = lowerer.struct_def(&student).unwrap();
    assert_eq!(
        emit::type_def_text(&CTypeDef::Struct(def)),
        "struct Student {
    std::string name{};
    bool passed{};
    Color color{};

    friend std::ostream& operator<<(std::ostream& out, const Student& value) {
        out << \"Student{name: \" << value.name << \", passed: \" << (value.passed ? \"true\" : \"false\")
            << \", color: \" << value.color << \"}\";
        return out;
    }
};
"
    );
    assert_eq!(lowerer.item_headers.decl, headers(&["<ostream>", "<string>"]));
    assert_eq!(
        point(CppStandard::Cpp17, false, false).1.decl,
        headers(&["<string>", "<vector>"])
    );
}

#[test]
fn generated_parameters_avoid_every_name_in_the_program() {
    let mut sast = Sast::default();
    let fields = [
        ("lhs", Type::Int),
        ("rhs", Type::Int),
        ("out", Type::Int),
        ("value", Type::Int),
    ];
    let clash = sast.structure("Clash", &fields, true, true);
    sast.var("lhs2", Type::Int);
    let program = sast.program(CppStandard::Cpp17, Vec::new());
    let mut lowerer = Lowerer::new(&program);
    let def = lowerer.struct_def(&clash).unwrap();
    let text = emit::type_def_text(&CTypeDef::Struct(def));
    assert!(
        text.contains("friend bool operator==(const Clash& lhs3, const Clash& rhs2)"),
        "{text}"
    );
    assert!(
        text.contains("std::tie(lhs3.lhs, lhs3.rhs, lhs3.out, lhs3.value)"),
        "{text}"
    );
    assert!(
        text.contains("operator<<(std::ostream& out2, const Clash& value2)"),
        "{text}"
    );
}

#[test]
fn structs_without_fields_leave_parameters_unnamed() {
    let mut sast = Sast::default();
    let empty = sast.structure("Empty", &[], true, true);
    let program = sast.program(CppStandard::Cpp17, Vec::new());
    let mut lowerer = Lowerer::new(&program);
    lowerer.start_item();
    let def = lowerer.struct_def(&empty).unwrap();
    let text = emit::type_def_text(&CTypeDef::Struct(def));
    assert!(
        text.starts_with(
            "struct Empty {\n    friend bool operator==(const Empty&, const Empty&) {\n        return true;\n    }\n\n"
        ),
        "{text}"
    );
    assert!(
        text.contains("friend bool operator<(const Empty&, const Empty&) {\n        return false;\n    }"),
        "{text}"
    );
    assert!(
        text.contains("friend bool operator>=(const Empty&, const Empty&) {\n        return true;\n    }"),
        "{text}"
    );
    assert!(
        text.ends_with(
            "    friend std::ostream& operator<<(std::ostream& out, const Empty&) {
        out << \"Empty{}\";
        return out;
    }
};
"
        ),
        "{text}"
    );
    assert_eq!(
        lowerer.item_headers.decl,
        headers(&["<ostream>"]),
        "no std::tie, no <tuple>"
    );
}

#[test]
fn enums_get_print_support_where_they_are_printed_or_joined() {
    let mut sast = Sast::default();
    let color = sast.enumeration("Color", &["Red", "Green"]);
    let program = sast.program(CppStandard::Cpp20, Vec::new());

    let mut lowerer = Lowerer::new(&program);
    lowerer.start_item();
    let def = lowerer.enum_def(&color, false, false).unwrap();
    assert_eq!(
        emit::type_def_text(&CTypeDef::Enum(def)),
        "enum class Color { Red, Green };\n"
    );
    assert!(lowerer.item_headers.decl.is_empty());

    let mut lowerer = Lowerer::new(&program);
    lowerer.start_item();
    let def = lowerer.enum_def(&color, true, true).unwrap();
    assert_eq!(
        emit::type_def_text(&CTypeDef::Enum(def)),
        "enum class Color { Red, Green };

std::ostream& operator<<(std::ostream& out, Color value) {
    switch (value) {
        case Color::Red:
            return out << \"Red\";
        case Color::Green:
            return out << \"Green\";
    }
    return out;
}

std::string to_string(Color value) {
    switch (value) {
        case Color::Red:
            return \"Red\";
        case Color::Green:
            return \"Green\";
    }
    return \"\";
}
"
    );
    assert_eq!(lowerer.item_headers.decl, headers(&["<ostream>", "<string>"]));

    let none = sast.enumeration("None", &[]);
    let program = sast.program(CppStandard::Cpp20, Vec::new());
    let mut lowerer = Lowerer::new(&program);
    let def = lowerer.enum_def(&none, true, true).unwrap();
    assert_eq!(
        emit::type_def_text(&CTypeDef::Enum(def)),
        "enum class None {};

std::ostream& operator<<(std::ostream& out, None) {
    return out;
}

std::string to_string(None) {
    return \"\";
}
"
    );
}

#[test]
fn only_structs_and_enums_have_definitions() {
    let mut sast = Sast::default();
    let color = sast.enumeration("Color", &["Red"]);
    let point = sast.structure("Point", &[], false, false);
    let n = sast.var("n", Type::Int);
    let missing = SymbolId::new("sym_missing").unwrap();
    let program = sast.program(CppStandard::Cpp20, Vec::new());
    let mut lowerer = Lowerer::new(&program);
    assert!(lowerer.struct_def(&color).is_none());
    assert!(lowerer.struct_def(&n).is_none());
    assert!(lowerer.struct_def(&missing).is_none());
    assert!(lowerer.enum_def(&point, true, true).is_none());
    assert!(lowerer.enum_def(&missing, true, true).is_none());
}

// ---- headers per item part -----------------------------------------------

#[test]
fn headers_are_recorded_per_item_part_and_reach_the_cpp() {
    let program = Sast::default().program(CppStandard::Cpp20, Vec::new());
    let mut lowerer = Lowerer::new(&program);
    lowerer.start_item();
    lowerer.include("<vector>");
    lowerer.start_body();
    lowerer.include("<map>");
    assert_eq!(lowerer.item_headers.decl, headers(&["<vector>"]));
    assert_eq!(lowerer.item_headers.body, headers(&["<map>"]));
    let catalog = ItemHeaders {
        decl: headers(&["<cmath>"]),
        body: headers(&["<algorithm>"]),
    };
    lowerer.finish_item(&catalog);
    assert!(lowerer.item_headers.is_empty());
    assert_eq!(
        lowerer.usage.includes,
        headers(&["<algorithm>", "<cmath>", "<map>", "<vector>"])
    );
}

#[test]
fn every_header_the_generator_names_is_standard() {
    let named = [
        "<string>",
        "<cstddef>",
        "<cstdlib>",
        "<iostream>",
        "<limits>",
        "<compare>",
        "<tuple>",
        "<ostream>",
    ];
    let helpers = crate::helpers::Helper::ALL
        .iter()
        .flat_map(|helper| helper.headers());
    for name in named.iter().chain(helpers) {
        assert!(Header::standard(name).is_some(), "{name}");
    }
}

// ---- the whole program, compiled and run --------------------------------

/// What [`forms_program`] prints.
const FORMS_OUTPUT: &str = "true false false false true true
true false false false true true
Student{name: , mark: 0, color: Red, passed: false, average: 0}
Red Empty{}
colour: Red
5 7
c
Ann 30
5
7
0 1
Student{name: , mark: 0, color: Red, passed: false, average: 0}
Red
1 2 1
";

/// `std::cout << items… << '\n';`
fn cout(lowerer: &mut Lowerer<'_>, items: Vec<CExpr>) -> CStmt {
    lowerer.include("<iostream>");
    let mut chain = CExpr::fixed("std::cout");
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 {
            chain = CExpr::binary(BinOp::Shl, chain, CExpr::fixed("' '"));
        }
        chain = CExpr::binary(BinOp::Shl, chain, item);
    }
    CStmt::new(CStmtKind::Expr(CExpr::binary(
        BinOp::Shl,
        chain,
        CExpr::fixed("'\\n'"),
    )))
}

/// `object.name(args…);`
fn call(object: &Ident, name: &'static str, args: Vec<CExpr>) -> CStmt {
    CStmt::new(CStmtKind::Expr(CExpr::method(
        name_expr(object.clone()),
        name,
        args,
    )))
}

/// Inserts statements before a body's last statement (`main`'s `return 0;`)
/// or at its end.
fn splice(body: &mut CBlock, stmts: Vec<CStmt>, before_last: bool) {
    let at = if before_last {
        body.stmts.len().saturating_sub(1)
    } else {
        body.stmts.len()
    };
    body.stmts.splice(at..at, stmts);
}

/// A program with every M3 sample value type, a comparable struct used with
/// all six comparisons and as the key of a `std::set` and a `std::map`, a
/// printable struct, an enum printed and joined, indexed access, a size
/// compared with an `int`, range-`for` loops and read-only parameters of
/// library and user types (types.md §9). It prints [`FORMS_OUTPUT`].
#[allow(clippy::too_many_lines, clippy::many_single_char_names)] // names as in the C++
fn forms_program(standard: CppStandard) -> String {
    use b2c_ir::sast::BinaryOp as Op;
    let mut sast = Sast::default();
    let color = sast.enumeration("Color", &["Red", "Green", "Blue"]);
    let named_color = Type::Named(color.clone());
    let map = |k: Type, v: Type| lib(LibType::Map, vec![k, v]);
    let point_fields = [
        ("x", Type::Int),
        ("y", Type::Double),
        ("name", Type::String),
        ("list", list(Type::Int)),
        ("tags", lib(LibType::Set, vec![Type::String])),
        ("counts", map(Type::String, Type::Int)),
        ("pending", lib(LibType::Queue, vec![Type::Int])),
        ("undo", lib(LibType::Stack, vec![Type::String])),
        ("letters", lib(LibType::Deque, vec![Type::Char])),
        ("grid", list(list(Type::Int))),
        ("color", named_color.clone()),
        ("flag", Type::Bool),
        ("grade", Type::Char),
    ];
    let point = sast.structure("Point", &point_fields, true, false);
    let student_fields = [
        ("name", Type::String),
        ("mark", Type::Int),
        ("color", named_color.clone()),
        ("passed", Type::Bool),
        ("average", Type::Double),
    ];
    let student = sast.structure("Student", &student_fields, true, true);
    let empty = sast.structure("Empty", &[], true, true);
    let point_ty = Type::Named(point.clone());

    // Variables of every sample value type (`b2c_ir::types::sample_types`),
    // with the comparable struct as `Point` and the enum as `Color`.
    let numbers = sast.var("numbers", list(Type::Int));
    let values = [
        sast.var("averages", list(Type::Double)),
        sast.var("shapes", list(point_ty.clone())),
        sast.var("grid", list(list(Type::Int))),
        sast.var("letters", lib(LibType::Deque, vec![Type::Char])),
        sast.var("ids", lib(LibType::Set, vec![Type::Int])),
        sast.var("words", lib(LibType::Set, vec![Type::String])),
        sast.var("readings", map(Type::Int, list(Type::Double))),
        sast.var("waiting", lib(LibType::Queue, vec![Type::Int])),
        sast.var("history", lib(LibType::Stack, vec![Type::String])),
        sast.var("best", lib(LibType::PriorityQueue, vec![Type::Double])),
        sast.var("lines", list(Type::String)),
        sast.var("people", list(Type::Named(student.clone()))),
    ];
    let ages = sast.var("ages", map(Type::String, Type::Int));
    let points = sast.var("points", lib(LibType::Set, vec![point_ty.clone()]));
    let scores = sast.var("scores", map(point_ty.clone(), Type::Int));
    let p = sast.var("p", point_ty.clone());
    let q = sast.var("q", point_ty);
    let s = sast.var("s", Type::Named(student.clone()));
    let c = sast.var("c", named_color.clone());
    let e = sast.var("e", Type::Named(empty.clone()));
    let f = sast.var("f", Type::Named(empty.clone()));
    let i = sast.var("i", Type::Int);
    let label = sast.var("label", Type::String);
    let name = sast.var("name", Type::String);
    let age = sast.var("age", Type::Int);
    let n = sast.var("n", Type::Int);

    // void show(const std::set<Point>& points, const Student& student,
    //           Color color, std::vector<int>& numbers,
    //           std::map<std::string, int> copy)
    let params = [
        ("points", sast.ty(&points), PassMode::ReadOnly),
        ("student", Type::Named(student.clone()), PassMode::ReadOnly),
        ("color", named_color, PassMode::ReadOnly),
        ("numbers", list(Type::Int), PassMode::Editable),
        ("copy", sast.ty(&ages), PassMode::Copy),
    ];
    let param_ids: Vec<SymbolId> = params
        .iter()
        .map(|(name, ty, mode)| sast.add(name, SymbolKind::Parameter { mode: *mode }, ty.clone()))
        .collect();
    let show = sast.add(
        "show",
        SymbolKind::Function {
            params: param_ids.clone(),
        },
        Type::Void,
    );
    let show_def = FunctionDef {
        symbol: show.clone(),
        params: param_ids
            .iter()
            .zip(&params)
            .map(|(id, (_, _, mode))| Param {
                symbol: id.clone(),
                mode: *mode,
                origin: origin("param"),
            })
            .collect(),
        ret: Type::Void,
        body: Block {
            stmts: vec![
                print(vec![sast.get(&param_ids[1])]),
                print(vec![sast.get(&param_ids[2])]),
            ],
        },
    };

    let mut main_body: Vec<Stmt> = [numbers.clone()]
        .iter()
        .chain(&values)
        .chain([&ages, &points, &scores, &p, &q, &s, &c, &e, &f])
        .map(|id| declare(id, None))
        .collect();
    main_body.push(declare(&i, Some(int("0"))));
    main_body.push(declare(&label, None));
    for (lhs, rhs) in [(&p, &q), (&e, &f)] {
        let comparisons = [Op::Eq, Op::Ne, Op::Lt, Op::Gt, Op::Le, Op::Ge]
            .into_iter()
            .map(|op| compare(op, sast.get(lhs), sast.get(rhs)))
            .collect();
        main_body.push(print(comparisons));
    }
    main_body.push(print(vec![sast.get(&s)]));
    main_body.push(print(vec![sast.get(&c), sast.get(&e)]));
    let joined = expr(ExprKind::Join(vec![text("colour: "), sast.get(&c)]), Type::String);
    main_body.push(stmt(StmtKind::Assign {
        target: label.clone(),
        value: joined,
    }));
    main_body.push(print(vec![sast.get(&label)]));
    let main_item = item(ItemKind::Main(MainDef {
        body: Block { stmts: main_body },
    }));
    let show_item = item(ItemKind::Function(show_def.clone()));
    let program = sast.program(standard, vec![main_item.clone(), show_item.clone()]);
    let ident = |id: &SymbolId| program.symbols.get(id).unwrap().name.clone();

    let mut lowerer = Lowerer::new(&program);
    let mut types = Vec::new();
    lowerer.start_item();
    types.push(CTypeDef::Enum(lowerer.enum_def(&color, true, true).unwrap()));
    for def in [&point, &student, &empty] {
        types.push(CTypeDef::Struct(lowerer.struct_def(def).unwrap()));
    }
    lowerer.finish_item(&ItemHeaders::default());

    let ItemKind::Main(main_def) = &main_item.kind else {
        unreachable!()
    };
    let mut main = lowerer.main(main_def, &main_item);
    let mut show_fn = lowerer.function(&show_def, &show_item).unwrap();

    // The forms no SAST node reaches yet, spliced into the bodies.
    lowerer.start_item();
    lowerer.start_body();
    lowerer.scopes = vec![Vec::new()];
    let size_vs_i = CExpr::binary(
        BinOp::Gt,
        lowerer.size_of(&sast.get(&numbers)),
        name_expr(ident(&i)),
    );
    let first_two = vec![
        lowerer.element_at(&sast.get(&numbers), &sast.get(&i)),
        lowerer.element_at(&sast.get(&numbers), &int("1")),
    ];
    let shown = cout(&mut lowerer, first_two);
    let first_letter = vec![lowerer.element_at(&sast.get(&label), &sast.get(&i))];
    let ages_loop = lowerer.range_for(
        &[name.clone(), age.clone()],
        &sast.get(&ages),
        &Block {
            stmts: vec![print(vec![sast.get(&name), sast.get(&age)])],
        },
    );
    let numbers_loop = lowerer.range_for(
        std::slice::from_ref(&n),
        &sast.get(&numbers),
        &Block {
            stmts: vec![print(vec![sast.get(&n)])],
        },
    );
    let found = vec![
        CExpr::method(name_expr(ident(&scores)), "count", vec![name_expr(ident(&q))]),
        lowerer.size_of(&sast.get(&points)),
    ];
    let mut extra = vec![
        call(&ident(&numbers), "push_back", vec![CExpr::fixed("5")]),
        call(&ident(&numbers), "push_back", vec![CExpr::fixed("7")]),
        call(&ident(&points), "insert", vec![name_expr(ident(&p))]),
        call(
            &ident(&ages),
            "emplace",
            vec![CExpr::fixed("\"Ann\""), CExpr::fixed("30")],
        ),
        CStmt::new(CStmtKind::If {
            branches: vec![(size_vs_i, CBlock { stmts: vec![shown] })],
            else_body: None,
        }),
    ];
    extra.push(cout(&mut lowerer, first_letter));
    extra.push(CStmt::new(ages_loop));
    extra.push(CStmt::new(numbers_loop));
    extra.push(cout(&mut lowerer, found));
    let show_call = expr(
        ExprKind::Call {
            function: show.clone(),
            args: [&points, &s, &c, &numbers, &ages].map(|id| sast.get(id)).to_vec(),
        },
        Type::Void,
    );
    extra.push(lowerer.stmt(&stmt(StmtKind::Eval { expr: show_call })));
    let sizes = [&param_ids[0], &param_ids[3], &param_ids[4]].map(|id| lowerer.size_of(&sast.get(id)));
    let sizes = cout(&mut lowerer, sizes.to_vec());
    lowerer.finish_item(&ItemHeaders::default());
    splice(&mut main.body, extra, true);
    splice(&mut show_fn.body, vec![sizes], false);

    let module = LoweredModule {
        types,
        functions: vec![show_fn],
        mains: vec![main],
        usage: lowerer.usage,
    };
    let options = CodegenOptions {
        project_name: String::from("Forms"),
        app_version: String::from("1.0.0"),
        ..CodegenOptions::default()
    };
    let source = emit::source_file(&module, "main", &options);
    assert_eq!(source.placeholders, 0, "{}", source.contents);
    source.contents
}

#[test]
fn the_forms_program_prints_as_expected() {
    insta::assert_snapshot!("forms_cpp20", forms_program(CppStandard::Cpp20));
    insta::assert_snapshot!("forms_cpp17", forms_program(CppStandard::Cpp17));
}

/// Warning flags of the *strict* level (spec §7.4.2) with `-Werror`.
const STRICT: &[&str] = &[
    "-Wall",
    "-Wextra",
    "-Wpedantic",
    "-Werror",
    "-Wshadow",
    "-Wconversion",
    "-Wsign-conversion",
    "-Wold-style-cast",
    "-Wnon-virtual-dtor",
    "-Woverloaded-virtual",
    "-Wnull-dereference",
    "-Wdouble-promotion",
    "-Wformat=2",
    "-Wimplicit-fallthrough",
];

/// A command for g++ or a built test program. Production code spawns
/// processes only through b2c-process; this test drives g++ directly.
#[allow(clippy::disallowed_methods)]
fn command(program: &Path) -> std::process::Command {
    std::process::Command::new(program)
}

/// The compilers to check with: the `g++` on `PATH` first, then every
/// `/usr/bin/g++-N` (side-by-side GCC versions, 07 §7.2). Without a `g++` on
/// `PATH` the check is skipped, unless `B2C_REQUIRE_GXX` is set.
fn compilers() -> Vec<PathBuf> {
    let path_gxx = PathBuf::from("g++");
    let works = |gxx: &Path| {
        command(gxx)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    };
    if !works(&path_gxx) {
        assert!(
            std::env::var_os("B2C_REQUIRE_GXX").is_none(),
            "B2C_REQUIRE_GXX is set but g++ was not found on PATH"
        );
        return Vec::new();
    }
    let mut found = vec![path_gxx];
    for major in 9..=20 {
        let versioned = PathBuf::from(format!("/usr/bin/g++-{major}"));
        if versioned.exists() && works(&versioned) {
            found.push(versioned);
        }
    }
    found
}

#[test]
fn the_forms_program_compiles_clean_and_runs_on_every_gxx() {
    let compilers = compilers();
    if compilers.is_empty() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    for (standard, flag) in [
        (CppStandard::Cpp17, "-std=c++17"),
        (CppStandard::Cpp20, "-std=c++20"),
    ] {
        let source = forms_program(standard);
        let path = dir.path().join("forms.cpp");
        std::fs::write(&path, &source).unwrap();
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
        // Built and run with the g++ on PATH: the generated operators work.
        let program = dir.path().join("forms");
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
        assert!(run.status.success());
        let stdout = String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n");
        assert_eq!(stdout, FORMS_OUTPUT, "{flag}");
    }
}
