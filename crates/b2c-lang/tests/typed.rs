//! Types of milestone M3 in the analyser (ADR-0017, ADR-0018): type terms in
//! type fields and parameter rows, references to user types, the type rules
//! of library types (comparisons, arithmetic, `B2C-E0313`, `B2C-E0314`),
//! typed slots (`value_block`'s place, `B2C-E0315`, the four steps, slot
//! types, unchecked blocks) and the catalog headers of items, with
//! hand-built signature tables (the catalog fills its own in package 2).

// Test code: unwrap/expect/panic and printing to stderr are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use b2c_ir::diag::Part;
use b2c_ir::headers::Header;
use b2c_ir::ids::BlockId;
use b2c_ir::sast::{ItemKind, StmtKind};
use b2c_ir::sig::{BlockSig, InputSig, PartName, SigTable, TypePattern, TypeVar};
use b2c_ir::types::{LibType, Type, TypeClass};
use b2c_lang::{Analysis, analyze_checked_with, analyze_with};
use common::*;
use serde_json::{Value, json};

fn vector(of: Type) -> Type {
    Type::lib(LibType::Vector, vec![of]).unwrap()
}

/// A `create` block whose type is this term.
fn declare_term(term: Value, name: &str, value: Option<Value>) -> Value {
    let mut block = declare("int", name, value);
    block["fields"]["TYPE"] = term;
    block
}

fn int_list() -> Value {
    json!({"std::vector": "int"})
}

/// Analyses a document with a signature table only (no M2 comparison).
fn run_with(blocks: Vec<Value>, sigs: &SigTable) -> Analysis {
    analyze_with(&doc(blocks), sigs)
}

/// The type of the symbol `sym_<name>`.
fn symbol_type(a: &Analysis, name: &str) -> Type {
    a.program
        .symbols
        .symbols
        .iter()
        .find(|(id, _)| id.as_str() == sym(name))
        .map_or_else(|| panic!("no symbol {name}"), |(_, symbol)| symbol.ty.clone())
}

/// Messages of the diagnostics with this code.
fn messages(a: &Analysis, code: &str) -> Vec<String> {
    a.diagnostics
        .iter()
        .filter(|d| d.code.0 == code)
        .map(|d| d.message.clone())
        .collect()
}

// --- Type terms and references -----------------------------------------------

#[test]
fn type_fields_and_rows_read_type_terms() {
    let a = run(vec![
        main(vec![
            declare_term(int_list(), "xs", None),
            declare_term(
                json!({"std::map": ["std::string", {"std::vector": "double"}]}),
                "m",
                None,
            ),
        ]),
        func("f", "int", &[], vec![ret(Some(num("0")))]),
    ]);
    assert_clean(&a);
    assert_eq!(symbol_type(&a, "xs"), vector(Type::Int));
    assert_eq!(
        symbol_type(&a, "m").label(&a.program.symbols).to_string(),
        "map from string to list of double"
    );

    // A parameter row and a result with a template.
    let mut function = func(
        "total",
        "int",
        &[("marks", "int", "read_only")],
        vec![ret(Some(num("0")))],
    );
    function["extra"]["params"][0]["type"] = int_list();
    function["fields"]["RETURNS"] = json!({"std::vector": "int"});
    let a = run(vec![main(vec![]), function]);
    assert_eq!(symbol_type(&a, "marks"), vector(Type::Int));
    assert_eq!(symbol_type(&a, "total"), vector(Type::Int));
    // `return 0` in a function giving back a list does not convert.
    assert_codes(&a, &["B2C-E0301"]);
}

#[test]
fn types_a_variable_cannot_hold_are_refused() {
    let a = run_main(vec![
        declare_term(json!("std::ifstream"), "f", None),
        declare_term(json!({"std::vector": "std::ofstream"}), "g", None),
        declare_term(json!("void"), "v", None),
        declare_term(json!("float"), "h", None),
    ]);
    let found = messages(&a, "B2C-E0430");
    assert_eq!(
        found,
        [
            "`std::ifstream` is not a type this block can use.",
            "`{\"std::vector\":\"std::ofstream\"}` is not a type this block can use.",
            "A variable can't have the type 'nothing' (void). Choose another type.",
            "`float` is not a type this block can use.",
        ]
    );
    let mut function = func("f", "void", &[("p", "int", "copy")], vec![]);
    function["extra"]["params"][0]["type"] = json!("void");
    let a = run(vec![main(vec![]), function]);
    assert_eq!(
        messages(&a, "B2C-E0430"),
        ["`void` is not a type that a function input can have."]
    );
}

#[test]
fn references_to_user_types_must_name_one() {
    // A variable chosen as a type.
    let a = run_main(vec![
        declare("int", "n", Some(num("1"))),
        declare_term(json!({"ref": sym("n")}), "x", None),
        declare_term(json!({"std::vector": {"ref": sym("n")}}), "y", None),
    ]);
    let found = messages(&a, "B2C-E0207");
    assert_eq!(found.len(), 2, "{}", render_all(&a));
    assert!(found[0].contains("`n` is a variable, not a type"), "{}", found[0]);
    assert_eq!(symbol_type(&a, "x"), Type::Error);
    // A struct that does not exist (any more).
    let a = run_main(vec![declare_term(json!({"ref": "sym_gone"}), "x", None)]);
    let d = only(&a, "B2C-E0201");
    assert!(
        d.message.contains("struct or enum that doesn't exist"),
        "{}",
        d.message
    );
    assert_eq!(
        d.primary.part,
        Part::Field {
            name: String::from("TYPE")
        }
    );
}

#[test]
fn set_items_and_map_keys_must_be_comparable() {
    let a = run_main(vec![declare_term(
        json!({"std::set": {"std::priority_queue": "int"}}),
        "s",
        None,
    )]);
    let d = only(&a, "B2C-E0313");
    assert_eq!(
        d.message,
        "A set keeps its items in order, so they must be comparable, and a priority queue of int is not. Choose \
         another type, or use a list."
    );
    assert_eq!(
        d.primary.part,
        Part::Field {
            name: String::from("TYPE")
        }
    );
    let a = run_main(vec![declare_term(
        json!({"std::vector": {"std::map": [{"std::priority_queue": "int"}, "int"]}}),
        "m",
        None,
    )]);
    assert!(
        only(&a, "B2C-E0313")
            .message
            .starts_with("A map keeps its keys in order")
    );
    // Comparable keys are fine, nested containers included.
    let a = run_main(vec![declare_term(
        json!({"std::set": {"std::vector": {"std::deque": "std::string"}}}),
        "fine",
        None,
    )]);
    assert_clean(&a);
}

// --- Type rules -----------------------------------------------------------------

/// `xs` a list of int, `ys` a list of double, `zs` another list of int.
fn lists() -> Vec<Value> {
    vec![
        declare_term(int_list(), "xs", None),
        declare_term(json!({"std::vector": "double"}), "ys", None),
        declare_term(int_list(), "zs", None),
        declare_term(json!({"std::priority_queue": "int"}), "pq", None),
        declare("int", "n", Some(num("1"))),
    ]
}

#[test]
fn comparisons_of_library_types_need_one_comparable_type() {
    let mut body = lists();
    body.push(print(vec![compare("eq", get("xs"), get("zs"))]));
    body.push(print(vec![compare("lt", get("xs"), get("zs"))]));
    assert_clean(&run_main(body));

    let mut body = lists();
    body.push(print(vec![compare("eq", get("xs"), get("ys"))]));
    body.push(print(vec![compare("eq", get("pq"), get("pq"))]));
    body.push(print(vec![compare("lt", get("xs"), get("n"))]));
    body.push(print(vec![compare("eq", text("a"), get("xs"))]));
    let a = run_main(body);
    assert_eq!(
        messages(&a, "B2C-E0302"),
        [
            "Can't compare a list of int with a list of double.",
            "Values of this type can't be compared: a priority queue of int isn't comparable.",
            "Can't compare a list of int with a whole number.",
            "Can't compare text with a list of int.",
        ]
    );
    assert_eq!(a.diagnostics.len(), 4, "{}", render_all(&a));
}

#[test]
fn arithmetic_and_logic_need_numbers_or_true_false_values() {
    let mut body = lists();
    body.extend([
        print(vec![arith("add", get("xs"), num("1"))]),
        print(vec![arith("mod", get("n"), get("xs"))]),
        print(vec![not(get("xs"))]),
        print(vec![logic("and", vec![get("xs"), boolean(true)])]),
        print(vec![e("-xs")]),
        change("xs", num("1")),
        change("n", get("xs")),
    ]);
    let a = run_main(body);
    assert_eq!(
        messages(&a, "B2C-E0302"),
        [
            "Can't add a list of int: + only works with numbers.",
            "'mod' only works with whole numbers, not a list of int.",
            "'not' needs a true/false value, but this is a list of int.",
            "'and' and 'or' need true/false values, but this is a list of int.",
            "The + and - signs only work with numbers, but this is a list of int.",
            "`xs` holds a list of int, so it can't be changed by a number.",
        ]
    );
    assert_eq!(
        messages(&a, "B2C-E0301"),
        ["The amount must be a number, but this is a list of int."]
    );
}

#[test]
fn library_types_convert_only_to_themselves() {
    let mut body = lists();
    body.extend([
        declare_term(int_list(), "copy", Some(get("xs"))),
        declare_term(int_list(), "wrong", Some(get("ys"))),
        declare("int", "count", Some(get("xs"))),
        set("xs", get("zs")),
        set("xs", num("3")),
        repeat(get("xs"), vec![]),
        if_then(get("xs"), vec![]),
        ask("xs", None),
        print(vec![convert("int", get("xs"))]),
    ]);
    let a = run_main(body);
    assert_eq!(
        messages(&a, "B2C-E0301"),
        [
            "`wrong` needs a list of int, but this is a list of double.",
            "`count` needs a whole number, but this is a list of int.",
            "`xs` needs a list of int, but this is a whole number.",
            "The number of times to repeat needs a whole number, but this is a list of int.",
            "The condition needs a true/false value, but this is a list of int.",
            "`xs` holds a list of int, but 'ask' can only fill in a number, a character, text or a true/false \
             value.",
            "Only numbers can be converted to a whole number, but this is a list of int.",
        ]
    );
    assert_eq!(a.diagnostics.len(), 7, "{}", render_all(&a));
}

#[test]
fn print_and_join_need_printable_values() {
    let mut body = lists();
    body.push(print(vec![get("xs"), get("n")]));
    body.push(print(vec![join(vec![text("all: "), get("pq")])]));
    let a = run_main(body);
    assert_eq!(
        messages(&a, "B2C-E0314"),
        [
            "A list can't be printed in one go. Use 'for each' to go through its items.",
            "A priority queue can't be joined into text in one go. Take its items out one by one instead.",
        ]
    );
}

#[test]
fn a_decimal_in_an_integer_input_is_a_warning() {
    let a = run_main(vec![repeat(num("2.5"), vec![])]);
    assert_codes(&a, &["B2C-W0518"]);
}

// --- Typed slots -----------------------------------------------------------------

fn var(name: &str) -> TypeVar {
    TypeVar::new(name).unwrap()
}

fn pattern(text: &str) -> TypePattern {
    TypePattern::parse(text).unwrap()
}

/// M2's patterns, with `text.join` made place-typed: `where = { C =
/// "listable" }`, output `C`, items `elem(C)`. Its arm still builds a join,
/// which is enough to drive `value_block`'s place.
fn with_place_typed_join() -> SigTable {
    let mut sigs = m2_sigs();
    sigs.insert(
        "text.join",
        BlockSig {
            vars: BTreeMap::from([(var("C"), TypeClass::Listable)]),
            output: Some(pattern("C")),
            inputs: BTreeMap::from([(
                "ITEM".into(),
                InputSig {
                    check: TypeClass::Any,
                    pattern: Some(pattern("elem(C)")),
                    assign: false,
                },
            )]),
            order: vec![PartName::Input("ITEM".into())],
            place_var: Some(var("C")),
            repeated: BTreeSet::from([String::from("ITEM")]),
            ..BlockSig::default()
        },
    );
    sigs
}

/// A `join` block with this ID.
fn join_with_id(id: &str, items: Vec<Value>) -> Value {
    let mut block = join(items);
    block["block"]["id"] = json!(id);
    block
}

#[test]
fn m2_patterns_go_through_type_block() {
    // With `assign` taken off `set`'s value, a `char` no longer fits a text
    // variable: the patterns decide, not the M2 rule.
    let mut sigs = m2_sigs();
    let mut set_sig = sigs.get("var.set").unwrap().clone();
    set_sig.inputs.get_mut("VALUE").unwrap().assign = false;
    sigs.insert("var.set", set_sig);
    let blocks = vec![main(vec![
        declare("std::string", "s", Some(text("a"))),
        set("s", chr("b")),
    ])];
    assert_clean(&analyze_with(&doc(blocks.clone()), &m2_sigs()));
    let a = run_with(blocks, &sigs);
    assert_eq!(
        messages(&a, "B2C-E0301"),
        ["`s` needs text, but this is a character. Use the 'join' block to turn it into text."]
    );
}

#[test]
fn a_place_typed_block_takes_its_type_from_its_place() {
    let sigs = with_place_typed_join();
    let a = run_with(
        vec![main(vec![
            declare_term(
                int_list(),
                "xs",
                Some(join_with_id("j1", vec![num("1"), num("2")])),
            ),
            declare("int", "n", Some(join_with_id("j2", vec![num("1"), num("2")]))),
            print(vec![join_with_id("j3", vec![num("1"), num("2")])]),
            declare("auto", "a", Some(join_with_id("j4", vec![num("1"), num("2")]))),
        ])],
        &sigs,
    );
    let types = a.block_types();
    let ty = |id: &str| types.get(&BlockId::new(id).unwrap()).cloned();
    assert_eq!(ty("j1"), Some(vector(Type::Int)));
    assert_eq!(ty("j2"), Some(Type::Error));
    let found: Vec<String> = a.diagnostics.iter().map(render).collect();
    assert_eq!(found.len(), 3, "{}", found.join("\n"));
    assert!(found[0].starts_with("error B2C-E0301 at j2: This block makes a list, a deque or a set, but a whole number is needed here."), "{}", found[0]);
    assert!(
        found[1].starts_with("error B2C-E0315 at j3: This block takes its type from where it is plugged in"),
        "{}",
        found[1]
    );
    assert!(found[2].starts_with("error B2C-E0315 at j4:"), "{}", found[2]);
    assert_eq!(symbol_type(&a, "a"), Type::Error);
}

#[test]
fn an_empty_binder_gives_no_place_error() {
    // `set [ ] to (list of …)`: only the missing variable is reported.
    let mut set_block = set("x", join_with_id("j1", vec![num("1"), num("2")]));
    set_block.as_object_mut().unwrap().remove("fields");
    let a = run_with(vec![main(vec![set_block])], &with_place_typed_join());
    assert_codes(&a, &["B2C-E0430"]);
}

#[test]
fn a_place_typed_value_is_lowered_after_its_siblings_at_its_own_scope_position() {
    let sigs = with_place_typed_join();
    // `f(join(1.5 + "a"), "b" - 1)`: the join's own error comes after the
    // second argument's, though it comes first in the block.
    let blocks = vec![
        main(vec![call_stmt(
            "f",
            vec![join_with_id("j1", vec![e("1 - \"a\""), num("2")]), e("\"b\" - 1")],
        )]),
        func("f", "void", &[("p", "int", "copy"), ("q", "int", "copy")], vec![]),
    ];
    let plain = analyze_with(&doc(blocks.clone()), &m2_sigs());
    let typed = run_with(blocks, &sigs);
    let parts = |a: &Analysis| -> Vec<String> {
        a.diagnostics
            .iter()
            .map(|d| {
                format!(
                    "{} {}",
                    d.code.0,
                    d.primary.block.as_ref().map_or("", |b| b.as_str())
                )
            })
            .collect()
    };
    assert_eq!(parts(&plain)[0], "B2C-E0302 j1");
    assert_eq!(parts(&typed)[0], parts(&plain)[1]);
    assert_eq!(parts(&typed)[1], "B2C-E0302 j1");
    let at = |a: &Analysis| a.symbols_in_scope(&BlockId::new("j1").unwrap(), None);
    assert_eq!(at(&typed), at(&plain));
}

#[test]
fn unchecked_blocks_check_nothing_at_their_parts_or_values() {
    let sigs = with_place_typed_join();
    let blocks = vec![main(vec![
        declare("int", "n", Some(text("text"))),
        print(vec![join_with_id("j1", vec![num("1"), num("2")])]),
    ])];
    let document = doc(blocks);
    let ids: Vec<BlockId> = common::all_blocks(&document)
        .into_iter()
        .filter(|(_, _, block)| matches!(block.block_type.as_str(), "var.declare" | "io.print"))
        .map(|(_, _, block)| block.id.clone())
        .collect();
    let checked = analyze_checked_with(&document, &[], &BTreeSet::new(), &sigs);
    assert_codes(&checked, &["B2C-E0301", "B2C-E0315"]);
    let unchecked: BTreeSet<BlockId> = ids.into_iter().collect();
    let a = analyze_checked_with(&document, &[], &unchecked, &sigs);
    assert_clean(&a);
    // An unchecked value is `Error`, so its parent checks nothing against it.
    let value = common::all_blocks(&document)
        .into_iter()
        .find(|(_, _, block)| block.block_type == "text.literal")
        .map(|(_, _, block)| block.id.clone())
        .unwrap();
    let a = analyze_checked_with(&document, &[], &BTreeSet::from([value.clone()]), &sigs);
    assert_codes(&a, &["B2C-E0315"]);
    assert_eq!(a.block_types().get(&value), Some(&Type::Error));
}

#[test]
fn an_editable_argument_keeps_its_exact_rule() {
    let function = func("bump", "void", &[("v", "int", "editable")], vec![]);
    for arg in [e("1.5"), chr("a"), num("1"), e("x + 1")] {
        let a = run(vec![
            main(vec![
                declare("int", "x", Some(num("0"))),
                call_stmt("bump", vec![arg]),
            ]),
            function.clone(),
        ]);
        assert_codes(&a, &["B2C-E0307"]);
    }
    let a = run(vec![
        main(vec![
            declare("double", "d", Some(num("0"))),
            call_stmt("bump", vec![get("d")]),
        ]),
        function,
    ]);
    assert_codes(&a, &["B2C-E0307"]);
}

#[test]
fn slot_types_list_each_expression_slot_lowered() {
    let a = run_main(vec![
        declare("double", "x", Some(e("1 + 2"))),
        declare("int", "y", Some(num("3"))),
        print(vec![e("\"hi\""), get("x")]),
    ]);
    let slots = a.slot_types();
    let body = match &a.program.modules[0].items[0].kind {
        ItemKind::Main(main) => &main.body.stmts,
        ItemKind::Function(_) => panic!("main"),
    };
    let block_of = |i: usize| body[i].origin.block.clone();
    assert_eq!(
        slots[&block_of(0)],
        BTreeMap::from([(String::from("VALUE"), Type::Int)])
    );
    assert_eq!(
        slots[&block_of(2)],
        BTreeMap::from([(String::from("ITEM0"), Type::String)])
    );
    // A block in an input is not a slot.
    assert!(!slots.contains_key(&block_of(1)));
    assert!(matches!(body[1].kind, StmtKind::VarDecl(_)));
    assert_eq!(slots.len(), 2);
}

// --- Headers -----------------------------------------------------------------

fn header(name: &str) -> Header {
    Header::standard(name).unwrap()
}

/// Signatures with headers: `when program starts` and `define` (printed
/// with the declaration), `print`, and `arithmetic` whose `div` option adds
/// one.
fn header_sigs() -> SigTable {
    let mut sigs = SigTable::empty();
    let with = |headers: &[&str]| BlockSig {
        headers: headers.iter().map(|name| header(name)).collect(),
        ..BlockSig::default()
    };
    sigs.insert("program.main", with(&["<cstdlib>"]));
    sigs.insert("func.define", with(&["<string>"]));
    sigs.insert("io.print", with(&["<iostream>"]));
    let mut arithmetic = with(&[]);
    arithmetic.option_headers = BTreeMap::from([(
        String::from("OP"),
        BTreeMap::from([(String::from("div"), vec![header("<cmath>")])]),
    )]);
    sigs.insert("math.arithmetic", arithmetic);
    sigs
}

#[test]
fn item_headers_come_from_the_blocks_lowered_into_them() {
    let mut disabled = print(vec![num("1")]);
    disabled["disabled"] = json!(true);
    let a = run_with(
        vec![
            main(vec![print(vec![arith("div", num("1"), num("2"))]), disabled]),
            func("quiet", "void", &[], vec![]),
            func(
                "noisy",
                "int",
                &[],
                vec![print(vec![arith("add", num("1"), num("2"))]), ret(Some(num("0")))],
            ),
            // Parked: not lowered, so no headers.
            json!({"id": "parked", "type": "io.print", "v": 1, "extra": {"itemCount": 1},
                   "inputs": {"ITEM0": arith("div", num("1"), num("2"))}}),
        ],
        &header_sigs(),
    );
    let items = &a.program.modules[0].items;
    let headers = |i: usize| {
        let item = &items[i];
        let names = |set: &BTreeSet<Header>| set.iter().map(|h| h.as_str()).collect::<Vec<_>>();
        (names(&item.headers.decl), names(&item.headers.body))
    };
    let index = |name: &str| {
        items
            .iter()
            .position(|item| match &item.kind {
                ItemKind::Function(function) => function.symbol.as_str() == sym(name),
                ItemKind::Main(_) => name == "main",
            })
            .unwrap()
    };
    assert_eq!(
        headers(index("main")),
        (vec!["<cstdlib>"], vec!["<cmath>", "<iostream>"])
    );
    assert_eq!(headers(index("quiet")), (vec!["<string>"], vec![]));
    // A body-only header never reaches `decl`; an option not chosen adds nothing.
    assert_eq!(headers(index("noisy")), (vec!["<string>"], vec!["<iostream>"]));
    // Without signatures, no headers.
    let plain = run(vec![main(vec![print(vec![num("1")])])]);
    assert!(plain.program.modules[0].items[0].headers.is_empty());
}
