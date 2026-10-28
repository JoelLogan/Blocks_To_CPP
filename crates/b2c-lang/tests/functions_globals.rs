//! W2's functions and globals in the analyser (docs/plans/m3-contracts/w2.md
//! §1.9, §2.7, §2.8; package C2): default values of parameters, which a
//! call may leave out (`B2C-E0426` for a parameter without one after one
//! with one); functions of one name that a call can tell apart (overloads,
//! `B2C-E0211` for those it cannot). The blocks come as the project file holds them, analysed with
//! the catalog's signatures: the defaults take their parameters' types
//! (`param:NAME`), which slots.md §5's patterns of M2's blocks do not give,
//! so these tests use [`analyze`], not `analyse`.

// Test code: unwrap/expect/panic and builders that take JSON by value are
// fine here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::needless_pass_by_value
)]

mod common;

use common::*;
use serde_json::Value;

/// The rendered diagnostics, one per line.
fn reported(a: &b2c_lang::Analysis) -> Vec<String> {
    a.diagnostics.iter().map(render).collect()
}

/// `define <name>(params) returns <ret>` whose parameter rows hold these
/// default values (`DEFAULT<i>`, `None` for none).
fn func_with_defaults(
    name: &str,
    ret: &str,
    params: &[(&str, &str, &str)],
    defaults: Vec<Option<Value>>,
    body: Vec<Value>,
) -> Value {
    let mut block = func(name, ret, params, body);
    let inputs: serde_json::Map<String, Value> = defaults
        .into_iter()
        .enumerate()
        .filter_map(|(index, value)| value.map(|value| (format!("DEFAULT{index}"), value)))
        .collect();
    block["inputs"] = Value::Object(inputs);
    block
}

/// `greet(name, times = 1)`, printing the name `times` times.
fn greet() -> Value {
    func_with_defaults(
        "greet",
        "void",
        &[("name", "std::string", "read_only"), ("times", "int", "copy")],
        vec![None, Some(e("1"))],
        vec![repeat(get("times"), vec![print(vec![get("name")])])],
    )
}

/// The function item named `name` as JSON.
fn function_json(a: &b2c_lang::Analysis, function: &str) -> Value {
    let program = serde_json::to_value(&a.program).unwrap();
    program["modules"][0]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| {
            let def = item["kind"].get("function")?;
            (def["symbol"] == sym(function)).then(|| def.clone())
        })
        .unwrap_or_else(|| panic!("no function {function}"))
}

#[test]
fn a_call_may_leave_out_the_arguments_of_parameters_with_a_default() {
    let a = analyze(&doc(vec![
        greet(),
        main(vec![
            call_stmt("greet", vec![e("\"Ann\"")]),
            call_stmt("greet", vec![e("\"Bob\""), e("2")]),
        ]),
    ]));
    assert_clean(&a);
    // The default is the parameter's, lowered with its type.
    let greet = function_json(&a, "greet");
    assert_eq!(greet["params"][0].get("default"), None);
    assert_eq!(greet["params"][1]["default"]["kind"]["int"]["repr"], "1");
    assert_eq!(greet["params"][1]["default"]["ty"], "int");
    // The symbol says how many arguments a call may leave out.
    let symbols = serde_json::to_value(&a.program.symbols).unwrap();
    assert_eq!(
        symbols["symbols"][sym("greet")]["kind"]["function"]["optional"],
        1
    );
}

#[test]
fn a_call_still_needs_the_arguments_without_a_default() {
    let a = analyze(&doc(vec![
        greet(),
        main(vec![
            call_stmt("greet", vec![]),
            call_stmt("greet", vec![e("\"Ann\""), e("2"), e("3")]),
        ]),
    ]));
    let found = reported(&a);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert!(found[0].starts_with("error B2C-E0306"), "{found:#?}");
    assert!(
        found[0].ends_with("`greet` needs 1 to 2 values, but 0 are given."),
        "{found:#?}"
    );
    assert!(
        found[1].ends_with("`greet` needs 1 to 2 values, but 3 are given."),
        "{found:#?}"
    );
    // Every parameter with a default: up to that many.
    let all = func_with_defaults(
        "beep",
        "void",
        &[("times", "int", "copy")],
        vec![Some(e("1"))],
        vec![],
    );
    let a = analyze(&doc(vec![
        all,
        main(vec![
            call_stmt("beep", vec![]),
            call_stmt("beep", vec![e("1"), e("2")]),
        ]),
    ]));
    let found = reported(&a);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0].ends_with("`beep` needs at most 1 value, but 2 are given."),
        "{found:#?}"
    );
}

#[test]
fn a_parameter_without_a_default_after_one_with_a_default_is_reported() {
    let f = func_with_defaults(
        "draw",
        "void",
        &[
            ("width", "int", "copy"),
            ("height", "int", "copy"),
            ("fill", "char", "copy"),
        ],
        vec![None, Some(e("10")), None],
        vec![],
    );
    let a = analyze(&doc(vec![f, main(vec![])]));
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0426 at fn_draw field params[2]: `fill` has no default value, but `height` before it \
             has one. In C++, once an input of `draw` has a default value, every input after it needs one \
             too: give `fill` a default value, or remove the default value of `height`."
        ]
    );
    let diagnostic = only(&a, "B2C-E0426");
    assert_eq!(diagnostic.related.len(), 1);
    assert_eq!(diagnostic.related[0].message, "the input with a default value");
}

#[test]
fn a_default_takes_its_parameters_type() {
    // A whole number for a decimal converts; text for a number does not.
    let f = func_with_defaults(
        "scale",
        "void",
        &[("by", "double", "copy"), ("label", "int", "copy")],
        vec![Some(e("2")), Some(e("\"big\""))],
        vec![],
    );
    let a = analyze(&doc(vec![f, main(vec![])]));
    let found = reported(&a);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0]
            .starts_with("error B2C-E0301 at fn_scale DEFAULT1[0..1]: The default value of `label` needs"),
        "{found:#?}"
    );
}

#[test]
fn a_default_sees_the_modules_functions_but_not_the_parameters() {
    let next = func("next_id", "int", &[], vec![ret(Some(e("7")))]);
    let f = func_with_defaults(
        "make",
        "void",
        &[
            ("first", "int", "copy"),
            ("id", "int", "copy"),
            ("copy", "int", "copy"),
        ],
        vec![None, Some(e("next_id()")), Some(e("first"))],
        vec![],
    );
    let a = analyze(&doc(vec![next, f, main(vec![])]));
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0203 at fn_make DEFAULT2[0..1]: `first` is an input of the function `make`, and a default \
             value can't use the inputs: C++ works it out before the function runs. Use a value, or a constant, \
             instead."
        ]
    );
    // The call to a function prints after its declaration (modules.md §4):
    // `next_id`'s declaration comes before `make`'s, whatever their names.
    let make = function_json(&a, "make");
    assert_eq!(
        make["params"][1]["default"]["kind"]["call"]["function"],
        sym("next_id")
    );
}

#[test]
fn an_editable_parameter_needs_a_variable_as_its_default() {
    let f = func_with_defaults(
        "bump",
        "void",
        &[("count", "int", "editable")],
        vec![Some(e("1"))],
        vec![],
    );
    let a = analyze(&doc(vec![f, main(vec![])]));
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0307 at fn_bump DEFAULT0[0..1]: The `count` input of `bump` is editable (the function may \
             change it), so it needs a variable, not a value or calculation."
        ]
    );
}

#[test]
fn a_function_declaration_follows_what_its_defaults_call() {
    // `a_first` would be declared before `z_last` by name; its default calls
    // `z_last`, so `z_last` comes first.
    let z = func("z_last", "int", &[], vec![ret(Some(e("1")))]);
    let a_first = func_with_defaults(
        "a_first",
        "void",
        &[("n", "int", "copy")],
        vec![Some(e("z_last()"))],
        vec![],
    );
    let a = analyze(&doc(vec![a_first, z, main(vec![])]));
    assert_clean(&a);
    let module = &a.program.modules[0];
    let order: Vec<String> = a.program.layout.modules[0]
        .source_order
        .iter()
        .map(|index| match &module.items[*index].kind {
            b2c_ir::sast::ItemKind::Function(def) => {
                a.program.symbols.get(&def.symbol).unwrap().name.to_string()
            }
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(order, ["z_last", "a_first"]);
    // Two functions whose defaults call each other cannot be declared in
    // any order.
    let first = func_with_defaults(
        "ping",
        "int",
        &[("a", "int", "copy")],
        vec![Some(e("pong()"))],
        vec![ret(Some(e("1")))],
    );
    let second = func_with_defaults(
        "pong",
        "int",
        &[("b", "int", "copy")],
        vec![Some(e("ping()"))],
        vec![ret(Some(e("2")))],
    );
    let a = analyze(&doc(vec![first, second, main(vec![])]));
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0421 at fn_ping: These definitions each need the next one first, in a circle that C++ \
             cannot put in order: `ping` needs `pong` needs `ping`. Change one of them so that it no longer needs \
             the next.",
            "error B2C-E0421 at fn_pong: These definitions each need the next one first, in a circle that C++ \
             cannot put in order: `pong` needs `ping` needs `pong`. Change one of them so that it no longer needs \
             the next.",
        ]
    );
}

/// `define <name>` with symbol `sym_<sym>` (overloads share a name, not a
/// symbol), parameters `(name, type, mode)` with symbols `sym_<sym>_<name>`,
/// and default values.
fn overload(
    name: &str,
    sym: &str,
    params: &[(&str, &str, &str)],
    defaults: Vec<Option<Value>>,
    body: Vec<Value>,
) -> Value {
    let rows: Vec<Value> = params
        .iter()
        .map(|(p, ty, mode)| {
            serde_json::json!({"sym": format!("sym_{sym}_{p}"), "name": p, "type": ty, "mode": mode})
        })
        .collect();
    let inputs: serde_json::Map<String, Value> = defaults
        .into_iter()
        .enumerate()
        .filter_map(|(index, value)| value.map(|value| (format!("DEFAULT{index}"), value)))
        .collect();
    serde_json::json!({
        "id": format!("fn_{sym}"), "type": "func.define", "v": 1,
        "fields": {"NAME": {"sym": format!("sym_{sym}"), "name": name}, "RETURNS": "void"},
        "extra": {"params": rows}, "inputs": inputs, "statements": {"BODY": body}
    })
}

#[test]
fn functions_with_one_name_and_different_inputs_are_overloads() {
    let a = analyze(&doc(vec![
        overload(
            "area",
            "area_i",
            &[("w", "int", "copy"), ("h", "int", "copy")],
            vec![],
            vec![],
        ),
        overload(
            "area",
            "area_d",
            &[("w", "double", "copy"), ("h", "double", "copy")],
            vec![],
            vec![],
        ),
        overload("area", "area_r", &[("r", "double", "copy")], vec![], vec![]),
        main(vec![]),
    ]));
    assert_clean(&a);
}

#[test]
fn functions_a_call_cannot_tell_apart_are_reported() {
    // The same inputs; the same inputs passed differently; a default that
    // makes one call fit both; no inputs at all.
    let cases = [
        (
            overload(
                "area",
                "a1",
                &[("w", "int", "copy"), ("h", "int", "copy")],
                vec![],
                vec![],
            ),
            overload(
                "area",
                "a2",
                &[("x", "int", "read_only"), ("y", "int", "editable")],
                vec![],
                vec![],
            ),
            "error B2C-E0211 at fn_a2 field NAME: Two functions called `area` take the same inputs (int, int), so \
             a call could not tell them apart.",
        ),
        (
            overload("show", "s1", &[("n", "int", "copy")], vec![], vec![]),
            overload(
                "show",
                "s2",
                &[("n", "int", "copy"), ("width", "int", "copy")],
                vec![None, Some(e("0"))],
                vec![],
            ),
            "error B2C-E0211 at fn_s2 field NAME: Two functions called `show` take the same inputs (int), so a \
             call could not tell them apart.",
        ),
        (
            overload("beep", "b1", &[], vec![], vec![]),
            overload(
                "beep",
                "b2",
                &[("times", "int", "copy")],
                vec![Some(e("1"))],
                vec![],
            ),
            "error B2C-E0211 at fn_b2 field NAME: Two functions called `beep` can both be called with no inputs, \
             so a call could not tell them apart.",
        ),
        (
            overload(
                "label",
                "l1",
                &[("text", "std::string", "read_only")],
                vec![],
                vec![],
            ),
            overload("label", "l2", &[("text", "std::string", "copy")], vec![], vec![]),
            "error B2C-E0211 at fn_l2 field NAME: Two functions called `label` take the same inputs \
             (std::string), so a call could not tell them apart.",
        ),
    ];
    for (first, second, expected) in cases {
        let a = analyze(&doc(vec![first, second, main(vec![])]));
        assert_eq!(reported(&a), [expected]);
        let diagnostic = only(&a, "B2C-E0211");
        assert_eq!(diagnostic.related[0].message, "the other function");
    }
}

#[test]
fn a_call_names_its_overload_by_symbol() {
    // `show(int)` and `show(bool)`: a call of each keeps its own function,
    // and its argument is checked against that one's input.
    let a = analyze(&doc(vec![
        overload("show", "show_i", &[("n", "int", "copy")], vec![], vec![]),
        overload("show", "show_b", &[("b", "bool", "copy")], vec![], vec![]),
        main(vec![
            serde_json::json!({"id": "c1", "type": "func.call_stmt", "v": 1,
                               "fields": {"FUNC": {"ref": "sym_show_b"}}, "extra": {"argCount": 1},
                               "inputs": {"ARG0": e("\"hello\"")}}),
            serde_json::json!({"id": "c2", "type": "func.call_stmt", "v": 1,
                               "fields": {"FUNC": {"ref": "sym_show_i"}}, "extra": {"argCount": 1},
                               "inputs": {"ARG0": e("2.5")}}),
        ]),
    ]));
    let found = reported(&a);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert!(
        found[0].starts_with("error B2C-E0301 at c1 ARG0[0..1]: The `b` input of `show`"),
        "{found:#?}"
    );
    assert!(
        found[1].starts_with("warning B2C-W0518 at c2 ARG0[0..1]: The `n` input of `show`"),
        "{found:#?}"
    );
}
