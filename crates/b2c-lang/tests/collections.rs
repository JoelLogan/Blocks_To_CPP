//! The collection blocks of W2 (docs/plans/m3-contracts/w2.md §1.2,
//! types.md §1.5): typing by collection type, the C++ forms' SAST, and each
//! rule with its exact message: `B2C-E0301` for each class, `B2C-E0308`,
//! `B2C-E0313`, `B2C-E0315`, and the scope of the loops' names.

// Test code: unwrap/expect/panic and printing to stderr are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]
#![allow(clippy::needless_pass_by_value, reason = "helpers take json! values")]

mod common;

use b2c_ir::diag::Part;
use b2c_ir::sast::{CollectionChange, ExprKind, ItemKind, RangeMode, StmtKind, SymbolKind};
use b2c_ir::types::{LibType, Type};
use b2c_lang::Analysis;
use common::*;
use serde_json::{Value, json};

// --- Building ------------------------------------------------------------------

/// A type term from compact text: `int`, `vector<int>`, `map<std::string,int>`.
fn term(text: &str) -> Value {
    let text = text.trim();
    let Some(open) = text.find('<') else {
        return json!(text);
    };
    let name = format!("std::{}", &text[..open]);
    let inner = &text[open + 1..text.len() - 1];
    // Split the arguments at top-level commas.
    let mut args = Vec::new();
    let (mut depth, mut start) = (0, 0);
    for (index, c) in inner.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => {
                args.push(term(&inner[start..index]));
                start = index + 1;
            }
            _ => {}
        }
    }
    args.push(term(&inner[start..]));
    if args.len() == 1 {
        json!({ name: args[0] })
    } else {
        json!({ name: args })
    }
}

/// `create <ty> <name>` (a template type in compact text), with a value.
fn create(ty: &str, name: &str, value: Option<Value>) -> Value {
    let mut block = json!({"id": id(), "type": "var.declare", "v": 1,
        "fields": {"TYPE": term(ty), "NAME": {"sym": sym(name), "name": name}}});
    if let Some(value) = value {
        block["inputs"] = json!({ "VALUE": value });
    }
    block
}

/// A collection block naming `list` in `LIST`, with these inputs.
fn on(kind: &str, list: &str, inputs: Value) -> Value {
    json!({"id": id(), "type": kind, "v": 1, "fields": {"LIST": {"ref": sym(list)}}, "inputs": inputs})
}

fn add(list: &str, value: Value) -> Value {
    on("coll.add", list, json!({ "VALUE": value }))
}

fn item(list: &str, index: Value) -> Value {
    on("coll.item", list, json!({ "INDEX": index }))
}

fn set_item(list: &str, index: Value, value: Value) -> Value {
    on("coll.set_item", list, json!({ "INDEX": index, "VALUE": value }))
}

fn whole(kind: &str, list: &str) -> Value {
    on(kind, list, json!({}))
}

/// *list of* with these items.
fn list_of(items: Vec<Value>) -> Value {
    let inputs: serde_json::Map<String, Value> = items
        .into_iter()
        .enumerate()
        .map(|(i, v)| (format!("ITEM{i}"), v))
        .collect();
    let count = inputs.len();
    json!({"id": id(), "type": "coll.list_of", "v": 1, "extra": {"itemCount": count}, "inputs": inputs})
}

/// *for each* `name` in `list` (an input) with `mode`.
fn for_each(name: &str, mode: &str, list: Value, body: Vec<Value>) -> Value {
    json!({"id": id(), "type": "coll.for_each", "v": 1,
        "fields": {"ITEM": {"sym": sym(name), "name": name}, "MODE": mode},
        "inputs": {"LIST": list}, "statements": {"BODY": body}})
}

/// *for each key value* over the map `map`.
fn for_each_pair(map: &str, key: &str, value: &str, body: Vec<Value>) -> Value {
    json!({"id": id(), "type": "coll.for_each_pair", "v": 1,
        "fields": {"MAP": {"ref": sym(map)}, "KEY": {"sym": sym(key), "name": key},
            "VALUE": {"sym": sym(value), "name": value}},
        "statements": {"BODY": body}})
}

/// Analyses `main` with these statements with the catalog's signatures.
fn check(body: Vec<Value>) -> Analysis {
    analyze(&doc(vec![main(body)]))
}

/// The statements of `main`.
fn statements(a: &Analysis) -> &[b2c_ir::sast::Stmt] {
    a.program.modules[0]
        .items
        .iter()
        .find_map(|item| match &item.kind {
            ItemKind::Main(main) => Some(main.body.stmts.as_slice()),
            _ => None,
        })
        .expect("main")
}

/// The messages of the diagnostics with this code, in order.
fn messages(a: &Analysis, code: &str) -> Vec<String> {
    a.diagnostics
        .iter()
        .filter(|d| d.code.0 == code)
        .map(|d| d.message.clone())
        .collect()
}

fn lib(ty: LibType, args: Vec<Type>) -> Type {
    Type::lib(ty, args).unwrap()
}

/// The type of `value` as the initial value of an `auto` variable after
/// `setup`.
fn type_of(setup: Vec<Value>, value: Value) -> Type {
    let mut body = setup;
    let count = body.len();
    body.push(declare("auto", "r", Some(input(value))));
    let a = check(body);
    assert_clean(&a);
    let StmtKind::VarDecl(decl) = &statements(&a)[count].kind else {
        panic!("a declaration")
    };
    decl.init.as_ref().unwrap().ty.clone()
}

// --- Typing by collection type ---------------------------------------------------

#[test]
fn items_lengths_and_lookups_are_typed_by_the_collection() {
    let collections = [
        ("vector<double>", Type::Double),
        ("deque<char>", Type::Char),
        ("map<std::string,int>", Type::Int),
    ];
    for (ty, elem) in collections {
        let index = if ty.starts_with("map") {
            text("Ann")
        } else {
            e("0")
        };
        let setup = vec![create(ty, "c", None)];
        assert_eq!(type_of(setup.clone(), item("c", index)), elem, "{ty}");
        assert_eq!(type_of(setup, whole("coll.length", "c")), Type::Int, "{ty}");
    }
    for ty in [
        "vector<int>",
        "deque<int>",
        "set<int>",
        "map<int,int>",
        "queue<int>",
        "stack<int>",
        "priority_queue<int>",
    ] {
        let setup = vec![create(ty, "c", None)];
        assert_eq!(
            type_of(setup.clone(), whole("coll.length", "c")),
            Type::Int,
            "{ty}"
        );
        assert_eq!(type_of(setup, whole("coll.is_empty", "c")), Type::Bool, "{ty}");
    }
    for ty in ["queue<double>", "stack<double>", "priority_queue<double>"] {
        let setup = vec![create(ty, "c", None)];
        assert_eq!(type_of(setup, whole("coll.next", "c")), Type::Double, "{ty}");
    }
    for (ty, value) in [
        ("vector<int>", e("1")),
        ("set<std::string>", text("a")),
        ("map<std::string,int>", text("a")),
    ] {
        let setup = vec![create(ty, "c", None)];
        let contains = on("coll.contains", "c", json!({ "VALUE": value }));
        assert_eq!(type_of(setup, contains), Type::Bool, "{ty}");
    }
}

#[test]
fn changes_lower_to_their_collection_ops() {
    let a = check(vec![
        create("vector<int>", "xs", None),
        create("set<int>", "s", None),
        create("map<std::string,double>", "m", None),
        create("queue<int>", "q", None),
        add("xs", e("1")),
        add("s", e("2")),
        add("q", e("3")),
        set_item("xs", e("0"), e("4")),
        set_item("m", text("Ann"), e("1.5")),
        whole("coll.clear", "q"),
        on("coll.remove_at", "xs", json!({ "INDEX": e("0") })),
        on("coll.remove_value", "s", json!({ "VALUE": e("2") })),
        whole("coll.remove_next", "q"),
        json!({"id": id(), "type": "coll.sort", "v": 1,
            "fields": {"LIST": {"ref": sym("xs")}, "ORDER": "descending"}}),
    ]);
    assert_clean(&a);
    let ops: Vec<(&str, &CollectionChange)> = statements(&a)[4..]
        .iter()
        .map(|stmt| match &stmt.kind {
            StmtKind::CollectionOp { collection, op } => (collection.as_str(), op),
            other => panic!("{other:?}"),
        })
        .collect();
    let names: Vec<&str> = ops.iter().map(|(collection, _)| *collection).collect();
    assert_eq!(
        names,
        [
            "sym_xs", "sym_s", "sym_q", "sym_xs", "sym_m", "sym_q", "sym_xs", "sym_s", "sym_q", "sym_xs"
        ]
    );
    assert!(matches!(ops[0].1, CollectionChange::Add(value) if value.ty == Type::Int));
    assert!(matches!(ops[4].1, CollectionChange::SetItem { index, value }
        if index.ty == Type::String && value.ty == Type::Double));
    assert!(matches!(ops[5].1, CollectionChange::Clear));
    assert!(matches!(ops[6].1, CollectionChange::RemoveAt(_)));
    assert!(matches!(ops[7].1, CollectionChange::RemoveValue(_)));
    assert!(matches!(ops[8].1, CollectionChange::RemoveNext));
    assert!(matches!(ops[9].1, CollectionChange::Sort { descending: true }));
}

#[test]
fn for_each_declares_its_item_by_mode() {
    let a = check(vec![
        create("vector<std::string>", "names", None),
        for_each("name", "read_only", e("names"), vec![print(vec![e("name")])]),
        for_each("copy", "copy", e("names"), vec![print(vec![e("copy")])]),
        for_each("own", "editable", e("names"), vec![set("own", text("x"))]),
        for_each(
            "letter",
            "read_only",
            text("hello"),
            vec![print(vec![e("letter")])],
        ),
    ]);
    assert_clean(&a);
    let loops = &statements(&a)[1..];
    let expected = [
        (
            RangeMode::ReadOnly,
            "name",
            SymbolKind::LoopVariable,
            Type::String,
        ),
        (
            RangeMode::Copy,
            "copy",
            SymbolKind::Variable { is_const: false },
            Type::String,
        ),
        (
            RangeMode::Editable,
            "own",
            SymbolKind::Variable { is_const: false },
            Type::String,
        ),
        (
            RangeMode::ReadOnly,
            "letter",
            SymbolKind::LoopVariable,
            Type::Char,
        ),
    ];
    for (stmt, (mode, name, kind, ty)) in loops.iter().zip(expected) {
        let StmtKind::RangeFor {
            vars,
            mode: actual,
            range,
            ..
        } = &stmt.kind
        else {
            panic!("{:?}", stmt.kind)
        };
        assert_eq!(*actual, mode);
        assert_eq!(vars.len(), 1);
        let symbol = a.program.symbols.get(&vars[0]).unwrap();
        assert_eq!(
            (symbol.name.as_str(), &symbol.kind, &symbol.ty),
            (name, &kind, &ty)
        );
        assert_ne!(range.ty, Type::Error);
    }
}

#[test]
fn for_each_pair_declares_a_read_only_key_and_value() {
    let a = check(vec![
        create("map<char,vector<int>>", "m", None),
        for_each_pair("m", "key", "value", vec![print(vec![e("key")])]),
    ]);
    assert_clean(&a);
    let StmtKind::RangeFor {
        vars, mode, range, ..
    } = &statements(&a)[1].kind
    else {
        panic!()
    };
    assert_eq!(*mode, RangeMode::ReadOnly);
    assert!(matches!(&range.kind, ExprKind::Var(id) if id.as_str() == "sym_m"));
    let types: Vec<(&SymbolKind, &Type)> = vars
        .iter()
        .map(|id| {
            let symbol = a.program.symbols.get(id).unwrap();
            (&symbol.kind, &symbol.ty)
        })
        .collect();
    assert_eq!(
        types,
        [
            (&SymbolKind::LoopVariable, &Type::Char),
            (&SymbolKind::LoopVariable, &lib(LibType::Vector, vec![Type::Int])),
        ]
    );
}

#[test]
fn list_of_takes_its_type_and_its_items_types_from_its_place() {
    for ty in [
        "vector<double>",
        "deque<int>",
        "set<std::string>",
        "vector<vector<int>>",
    ] {
        let items = match ty {
            "set<std::string>" => vec![text("a"), text("b")],
            "vector<vector<int>>" => vec![input(list_of(vec![e("1")])), input(list_of(Vec::new()))],
            _ => vec![e("1"), e("2")],
        };
        let a = check(vec![create(ty, "c", Some(input(list_of(items))))]);
        assert_clean(&a);
        let StmtKind::VarDecl(decl) = &statements(&a)[0].kind else {
            panic!()
        };
        let init = decl.init.as_ref().unwrap();
        assert!(matches!(init.kind, ExprKind::ListOf(_)), "{ty}");
        assert_eq!(
            Some(&init.ty),
            a.program.symbols.get(&decl.symbol).map(|s| &s.ty),
            "{ty}"
        );
    }
    // Also where *add* and *set item* expect an item, and as an argument.
    let a = check(vec![
        create("vector<vector<int>>", "grid", None),
        add("grid", input(list_of(vec![e("1"), e("2")]))),
        set_item("grid", e("0"), input(list_of(vec![e("3")]))),
    ]);
    assert_clean(&a);
}

// --- The rules -------------------------------------------------------------------

#[test]
fn a_collection_outside_the_blocks_class_is_refused() {
    let cases: Vec<(Value, &str)> = vec![
        (
            add("m", e("1")),
            "This is a map from int to int, but this block needs a list, a deque, a set, a queue, a stack \
             or a priority queue here.",
        ),
        (
            set_item("s", e("0"), e("1")),
            "This is a set of int, but this block needs a list, a deque or a map here.",
        ),
        (
            whole("coll.clear", "n"),
            "This is a whole number, but this block needs a list, a deque, a set, a map, a queue, a stack \
             or a priority queue here.",
        ),
        (
            on("coll.remove_at", "s", json!({ "INDEX": e("0") })),
            "This is a set of int, but this block needs a list or a deque here.",
        ),
        (
            on("coll.remove_value", "xs", json!({ "VALUE": e("0") })),
            "This is a list of int, but this block needs a set or a map here.",
        ),
        (
            whole("coll.remove_next", "xs"),
            "This is a list of int, but this block needs a queue, a stack or a priority queue here.",
        ),
        (
            whole("coll.sort", "s"),
            "This is a set of int, but this block needs a list or a deque of values that can be compared \
             here.",
        ),
    ];
    for (block, message) in cases {
        let a = check(vec![
            create("vector<int>", "xs", None),
            create("set<int>", "s", None),
            create("map<int,int>", "m", None),
            declare("int", "n", Some(e("0"))),
            block,
        ]);
        assert_eq!(messages(&a, "B2C-E0301"), [message], "{}", render_all(&a));
        assert_eq!(a.diagnostics.len(), 1, "{}", render_all(&a));
        assert_eq!(
            a.diagnostics[0].primary.part,
            Part::Field {
                name: String::from("LIST")
            }
        );
    }
    // Values of the collection, and *for each key value* over a list.
    let values: Vec<(Value, &str)> = vec![
        (
            item("s", e("0")),
            "This is a set of int, but this block needs a list, a deque or a map here.",
        ),
        (
            whole("coll.next", "xs"),
            "This is a list of int, but this block needs a queue, a stack or a priority queue here.",
        ),
        (
            on("coll.contains", "q", json!({ "VALUE": e("1") })),
            "This is a queue of int, but this block needs a list, a deque, a set or a map here.",
        ),
        (
            whole("coll.length", "n"),
            "This is a whole number, but this block needs a list, a deque, a set, a map, a queue, a stack \
             or a priority queue here.",
        ),
    ];
    for (value, message) in values {
        let a = check(vec![
            create("vector<int>", "xs", None),
            create("set<int>", "s", None),
            create("queue<int>", "q", None),
            declare("int", "n", Some(e("0"))),
            print(vec![input(value)]),
        ]);
        assert_eq!(messages(&a, "B2C-E0301"), [message], "{}", render_all(&a));
    }
    let a = check(vec![
        create("vector<int>", "xs", None),
        for_each_pair("xs", "k", "v", Vec::new()),
    ]);
    assert_eq!(
        messages(&a, "B2C-E0301"),
        ["This is a list of int, which doesn't fit this block here."],
        "{}",
        render_all(&a)
    );
}

#[test]
fn parts_must_fit_the_collections_item_and_position_types() {
    let a = check(vec![
        create("vector<int>", "xs", None),
        create("map<std::string,int>", "ages", None),
        add("xs", text("a")),
        add("xs", e("1.5")),
        set_item("xs", text("a"), e("1")),
        set_item("ages", e("1"), e("2")),
        print(vec![input(item("ages", e("3")))]),
        set_item("ages", text("Ann"), text("old")),
    ]);
    assert_eq!(
        messages(&a, "B2C-E0301"),
        [
            "An item of `xs` needs a whole number, but this is text. Text is not turned into a number \
             automatically.",
            "The position in `xs` needs a whole number, but this is text. Text is not turned into a number \
             automatically.",
            "A key of `ages` needs text, but this is a whole number. Use the 'join' block to turn it into \
             text.",
            "A key of `ages` needs text, but this is a whole number. Use the 'join' block to turn it into \
             text.",
            "A value of `ages` needs a whole number, but this is text. Text is not turned into a number \
             automatically.",
        ],
        "{}",
        render_all(&a)
    );
    assert_eq!(
        messages(&a, "B2C-W0518"),
        [
            "An item of `xs` needs a whole number, but this is a decimal number, so the part after the decimal \
          point will be dropped. Use the 'convert' block to show that this is intended."
        ]
    );
    // A position that is a decimal number is narrowed, as a count is.
    let a = check(vec![
        create("vector<int>", "xs", None),
        print(vec![input(item("xs", e("1.5")))]),
    ]);
    assert_eq!(
        messages(&a, "B2C-W0518"),
        [
            "The position in `xs` should be a whole number, but this is a decimal number. Use the 'convert' \
          block to make it a whole number."
        ]
    );
    // A character is assigned to a text item, as `set` allows.
    let a = check(vec![
        create("vector<std::string>", "names", None),
        set_item("names", e("0"), chr("a")),
    ]);
    assert_clean(&a);
}

#[test]
fn read_only_names_and_constants_cannot_be_changed() {
    let a = check(vec![
        create("vector<vector<int>>", "grid", None),
        for_each("row", "read_only", e("grid"), vec![add("row", e("1"))]),
    ]);
    assert_eq!(
        messages(&a, "B2C-E0308"),
        [
            "`row` is read-only: the loop that creates it gives it each item in turn, and the blocks inside \
          can't change it. In a 'for each' loop over a list, choose 'copy' to change a copy of each item, or \
          'editable' to change the items themselves."
        ]
    );
    // The same for a map's key and value, and for `set`.
    let a = check(vec![
        create("map<int,int>", "m", None),
        for_each_pair("m", "k", "v", vec![set("v", e("1")), set("k", e("2"))]),
    ]);
    assert_eq!(codes(&a), ["B2C-E0308", "B2C-E0308"], "{}", render_all(&a));
    // A copy or an editable item can change.
    let a = check(vec![
        create("vector<vector<int>>", "grid", None),
        for_each("row", "copy", e("grid"), vec![add("row", e("1"))]),
        for_each("own", "editable", e("grid"), vec![add("own", e("1"))]),
    ]);
    assert_clean(&a);
    // A constant collection can be read but not changed.
    let mut constant = create("vector<int>", "fixed", Some(input(list_of(vec![e("1")]))));
    constant["fields"]["CONST"] = json!(true);
    let a = check(vec![
        constant,
        print(vec![input(item("fixed", e("0")))]),
        whole("coll.clear", "fixed"),
    ]);
    assert_eq!(
        messages(&a, "B2C-E0308"),
        [
            "`fixed` was created as a constant, so it can't be changed. Untick 'const' in its 'create' block if \
          it needs to change."
        ]
    );
}

#[test]
fn for_each_refuses_a_map_and_a_list_of_without_a_place() {
    let a = check(vec![
        create("map<int,int>", "m", None),
        for_each("x", "read_only", e("m"), Vec::new()),
    ]);
    assert_eq!(
        messages(&a, "B2C-E0301"),
        [
            "`m` is a map from int to int, but this block goes through a list, a deque, a set or text. Use 'for \
          each key, value' for a map."
        ]
    );
    let a = check(vec![for_each(
        "x",
        "read_only",
        input(list_of(vec![e("1")])),
        Vec::new(),
    )]);
    assert_codes(&a, &["B2C-E0315"]);
    // In *print* too: nothing says which type it should have.
    let a = check(vec![print(vec![input(list_of(vec![e("1")]))])]);
    assert_codes(&a, &["B2C-E0315"]);
    // A number is outside the loop's class.
    let a = check(vec![
        declare("int", "n", Some(e("3"))),
        for_each("x", "read_only", e("n"), Vec::new()),
    ]);
    assert_eq!(
        messages(&a, "B2C-E0301"),
        ["`n` is a whole number, but this block needs a list, a deque, a set or text."]
    );
}

#[test]
fn editable_needs_a_changeable_list_or_deque_of_real_items() {
    let editable = |list: Value| for_each("x", "editable", list, Vec::new());
    let a = check(vec![create("vector<bool>", "flags", None), editable(e("flags"))]);
    assert_eq!(
        messages(&a, "B2C-E0301"),
        [
            "The items of a list of true/false values can't be changed one by one in a loop. Choose 'copy', and \
          use 'set item' to change an item."
        ]
    );
    let a = check(vec![
        create("set<int>", "s", None),
        declare("std::string", "word", Some(text("hi"))),
        editable(e("s")),
        editable(e("word")),
    ]);
    assert_eq!(
        messages(&a, "B2C-E0301"),
        [
            "'editable' can only change the items of a list or a deque, but this is a set of int. Choose \
             'read-only' or 'copy'.",
            "'editable' can only change the items of a list or a deque, but this is text. Choose 'read-only' \
             or 'copy'.",
        ]
    );
    let mut constant = create("vector<int>", "fixed", Some(input(list_of(Vec::new()))));
    constant["fields"]["CONST"] = json!(true);
    let a = check(vec![constant, editable(text("abc"))]);
    assert_eq!(
        messages(&a, "B2C-E0308"),
        [
            "'editable' changes the items of a variable, so this block needs a variable here, such as a list. \
          Choose 'read-only' or 'copy' to go through a value."
        ]
    );
    let mut constant = create("deque<int>", "fixed", Some(input(list_of(Vec::new()))));
    constant["fields"]["CONST"] = json!(true);
    let a = check(vec![constant, editable(e("fixed"))]);
    assert_codes(&a, &["B2C-E0308"]);
    let a = check(vec![create("deque<int>", "d", None), editable(e("d"))]);
    assert_clean(&a);
}

#[test]
fn list_of_needs_a_place_that_is_a_listable_collection() {
    let a = check(vec![declare("int", "n", Some(input(list_of(vec![e("1")]))))]);
    assert_eq!(
        messages(&a, "B2C-E0301"),
        ["This block makes a list, a deque or a set, but a whole number is needed here."]
    );
    let a = check(vec![create(
        "map<int,int>",
        "m",
        Some(input(list_of(Vec::new()))),
    )]);
    assert_eq!(
        messages(&a, "B2C-E0301"),
        ["This block makes a list, a deque or a set, but a map from int to int is needed here."]
    );
    // `auto` gives no place.
    let a = check(vec![declare("auto", "x", Some(input(list_of(vec![e("1")]))))]);
    assert_codes(&a, &["B2C-E0315"]);
    // Items are checked against the place's item type.
    let a = check(vec![create(
        "vector<int>",
        "c",
        Some(input(list_of(vec![e("1.5"), text("a"), e("2")]))),
    )]);
    assert_eq!(
        messages(&a, "B2C-W0518"),
        [
            "An item of the list needs a whole number, but this is a decimal number, so the part after the \
          decimal point will be dropped. Use the 'convert' block to show that this is intended."
        ]
    );
    assert_eq!(
        messages(&a, "B2C-E0301"),
        [
            "An item of the list needs a whole number, but this is text. Text is not turned into a number \
          automatically."
        ]
    );
}

#[test]
fn loop_names_exist_only_inside_their_loop() {
    let a = check(vec![
        create("vector<int>", "xs", None),
        for_each("x", "read_only", e("xs"), Vec::new()),
        print(vec![e("x")]),
    ]);
    assert_eq!(
        messages(&a, "B2C-E0203"),
        ["`x` belongs to the loop that creates it and only exists inside that loop."]
    );
    // The loop's list is read before the item exists, so an item named
    // like the list only hides it inside the loop.
    let mut each = for_each("x", "read_only", e("xs"), Vec::new());
    each["fields"]["ITEM"] = json!({"sym": "sym_inner", "name": "xs"});
    let a = check(vec![create("vector<int>", "xs", None), each]);
    assert_codes(&a, &["B2C-W0501"]);
}

#[test]
fn a_collection_cannot_gain_or_lose_items_inside_a_for_each_over_it() {
    // `for (const auto& n : numbers) { numbers.push_back(n); }` and
    // `ids.erase(id)` inside a loop over `ids` are undefined behaviour:
    // the change may free what the loop is going through.
    let sort = |list: &str| {
        json!({"id": id(), "type": "coll.sort", "v": 1,
               "fields": {"LIST": {"ref": sym(list)}, "ORDER": "ascending"}})
    };
    let remove = |list: &str, value: Value| on("coll.remove_value", list, json!({ "VALUE": value }));
    let a = check(vec![
        create("vector<int>", "numbers", None),
        create("vector<int>", "others", None),
        create("set<int>", "ids", None),
        create("map<std::string,int>", "ages", None),
        for_each(
            "n",
            "read_only",
            e("numbers"),
            vec![
                add("numbers", e("n")),
                on("coll.remove_at", "numbers", json!({ "INDEX": e("0") })),
                whole("coll.clear", "numbers"),
                // These change no item's place in the list it goes through.
                set_item("numbers", e("0"), e("n")),
                sort("numbers"),
                add("others", e("n")),
                for_each("m", "copy", e("others"), vec![add("numbers", e("m"))]),
            ],
        ),
        for_each("id", "copy", e("ids"), vec![remove("ids", e("id"))]),
        for_each_pair("ages", "name", "age", vec![remove("ages", e("name"))]),
        // After its loop, a collection may change again.
        add("numbers", e("1")),
        remove("ids", e("1")),
    ]);
    let refused = |name: &str| {
        format!(
            "`{name}` can't gain or lose items inside a 'for each' loop that goes through it: the loop would \
             lose its place. Collect the changes in another collection and make them after the loop."
        )
    };
    assert_eq!(
        messages(&a, "B2C-E0308"),
        [
            refused("numbers"),
            refused("numbers"),
            refused("numbers"),
            refused("numbers"),
            refused("ids"),
            refused("ages"),
        ]
    );
    assert_codes(&a, &["B2C-E0308"; 6]);
    let at: Vec<String> = a
        .diagnostics
        .iter()
        .map(|d| format!("{:?}", d.primary.part))
        .collect();
    assert!(at.iter().all(|part| part.contains("LIST")), "{at:?}");
}

#[test]
fn a_loop_item_is_not_visible_after_its_loop_in_any_mode() {
    // What is visible after a block (where a pasted block or a dropped
    // preset binds its names) holds no loop item, whatever the mode, and an
    // item named like an outer variable does not hide it there.
    for mode in ["read_only", "copy", "editable"] {
        for name in ["n", "x"] {
            let mut each = for_each("x", mode, e("xs"), Vec::new());
            each["fields"]["ITEM"] = json!({"sym": "sym_item", "name": name});
            let each_id = b2c_ir::BlockId::new(each["id"].as_str().unwrap()).unwrap();
            let a = check(vec![
                create("int", "n", Some(num("1"))),
                create("vector<int>", "xs", None),
                each,
            ]);
            let after = a.scope_after(&each_id).unwrap();
            assert_eq!(Some(&after), a.scope_at(&each_id, None).as_ref(), "{mode} {name}");
            let outer: Vec<&str> = after
                .iter()
                .filter(|info| info.name == "n")
                .map(|info| info.id.as_str())
                .collect();
            assert_eq!(outer, [sym("n").as_str()], "{mode} {name}");
            assert!(
                after.iter().all(|info| info.id.as_str() != "sym_item"),
                "{mode} {name}"
            );
        }
    }
}

#[test]
fn a_block_without_its_collection_or_names_is_incomplete() {
    let a = check(vec![
        json!({"id": id(), "type": "coll.add", "v": 1, "inputs": {"VALUE": e("1")}}),
        print(vec![input(json!({"id": id(), "type": "coll.length", "v": 1}))]),
        json!({"id": id(), "type": "coll.for_each_pair", "v": 1}),
    ]);
    assert_eq!(
        messages(&a, "B2C-E0430"),
        [
            "Choose a collection in this block.",
            "Choose a collection in this block.",
            "Choose a map in this block.",
            "This 'for each key, value' loop needs a name for its key.",
            "This 'for each key, value' loop needs a name for its value.",
        ]
    );
    let a = check(vec![
        create("vector<int>", "xs", None),
        json!({"id": id(), "type": "coll.for_each", "v": 1, "inputs": {"LIST": e("xs")}}),
        on("coll.set_item", "xs", json!({})),
    ]);
    assert_eq!(
        messages(&a, "B2C-E0430"),
        [
            "This 'for each' loop needs a name for its item.",
            "This block needs a position or key.",
            "This block needs a value.",
        ]
    );
}

#[test]
fn leave_loop_works_in_a_for_each_loop() {
    let a = check(vec![
        create("vector<int>", "xs", None),
        for_each("x", "read_only", e("xs"), vec![brk(), cont()]),
    ]);
    assert_codes(&a, &["B2C-W0502"]);
}
