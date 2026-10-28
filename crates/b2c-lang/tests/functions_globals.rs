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

#[test]
fn a_default_cannot_call_its_own_function() {
    // C++ declares `f` only after `int f(int n = f(1))` ends.
    let f = func_with_defaults(
        "f",
        "int",
        &[("n", "int", "copy")],
        vec![Some(e("f(1)"))],
        vec![ret(Some(e("n")))],
    );
    let a = analyze(&doc(vec![f, main(vec![])]));
    let found = reported(&a);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(found[0].starts_with("error B2C-E0421 at fn_f"), "{found:#?}");
    assert!(
        found[0].ends_with(
            "The default value of `n` calls `f` itself, but C++ knows `f` only after the whole first line of its \
             definition, inputs included. Use a value, a constant or another function instead."
        ),
        "{found:#?}"
    );
    // Another function of that name, declared first, is not the same one.
    let none = func("f", "int", &[], vec![ret(Some(e("1")))]);
    let mut one = func_with_defaults(
        "f",
        "int",
        &[("n", "int", "copy"), ("m", "int", "copy")],
        vec![None, Some(e("f()"))],
        vec![ret(Some(e("n + m")))],
    );
    one["id"] = serde_json::json!("fn_f_one");
    one["fields"]["NAME"]["sym"] = serde_json::json!("sym_f_one");
    let a = analyze(&doc(vec![none, one, main(vec![])]));
    assert_clean(&a);
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

// --- Globals and constants (w2.md §2.7) -------------------------------------------

/// *global `kind` `ty` `name` = `value`* (`kind`: `variable` or `constant`).
fn global(kind: &str, ty: &str, name: &str, value: Option<Value>) -> Value {
    let mut block = serde_json::json!({
        "id": format!("gl_{name}"), "type": "var.global", "v": 1,
        "fields": {"KIND": kind, "TYPE": ty, "NAME": {"sym": sym(name), "name": name}}
    });
    if let Some(value) = value {
        block["inputs"] = serde_json::json!({ "VALUE": value });
    }
    block
}

/// A definition whose symbol is `sym_<sym>` but whose name is `name`, so a
/// test can give a global or function the name of an input.
fn renamed(mut block: Value, name: &str) -> Value {
    block["fields"]["NAME"]["name"] = serde_json::json!(name);
    block
}

#[test]
fn a_default_cannot_name_a_global_or_function_called_like_an_input_before_it() {
    let count = || renamed(global("variable", "int", "gcount", Some(e("2"))), "count");
    let next_id = || renamed(func("gnext", "int", &[], vec![ret(Some(e("7")))]), "next_id");
    let hides = "warning B2C-W0501 at fn_f field params[0]";
    // `void f(int count, int n = count)`: C++ reads the input there.
    let f = func_with_defaults(
        "f",
        "void",
        &[("count", "int", "copy"), ("n", "int", "copy")],
        vec![None, Some(e("gcount"))],
        vec![],
    );
    let a = analyze(&doc(vec![count(), f, main(vec![])]));
    let found = reported(&a);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert!(found[0].starts_with(hides), "{found:#?}");
    assert_eq!(
        found[1],
        "error B2C-E0205 at fn_f DEFAULT1[0..1]: This uses the global `count`, but `f` has an input \
         called `count`, and from that input on C++ finds the input by that name, also in default values. Rename \
         the input or the global."
    );
    // `void f(int next_id, int n = next_id())`, at any depth.
    let f = func_with_defaults(
        "f",
        "void",
        &[("next_id", "int", "copy"), ("n", "int", "copy")],
        vec![None, Some(e("1 + gnext()"))],
        vec![],
    );
    let a = analyze(&doc(vec![next_id(), f, main(vec![])]));
    let found = reported(&a);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert!(found[0].starts_with(hides), "{found:#?}");
    assert!(
        found[1].starts_with("error B2C-E0205 at fn_f DEFAULT1"),
        "{found:#?}"
    );
    assert!(
        found[1].contains("This calls the function `next_id`, but `f` has an input called `next_id`"),
        "{found:#?}"
    );
    // An input's own default: g++ lets it through, the standard does not.
    let f = func_with_defaults(
        "f",
        "void",
        &[("count", "int", "copy")],
        vec![Some(e("gcount"))],
        vec![],
    );
    let a = analyze(&doc(vec![count(), f, main(vec![])]));
    assert_codes(&a, &["B2C-W0501", "B2C-E0205"]);
    // An input after the default is not seen there.
    let f = func_with_defaults(
        "f",
        "void",
        &[("n", "int", "copy"), ("count", "int", "copy")],
        vec![Some(e("gcount")), Some(e("1"))],
        vec![],
    );
    let a = analyze(&doc(vec![count(), f, main(vec![])]));
    assert_codes(&a, &["B2C-W0501"]);
}

/// The global items of the first module, by their symbol: (`init` present,
/// `constexpr`).
fn globals_json(a: &b2c_lang::Analysis) -> Vec<(String, bool, bool)> {
    let program = serde_json::to_value(&a.program).unwrap();
    program["modules"][0]["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| {
            let def = item["kind"].get("global")?;
            Some((
                def["symbol"].as_str().unwrap().to_owned(),
                !def["init"].is_null(),
                def["constexpr"].as_bool().unwrap(),
            ))
        })
        .collect()
}

/// The names of the first module's declarations, in the order the layout
/// prints them.
fn declaration_order(a: &b2c_lang::Analysis) -> Vec<String> {
    let module = &a.program.modules[0];
    a.program.layout.modules[0]
        .source_order
        .iter()
        .map(|&item| {
            let symbol = match &module.items[item].kind {
                b2c_ir::sast::ItemKind::Global(def) => &def.symbol,
                b2c_ir::sast::ItemKind::Function(def) => &def.symbol,
                b2c_ir::sast::ItemKind::Struct(def) => &def.symbol,
                b2c_ir::sast::ItemKind::Enum(def) => &def.symbol,
                other => panic!("not a declaration: {other:?}"),
            };
            a.program.symbols.get(symbol).unwrap().name.as_str().to_owned()
        })
        .collect()
}

#[test]
fn globals_are_seen_everywhere_in_their_module() {
    let a = analyze(&doc(vec![
        // `main` and the function come before the globals by block ID.
        main(vec![
            set("count", e("count + kStep")),
            call_stmt("report", vec![]),
        ]),
        func("report", "void", &[], vec![print(vec![e("count"), e("kName")])]),
        global("variable", "int", "count", Some(e("0"))),
        global("constant", "int", "kStep", Some(e("2"))),
        global("constant", "double", "kRate", Some(e("1.5"))),
        global("constant", "std::string", "kName", Some(e("\"Ann\""))),
        global("variable", "bool", "done", None),
    ]));
    assert_clean(&a);
    // A constant of a scalar type with a constant value is `constexpr`;
    // text is `const`, a variable neither.
    assert_eq!(
        globals_json(&a),
        [
            (sym("count"), true, false),
            (sym("done"), false, false),
            (sym("kName"), true, false),
            (sym("kRate"), true, true),
            (sym("kStep"), true, true),
        ]
    );
    let symbols = serde_json::to_value(&a.program.symbols).unwrap();
    assert_eq!(
        symbols["symbols"][sym("kStep")]["kind"],
        serde_json::json!({"global": {"is_const": true}})
    );
    // The scope query lists them in the function, with the module's
    // functions.
    let block = b2c_ir::BlockId::new("fn_report").unwrap();
    let names: Vec<String> = a
        .symbols_in_scope(&block, None)
        .into_iter()
        .map(|info| info.name)
        .collect();
    assert_eq!(names, ["count", "done", "kName", "kRate", "kStep", "report"]);
}

#[test]
fn a_constant_folds_into_a_case_label_beside_an_enum_switch() {
    let a = analyze(&doc(vec![
        serde_json::json!({"id": "en_Mode", "type": "type.enum", "v": 1,
                           "fields": {"NAME": {"sym": sym("Mode"), "name": "Mode"}},
                           "extra": {"values": [{"sym": sym("Mode_Fast"), "name": "Fast"},
                                                {"sym": sym("Mode_Slow"), "name": "Slow"}]}}),
        global("constant", "int", "kLimit", Some(e("kBase * 2"))),
        global("constant", "int", "kBase", Some(e("5"))),
        main(vec![
            declare("int", "n", Some(e("10"))),
            serde_json::json!({"id": "sw", "type": "control.switch", "v": 1,
                               "extra": {"caseCount": 2, "hasDefault": false},
                               "inputs": {"VALUE": e("n"), "CASE0": e("kLimit"), "CASE1": e("kBase")},
                               "statements": {"DO0": [print(vec![text("limit")])],
                                              "DO1": [print(vec![text("base")])]}}),
        ]),
    ]));
    assert_clean(&a);
    let constants = a.constants();
    let value = |name: &str| constants[&b2c_ir::SymbolId::new(&sym(name)).unwrap()];
    assert_eq!(value("kLimit").value, b2c_lang::ConstValue::Int(10));
    assert!(value("kLimit").constant_expression);
    // Two labels of one value are still told apart (`B2C-E0425`).
    let a = analyze(&doc(vec![
        global("constant", "int", "kTen", Some(e("10"))),
        main(vec![
            declare("int", "n", Some(e("10"))),
            serde_json::json!({"id": "sw", "type": "control.switch", "v": 1,
                               "extra": {"caseCount": 2, "hasDefault": false},
                               "inputs": {"VALUE": e("n"), "CASE0": e("kTen"), "CASE1": e("10")},
                               "statements": {"DO0": [], "DO1": []}}),
        ]),
    ]));
    assert_codes(&a, &["B2C-E0425"]);
    // A global variable is no constant.
    let a = analyze(&doc(vec![
        global("variable", "int", "ten", Some(e("10"))),
        main(vec![
            serde_json::json!({"id": "sw", "type": "control.switch", "v": 1,
                                     "extra": {"caseCount": 1, "hasDefault": false},
                                     "inputs": {"VALUE": e("1"), "CASE0": e("ten")},
                                     "statements": {"DO0": []}}),
        ]),
    ]));
    assert_codes(&a, &["B2C-E0424"]);
}

#[test]
fn a_constant_needs_a_value_and_never_changes() {
    let a = analyze(&doc(vec![
        global("constant", "int", "kMax", None),
        global("constant", "int", "kMin", Some(e("0"))),
        main(vec![serde_json::json!({"id": "b0", "type": "var.set", "v": 1,
                                     "fields": {"VAR": {"ref": sym("kMin")}}, "inputs": {"VALUE": e("1")}})]),
    ]));
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0430 at gl_kMax input VALUE: `kMax` is a constant, so it needs a value: give it one, or \
             choose 'variable'.",
            "error B2C-E0308 at b0 field VAR: `kMin` is a global constant, so it can't be changed. Choose \
             'variable' in its 'global' block if it needs to change.",
        ]
    );
}

#[test]
fn a_local_or_an_input_named_like_a_global_hides_it() {
    let a = analyze(&doc(vec![
        global("variable", "int", "count", Some(e("0"))),
        global("constant", "int", "kSize", Some(e("3"))),
        serde_json::json!({"id": "fn_show", "type": "func.define", "v": 1,
                           "fields": {"NAME": {"sym": sym("show"), "name": "show"}, "RETURNS": "void"},
                           "extra": {"params": [{"sym": sym("size_input"), "name": "kSize", "type": "int",
                                                 "mode": "copy"}]},
                           "statements": {"BODY": []}}),
        main(vec![
            serde_json::json!({"id": "b0", "type": "var.declare", "v": 1,
                               "fields": {"TYPE": "int", "NAME": {"sym": sym("count_local"), "name": "count"}},
                               "inputs": {"VALUE": e("1")}}),
        ]),
    ]));
    let found = reported(&a);
    assert_eq!(
        found,
        [
            "warning B2C-W0501 at fn_show field params[0]: `kSize` has the same name as a global constant, so \
             that global constant can't be used where `kSize` exists. This works, but it is easy to mix them \
             up, so consider another name.",
            "warning B2C-W0501 at b0 field NAME: `count` has the same name as a global variable, so that global \
             variable can't be used where `count` exists. This works, but it is easy to mix them up, so \
             consider another name.",
        ],
        "{found:#?}"
    );
    assert_eq!(a.diagnostics[0].related[0].message, "the global constant");
}

#[test]
fn a_global_has_a_name_of_its_own() {
    let a = analyze(&doc(vec![
        serde_json::json!({"id": "gl_area", "type": "var.global", "v": 1,
                           "fields": {"KIND": "variable", "TYPE": "int",
                                      "NAME": {"sym": sym("area_global"), "name": "area"}}}),
        func("area", "int", &[], vec![ret(Some(e("1")))]),
        global("variable", "int", "total", Some(e("0"))),
        serde_json::json!({"id": "gl_total2", "type": "var.global", "v": 1,
                           "fields": {"KIND": "variable", "TYPE": "int",
                                      "NAME": {"sym": sym("total2"), "name": "total"}}}),
        main(vec![]),
    ]));
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0210 at gl_area field NAME: There is already a function called `area`, and C++ would \
             not know which of the two the name means. Rename the global or the other one.",
            "error B2C-E0210 at gl_total2 field NAME: There is already a global called `total`. Rename one of \
             them.",
        ]
    );
}

#[test]
fn a_global_set_from_a_function_follows_the_globals_it_reads() {
    // `int a = next();` with `next` reading `b`: `b` is printed first,
    // though `a` comes first by name (modules.md §4.1, *initialised*).
    let a = analyze(&doc(vec![
        global("variable", "int", "a", Some(e("next()"))),
        global("variable", "int", "b", Some(e("41"))),
        func("next", "int", &[], vec![ret(Some(e("b + 1")))]),
        global("constant", "int", "kZero", Some(e("0"))),
        main(vec![print(vec![e("a")])]),
    ]));
    assert_clean(&a);
    assert_eq!(declaration_order(&a), ["kZero", "next", "b", "a"]);
}

#[test]
fn globals_that_need_each_other_are_a_circle() {
    let a = analyze(&doc(vec![
        global("variable", "int", "x", Some(e("y + 1"))),
        global("variable", "int", "y", Some(e("twice()"))),
        func("twice", "int", &[], vec![ret(Some(e("x * 2")))]),
        global("variable", "int", "z", Some(e("z + 1"))),
        main(vec![]),
    ]));
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0421 at gl_x: These definitions each need the next one first, in a circle that C++ \
             cannot put in order: `x` needs `y` needs `x`. Change one of them so that it no longer needs the \
             next.",
            "error B2C-E0421 at gl_y: These definitions each need the next one first, in a circle that C++ \
             cannot put in order: `y` needs `x` needs `y`. Change one of them so that it no longer needs the \
             next.",
            "error B2C-E0421 at gl_z: `z` needs its own value to be set first, which C++ cannot do. Change it \
             so that it no longer needs itself.",
        ]
    );
}

// --- change's new forms, flip and the program's arguments (w2.md §1.9) ------------------

/// *flip* `name`.
fn flip(id: &str, name: &str) -> Value {
    serde_json::json!({"id": id, "type": "var.flip", "v": 1, "fields": {"VAR": {"ref": sym(name)}}})
}

/// *change* `name` with the operator `op` by `by`, with its own block ID.
fn change_with(id: &str, name: &str, op: &str, by: Value) -> Value {
    serde_json::json!({"id": id, "type": "var.change", "v": 1,
                       "fields": {"VAR": {"ref": sym(name)}, "OP": op}, "inputs": {"BY": by}})
}

#[test]
fn text_is_added_to_the_end_and_whole_numbers_change_bitwise() {
    let a = analyze(&doc(vec![main(vec![
        declare("std::string", "s", Some(e("\"ab\""))),
        declare("int", "n", Some(e("6"))),
        declare("bool", "done", Some(e("false"))),
        change_with("c1", "s", "append", e("\"cd\"")),
        change_with("c2", "s", "append", e("'e'")),
        change_with("c3", "n", "and", e("3")),
        change_with("c4", "n", "shl", e("'a'")),
        flip("f1", "done"),
    ])]));
    assert_clean(&a);
    let stmts = main_json(&a);
    assert_eq!(stmts[3]["kind"]["compound_assign"]["op"], "append");
    assert_eq!(stmts[5]["kind"]["compound_assign"]["op"], "bit_and");
    assert_eq!(stmts[7]["kind"]["flip"]["target"], sym("done"));
}

#[test]
fn each_form_of_change_takes_its_own_types() {
    let a = analyze(&doc(vec![main(vec![
        declare("std::string", "s", Some(e("\"ab\""))),
        declare("double", "x", Some(e("1.5"))),
        declare("int", "n", Some(e("6"))),
        change_with("c1", "n", "append", e("\"cd\"")),
        change_with("c2", "s", "append", e("3")),
        change_with("c3", "x", "xor", e("1")),
        change_with("c4", "n", "or", e("1.5")),
        change_with("c5", "s", "add", e("1")),
    ])]));
    let found = reported(&a);
    assert_eq!(found.len(), 5, "{found:#?}");
    assert_eq!(
        found[0],
        "error B2C-E0302 at c1: `n` holds a whole number, and 'add to the end' only works on text. Choose \
         another operation in this block's menu."
    );
    assert!(
        found[1].starts_with(
            "error B2C-E0301 at c2 BY[0..1]: Only text or a character can be added to the end of `s`"
        ),
        "{found:#?}"
    );
    assert!(
        found[2].starts_with("error B2C-E0302 at c3: `x` holds a decimal number, and the bitwise operations"),
        "{found:#?}"
    );
    assert!(
        found[3].starts_with("error B2C-E0301 at c4 BY[0..1]: The amount must be a whole number"),
        "{found:#?}"
    );
    assert!(
        found[4].ends_with("To add to the end of the text, choose 'add to the end' in this block's menu."),
        "{found:#?}"
    );
}

#[test]
fn only_a_true_false_variable_that_may_change_is_flipped() {
    let a = analyze(&doc(vec![main(vec![
        declare("int", "n", Some(e("1"))),
        declare_const("bool", "fixed", e("true")),
        flip("f1", "n"),
        flip("f2", "fixed"),
    ])]));
    let found = reported(&a);
    assert_eq!(found.len(), 2, "{found:#?}");
    assert_eq!(
        found[0],
        "error B2C-E0301 at f1 field VAR: Only a true/false variable can be flipped, but `n` holds a whole \
         number."
    );
    assert!(
        found[1].starts_with("error B2C-E0308 at f2 field VAR: `fixed` was created as a constant"),
        "{found:#?}"
    );
}

/// *when program starts with arguments* `args` (when `with`), with `body`.
fn main_with_args(with: bool, body: Vec<Value>) -> Value {
    serde_json::json!({"id": "main", "type": "program.main", "v": 1,
                       "fields": {"ARGS": with, "ARGV": {"sym": sym("args"), "name": "args"}},
                       "statements": {"BODY": body}})
}

#[test]
fn the_program_reads_its_arguments_but_never_changes_them() {
    let a = analyze(&doc(vec![main_with_args(
        true,
        vec![declare("auto", "copy", Some(e("args")))],
    )]));
    assert_clean(&a);
    let program = serde_json::to_value(&a.program).unwrap();
    let main_item = &program["modules"][0]["items"][0]["kind"]["main"];
    assert_eq!(main_item["args"], sym("args"));
    let symbols = serde_json::to_value(&a.program.symbols).unwrap();
    assert_eq!(
        symbols["symbols"][sym("args")]["ty"],
        serde_json::json!({"std::vector": "std::string"})
    );
    // Read-only.
    let a = analyze(&doc(vec![main_with_args(
        true,
        vec![
            serde_json::json!({"id": "s1", "type": "var.set", "v": 1, "fields": {"VAR": {"ref": sym("args")}},
                                "inputs": {"VALUE": e("args")}}),
        ],
    )]));
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0308 at s1 field VAR: `args` holds the program's arguments and is read-only. Create a \
          variable set to it to change a copy."
        ]
    );
    // Without the tick there are none.
    let a = analyze(&doc(vec![main_with_args(false, vec![print(vec![e("args")])])]));
    assert_codes(&a, &["B2C-E0201"]);
    // Ticked without a name.
    let a = analyze(&doc(vec![
        serde_json::json!({"id": "main", "type": "program.main", "v": 1,
                                                 "fields": {"ARGS": true}}),
    ]));
    assert_eq!(
        reported(&a),
        ["error B2C-E0430 at main field ARGV: The program's arguments need a name."]
    );
}

/// `create int args = 0` with its own symbol, beside the program's
/// arguments `args`.
fn local_args() -> Value {
    serde_json::json!({"id": "local", "type": "var.declare", "v": 1,
                       "fields": {"TYPE": "int", "NAME": {"sym": "sym_local_args", "name": "args"}},
                       "inputs": {"VALUE": e("0")}})
}

#[test]
fn a_local_at_the_top_of_main_cannot_take_the_arguments_name() {
    // C++ declares the arguments in `main`'s own block: a second `args`
    // there does not compile.
    let a = analyze(&doc(vec![main_with_args(true, vec![local_args()])]));
    assert_codes(&a, &["B2C-E0210"]);
    // In a nested block it hides them, as C++ allows.
    let a = analyze(&doc(vec![main_with_args(
        true,
        vec![if_then(e("true"), vec![local_args()])],
    )]));
    assert_codes(&a, &["B2C-W0501"]);
    // Without the tick there are no arguments to clash with.
    let a = analyze(&doc(vec![main_with_args(false, vec![local_args()])]));
    assert_clean(&a);
}

// --- Struct field defaults (w2.md §1.9) ---------------------------------------------------

/// *struct* `name` with these `(field, type, default)` rows; a field's
/// symbol is `sym("<name>_<field>")`.
fn struct_with_defaults(name: &str, rows: &[(&str, &str, Option<Value>)]) -> Value {
    let fields: Vec<Value> = rows
        .iter()
        .map(|(field, ty, _)| serde_json::json!({"sym": sym(&format!("{name}_{field}")), "name": field, "type": ty}))
        .collect();
    let inputs: serde_json::Map<String, Value> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, (_, _, value))| value.clone().map(|value| (format!("DEFAULT{i}"), value)))
        .collect();
    serde_json::json!({
        "id": format!("st_{name}"), "type": "type.struct", "v": 1,
        "fields": {"NAME": {"sym": sym(name), "name": name}},
        "extra": {"fields": fields},
        "inputs": inputs
    })
}

/// The default values of the struct item `name`, by field symbol.
fn struct_defaults_json(a: &b2c_lang::Analysis, name: &str) -> Value {
    let program = serde_json::to_value(&a.program).unwrap();
    program["modules"][0]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|item| {
            let def = item["kind"].get("struct")?;
            (def["symbol"] == sym(name)).then(|| def.get("defaults").cloned().unwrap_or(Value::Null))
        })
        .unwrap_or_else(|| panic!("no struct {name}"))
}

#[test]
fn a_fields_default_takes_its_own_rows_type() {
    let student = struct_with_defaults(
        "Student",
        &[
            ("name", "std::string", Some(text("Ann"))),
            ("mark", "int", Some(e("0"))),
            ("passed", "bool", None),
        ],
    );
    let a = analyze(&doc(vec![student, main(vec![])]));
    assert_clean(&a);
    let defaults = struct_defaults_json(&a, "Student");
    let fields: Vec<&String> = defaults.as_object().unwrap().keys().collect();
    assert_eq!(fields, [&sym("Student_mark"), &sym("Student_name")]);
    assert_eq!(defaults[sym("Student_mark")]["ty"], "int");

    // A default of another type is reported against its own field.
    let student = struct_with_defaults(
        "Student",
        &[
            ("name", "std::string", Some(e("1"))),
            ("mark", "int", Some(text("high"))),
        ],
    );
    let a = analyze(&doc(vec![student, main(vec![])]));
    let lines = reported(&a);
    assert_eq!(lines.len(), 2, "{lines:#?}");
    assert!(
        lines.iter().all(|line| line.starts_with("error B2C-E0301")),
        "{lines:#?}"
    );
    assert!(lines[0].contains("The default value of `name`"), "{lines:#?}");
    assert!(lines[1].contains("The default value of `mark`"), "{lines:#?}");
}

#[test]
fn a_fields_default_sees_the_modules_globals_and_functions() {
    let config = struct_with_defaults(
        "Config",
        &[
            ("limit", "int", Some(e("kMax"))),
            ("start", "int", Some(e("first()"))),
        ],
    );
    let a = analyze(&doc(vec![
        config,
        global("constant", "int", "kMax", Some(e("10"))),
        func("first", "int", &[], vec![ret(Some(e("1")))]),
        main(vec![]),
    ]));
    assert_clean(&a);
    // A name the module does not define is reported as anywhere else.
    let config = struct_with_defaults("Config", &[("limit", "int", Some(e("missing")))]);
    let a = analyze(&doc(vec![config, main(vec![])]));
    assert_codes(&a, &["B2C-E0201"]);
}

#[test]
fn a_fields_default_cannot_name_a_global_or_function_called_like_a_field() {
    // `int max = limit;` inside `Config` reads the field `limit`, not the
    // global: before or after it, and at any depth.
    let limit = || renamed(global("variable", "int", "glimit", Some(e("100"))), "limit");
    let expected = "error B2C-E0205 at st_Config DEFAULT1[0..1]: This uses the global `limit`, but `Config` has a \
                    field called `limit`, and inside a struct C++ finds the field by that name, also in default \
                    values. Rename the field or the global.";
    for rows in [
        [("limit", Some(e("5"))), ("max", Some(e("glimit")))],
        [("max", None), ("limit", Some(e("glimit * 2")))],
    ] {
        let rows: Vec<(&str, &str, Option<Value>)> = rows.into_iter().map(|(n, v)| (n, "int", v)).collect();
        let config = struct_with_defaults("Config", &rows);
        let a = analyze(&doc(vec![limit(), config, main(vec![])]));
        let found = reported(&a);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert!(
            found[0].starts_with("error B2C-E0205 at st_Config DEFAULT1"),
            "{found:#?}"
        );
        assert!(
            found[0].ends_with(&expected[expected.find(':').unwrap()..]),
            "{found:#?}"
        );
    }
    let config = struct_with_defaults(
        "Config",
        &[("limit", "int", Some(e("5"))), ("max", "int", Some(e("glimit")))],
    );
    let a = analyze(&doc(vec![limit(), config, main(vec![])]));
    assert_eq!(reported(&a), [expected]);
    // A function: `int id = next_id();` beside a field `next_id`.
    let next_id = renamed(func("gnext", "int", &[], vec![ret(Some(e("7")))]), "next_id");
    let config = struct_with_defaults(
        "Config",
        &[("next_id", "int", None), ("id", "int", Some(e("gnext()")))],
    );
    let a = analyze(&doc(vec![next_id, config, main(vec![])]));
    let found = reported(&a);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0].contains("This calls the function `next_id`, but `Config` has a field called `next_id`"),
        "{found:#?}"
    );
}

/// *new P with* these parts (`FIELD<i>`, `None` for an empty one), as the
/// value of `create P p`.
fn create_new_p(parts: Vec<Option<Value>>) -> Value {
    let count = parts.len();
    let inputs: serde_json::Map<String, Value> = parts
        .into_iter()
        .enumerate()
        .filter_map(|(index, part)| part.map(|part| (format!("FIELD{index}"), part)))
        .collect();
    let made = serde_json::json!({"id": "new_p", "type": "type.new", "v": 1,
                                  "fields": {"TYPE": {"ref": sym("P")}},
                                  "extra": {"fieldCount": count}, "inputs": inputs});
    serde_json::json!({"id": id(), "type": "var.declare", "v": 1,
                       "fields": {"TYPE": {"ref": sym("P")}, "NAME": {"sym": sym("p"), "name": "p"}},
                       "inputs": {"VALUE": {"block": made}}})
}

#[test]
fn in_cpp17_new_cannot_leave_a_defaulted_field_empty_before_a_filled_one() {
    use b2c_ir::sast::CppStandard::{Cpp17, Cpp20};
    // `struct P { int x = 10; int y{}; int z = 3; };`
    let p = || {
        struct_with_defaults(
            "P",
            &[
                ("x", "int", Some(e("10"))),
                ("y", "int", None),
                ("z", "int", Some(e("3"))),
            ],
        )
    };
    let analyse = |standard: b2c_ir::sast::CppStandard, parts: Vec<Option<Value>>| {
        let mut document = doc(vec![p(), main(vec![create_new_p(parts)])]);
        document.project.language.standard = standard;
        analyze(&document)
    };
    // `P{{}, 5}` would make x 0 in C++17; `P{.y = 5}` keeps 10 in C++20.
    let a = analyse(Cpp17, vec![None, Some(e("5")), None]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0430 at new_p input FIELD0: In C++17, an empty part before a filled one sets its field to an \
             empty value, so `x` would not get its default value. Fill in this part, or choose C++20 in the \
             project's settings."
        ]
    );
    assert_clean(&analyse(Cpp20, vec![None, Some(e("5")), None]));
    // Empty parts after the last filled one, and a gap over a field
    // without a default, are the same in both standards.
    assert_clean(&analyse(Cpp17, vec![Some(e("1")), None, None]));
    assert_clean(&analyse(Cpp17, vec![Some(e("1")), None, Some(e("4"))]));
}

#[test]
fn a_global_built_with_a_structs_defaults_follows_the_globals_they_read() {
    // `Config a{};` runs `int limit = next();`, and `next` reads `z`: `z`
    // is printed before `a`, though `a` comes first by name (modules.md
    // §4.1, *initialised*, through a default member initialiser of the type
    // the global builds).
    let config = struct_with_defaults("Config", &[("limit", "int", Some(e("next()")))]);
    let mut a_global = global("variable", "int", "a", None);
    a_global["fields"]["TYPE"] = serde_json::json!({"ref": sym("Config")});
    let a = analyze(&doc(vec![
        a_global,
        config,
        func("next", "int", &[], vec![ret(Some(e("z + 1")))]),
        global("variable", "int", "z", Some(e("5"))),
        main(vec![]),
    ]));
    assert_clean(&a);
    assert_eq!(declaration_order(&a), ["next", "Config", "z", "a"]);
}
