//! W2's control and file blocks in the analyser
//! (docs/plans/m3-contracts/w2.md §1.6, §1.8, §2.4, §2.9; package D): *switch
//! on* an `int` and a `char`, its labels and parts, *leave loop* in a case,
//! the paths through a switch; *do … then repeat*; *for each line in file*.
//! The blocks come as the project file holds them, analysed with the
//! catalog's signatures. The switch on an enum takes package C1's enum
//! blocks: its labels are enum values, a repeated one is `B2C-E0425`, and
//! without a default part a missed value is `B2C-W0508`. Package S's
//! if-time blocks: *use header*, *check at compile time*, *open file*, *read
//! line from … into*, *write … to file* and *file … exists*.

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

use b2c_ir::ids::BlockId;
use b2c_ir::types::Type;
use common::*;
use serde_json::{Map, Value, json};

/// The rendered diagnostics, one per line.
fn reported(a: &b2c_lang::Analysis) -> Vec<String> {
    a.diagnostics.iter().map(render).collect()
}

/// *switch on (value)* with one case per `(label, statements)` (`None` for
/// an empty label), and a default part when `default` is given.
fn switch(value: Value, cases: Vec<(Option<Value>, Vec<Value>)>, default: Option<Vec<Value>>) -> Value {
    let mut inputs = Map::new();
    let mut statements = Map::new();
    inputs.insert(String::from("VALUE"), value);
    let count = cases.len();
    for (index, (label, body)) in cases.into_iter().enumerate() {
        if let Some(label) = label {
            inputs.insert(format!("CASE{index}"), label);
        }
        statements.insert(format!("DO{index}"), Value::Array(body));
    }
    let has_default = default.is_some();
    if let Some(body) = default {
        statements.insert(String::from("DEFAULT"), Value::Array(body));
    }
    json!({"id": id(), "type": "control.switch", "v": 1,
           "extra": {"caseCount": count, "hasDefault": has_default},
           "inputs": inputs, "statements": statements})
}

/// *switch on* with labelled cases only.
fn switch_cases(value: Value, cases: Vec<(Value, Vec<Value>)>, default: Option<Vec<Value>>) -> Value {
    switch(
        value,
        cases
            .into_iter()
            .map(|(label, body)| (Some(label), body))
            .collect(),
        default,
    )
}

/// *do (body) then repeat while/until (cond)*.
fn do_while(mode: &str, cond: Value, body: Vec<Value>) -> Value {
    json!({"id": id(), "type": "control.do_while", "v": 1, "fields": {"MODE": mode},
           "inputs": {"COND": cond}, "statements": {"BODY": body}})
}

/// *for each line [name] in file (path)* with `body`, and its *if it can't
/// be opened* part when `failed` is given.
fn file_lines(name: &str, path: Value, body: Vec<Value>, failed: Option<Vec<Value>>) -> Value {
    let mut block = json!({"id": id(), "type": "file.for_each_line", "v": 1,
                           "extra": {"hasFailed": failed.is_some()},
                           "fields": {"LINE": {"sym": sym(name), "name": name}},
                           "inputs": {"PATH": path}, "statements": {"BODY": body}});
    if let Some(failed) = failed {
        block["statements"]["FAILED"] = Value::Array(failed);
    }
    block
}

// --- switch on ----------------------------------------------------------------------

#[test]
fn a_switch_on_a_whole_number_or_a_character_is_clean() {
    let a = run_main(vec![
        declare("int", "n", Some(e("2"))),
        switch_cases(
            e("n"),
            vec![
                (e("1"), vec![print(vec![e("\"one\"")])]),
                (e("'A'"), vec![]),
                (e("1 + 2"), vec![print(vec![e("n")])]),
            ],
            Some(vec![print(vec![e("\"other\"")])]),
        ),
        declare("char", "c", Some(e("'x'"))),
        switch_cases(e("c"), vec![(e("'x'"), vec![]), (e("'y'"), vec![])], None),
    ]);
    assert_clean(&a);
    let stmts = main_json(&a);
    let cases = stmts[1]["kind"]["switch"]["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 3);
    assert!(stmts[1]["kind"]["switch"]["default"].is_object());
    assert!(stmts[3]["kind"]["switch"]["default"].is_null());
}

#[test]
fn a_switch_takes_whole_numbers_characters_and_enums_only() {
    for (ty, value, shown) in [
        ("bool", "true", "a true/false value"),
        ("double", "1.5", "a decimal number"),
        ("std::string", "\"a\"", "text"),
    ] {
        let a = run_main(vec![
            declare(ty, "v", Some(e(value))),
            switch_cases(e("v"), vec![(e("1"), vec![])], Some(vec![])),
        ]);
        let reported = reported(&a);
        assert_eq!(codes(&a)[0], "B2C-E0301", "{reported:?}");
        assert!(reported[0].contains(shown), "{reported:?}");
    }
}

#[test]
fn case_labels_are_typed_by_the_value_switched_on() {
    let a = run_main(vec![
        declare("int", "n", Some(e("2"))),
        switch_cases(e("n"), vec![(e("\"one\""), vec![])], None),
    ]);
    // Refused as text, and not again as a value that is not fixed.
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0301 at b0002 CASE0[0..1]: The value of case 1 needs a whole number, but this is text. Text \
          is not turned into a number automatically."
        ],
    );
}

#[test]
fn an_empty_case_label_is_reported_and_its_part_still_lowered() {
    let a = run_main(vec![
        declare("int", "n", Some(e("2"))),
        switch(
            e("n"),
            vec![(Some(e("1")), vec![]), (None, vec![print(vec![e("missing")])])],
            None,
        ),
    ]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0430 at b0003 input CASE1: This block needs the value of case 2.",
            "error B2C-E0201 at b0002 ITEM0[0..1]: This refers to a variable or function that doesn't exist (any \
             more). Choose another one, or create it again.",
        ]
    );
}

#[test]
fn case_labels_must_be_fixed_and_different() {
    let a = run_main(vec![
        declare("int", "n", Some(e("2"))),
        declare_const("int", "k", e("65")),
        switch_cases(
            e("n"),
            vec![
                (e("n"), vec![]),
                (e("'A'"), vec![]),
                (e("k"), vec![]),
                (e("k + 1"), vec![]),
            ],
            None,
        ),
    ]);
    // `n` is a variable; 'A' and the constant `k` are both 65; `k + 1` is
    // a calculation of constants, which C++ takes.
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0424 at b0003 CASE0[0..1]: A 'case' label must be a value that is fixed when the program \
             is built: a whole number, a character, a value of an enum, or a constant whole number or character \
             with a fixed value.",
            "error B2C-E0425 at b0003 CASE2[0..1]: Another 'case' of this switch has the same value (65). Each \
             case needs its own value: remove one of them, or put their blocks together in one case.",
        ]
    );
}

#[test]
fn each_case_is_a_scope_of_its_own() {
    // A second `amount`, with a symbol of its own, read by the name its
    // symbol gives in the tokens (`amount2` is its symbol's key).
    let mut second_amount = declare("int", "amount", Some(e("2")));
    second_amount["fields"]["NAME"]["sym"] = json!(sym("amount2"));
    let a = run_main(vec![
        declare("int", "n", Some(e("2"))),
        switch_cases(
            e("n"),
            vec![
                (e("1"), vec![declare("int", "amount", Some(e("1")))]),
                (e("2"), vec![second_amount, print(vec![e("amount2")])]),
            ],
            Some(vec![print(vec![e("amount")])]),
        ),
    ]);
    // Both cases create `amount`; the default part sees neither.
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0203 at b0005 ITEM0[0..1]: `amount` can't be used here: it only exists from the block that \
          creates it to the end of that group of blocks (including the blocks nested inside them)."
        ]
    );
}

#[test]
fn leave_loop_in_a_case_would_only_leave_the_switch() {
    let message = "error B2C-E0401";
    // Directly in a case, and inside an `if` in a case.
    for body in [vec![brk()], vec![if_then(e("true"), vec![brk()])]] {
        let a = run_main(vec![
            declare("int", "n", Some(e("2"))),
            repeat(
                e("3"),
                vec![switch_cases(e("n"), vec![(e("1"), body)], Some(vec![]))],
            ),
        ]);
        assert_eq!(codes(&a), ["B2C-E0401"], "{}", render_all(&a));
        assert!(reported(&a)[0].starts_with(message));
        assert!(
            reported(&a)[0]
                .contains("In C++ 'leave loop' here would only leave the switch, not the loop around it.")
        );
    }
    // In the default part too.
    let a = run_main(vec![
        declare("int", "n", Some(e("2"))),
        repeat(
            e("3"),
            vec![switch_cases(e("n"), vec![(e("1"), vec![])], Some(vec![brk()]))],
        ),
    ]);
    assert_codes(&a, &["B2C-E0401"]);
    // Inside a loop in a case it leaves that loop; *skip to next round*
    // reaches the loop around the switch.
    let a = run_main(vec![
        declare("int", "n", Some(e("2"))),
        forever(vec![
            switch_cases(
                e("n"),
                vec![(e("1"), vec![forever(vec![brk()])]), (e("2"), vec![cont()])],
                Some(vec![]),
            ),
            brk(),
        ]),
    ]);
    assert_clean(&a);
}

#[test]
fn a_switch_whose_parts_all_return_ends_every_path_only_with_a_default() {
    let returning = |default: Option<Vec<Value>>| {
        func(
            "pick",
            "int",
            &[("n", "int", "copy")],
            vec![switch_cases(
                e("n"),
                vec![
                    (e("1"), vec![ret(Some(e("10")))]),
                    (e("2"), vec![ret(Some(e("20")))]),
                ],
                default,
            )],
        )
    };
    let a = run(vec![returning(Some(vec![ret(Some(e("0")))])), main(vec![])]);
    assert_clean(&a);
    let a = run(vec![returning(None), main(vec![])]);
    assert_codes(&a, &["B2C-E0410"]);
}

// --- switch on an enum ---------------------------------------------------------------

/// The symbol of an enum's value.
fn value_sym(owner: &str, name: &str) -> String {
    sym(&format!("{owner}_{name}"))
}

/// *enum* `name` with these values.
fn enumeration(name: &str, values: &[&str]) -> Value {
    let rows: Vec<Value> = values
        .iter()
        .map(|value| json!({"sym": value_sym(name, value), "name": value}))
        .collect();
    json!({"id": format!("en_{name}"), "type": "type.enum", "v": 1,
           "fields": {"NAME": {"sym": sym(name), "name": name}}, "extra": {"values": rows}})
}

/// *`owner` `value`*, an enum value block in an input.
fn enum_value(owner: &str, value: &str) -> Value {
    input(json!({"id": id(), "type": "type.enum_value", "v": 1,
                 "fields": {"ENUM": {"ref": sym(owner)}, "VALUE": {"ref": value_sym(owner, value)}}}))
}

/// *create* `name` of the enum `owner`, set to its value `value`.
fn create_enum(owner: &str, name: &str, value: &str) -> Value {
    let mut block = declare("int", name, Some(enum_value(owner, value)));
    block["fields"]["TYPE"] = json!({"ref": sym(owner)});
    block
}

/// The enum `Action` of the bank menu assignment, short of its last value.
fn action() -> Value {
    enumeration("Action", &["Deposit", "Withdraw", "ShowBalance", "Quit"])
}

/// A program with `Action` and a `main` that creates `act` (`Deposit`) and
/// switches on it with a case for each of `values`, and a default part when
/// `default` is given.
fn switch_on_action(values: &[&str], default: Option<Vec<Value>>) -> b2c_lang::Analysis {
    let cases = values
        .iter()
        .map(|value| (enum_value("Action", value), vec![print(vec![text(value)])]))
        .collect();
    run(vec![
        action(),
        main(vec![
            create_enum("Action", "act", "Deposit"),
            switch_cases(e("act"), cases, default),
        ]),
    ])
}

#[test]
fn a_switch_on_an_enum_takes_its_values_as_labels() {
    let a = switch_on_action(&["Deposit", "Withdraw", "ShowBalance", "Quit"], None);
    assert_clean(&a);
    // Each label is the enumerator itself, typed by its enum.
    let stmts = main_json(&a);
    let cases = stmts[1]["kind"]["switch"]["cases"].as_array().unwrap();
    let labels: Vec<&Value> = cases
        .iter()
        .map(|case| &case["label"]["kind"]["enumerator"])
        .collect();
    let expected: Vec<Value> = ["Deposit", "Withdraw", "ShowBalance", "Quit"]
        .iter()
        .map(|value| json!(value_sym("Action", value)))
        .collect();
    assert_eq!(labels, expected.iter().collect::<Vec<_>>(), "{stmts}");
    assert!(stmts[1]["kind"]["switch"]["default"].is_null());
    for case in cases {
        assert_eq!(case["label"]["ty"], json!({"ref": sym("Action")}), "{case}");
    }
}

#[test]
fn a_label_of_an_enum_switch_is_a_value_of_that_enum() {
    // A whole number, a value of another enum, and a variable of the enum.
    let a = run(vec![
        action(),
        enumeration("Size", &["Small"]),
        main(vec![
            create_enum("Action", "act", "Deposit"),
            create_enum("Action", "other", "Quit"),
            switch_cases(
                e("act"),
                vec![
                    (e("1"), vec![]),
                    (enum_value("Size", "Small"), vec![]),
                    (e("other"), vec![]),
                ],
                Some(vec![]),
            ),
        ]),
    ]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0301 at b0006 CASE0[0..1]: The value of case 1 needs an `Action`, but this is a whole \
             number.",
            "error B2C-E0301 at b0005: The value of case 2 needs an `Action`, but this is a `Size`.",
            "error B2C-E0424 at b0006 CASE2[0..1]: A 'case' label must be a value that is fixed when the program \
             is built: a whole number, a character, a value of an enum, or a constant whole number or character \
             with a fixed value.",
        ]
    );
}

#[test]
fn a_repeated_enum_value_is_one_case_too_many() {
    let a = switch_on_action(&["Deposit", "Withdraw", "Deposit"], Some(vec![]));
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0425 at b0007: Another 'case' of this switch has the same value (Deposit). Each case \
             needs its own value: remove one of them, or put their blocks together in one case."
        ]
    );
}

#[test]
fn a_switch_on_an_enum_that_misses_one_value_warns_without_a_default_part() {
    let a = switch_on_action(&["Deposit", "Withdraw", "ShowBalance"], None);
    assert_eq!(
        reported(&a),
        [
            "warning B2C-W0508 at b0012: This switch on `Action` has no case for `Quit`, and no default part, \
             so for that value none of its parts runs. Add a case for each value, or a default part."
        ]
    );
}

#[test]
fn a_switch_on_an_enum_that_misses_several_values_names_them_all() {
    let a = switch_on_action(&["Withdraw"], None);
    assert_eq!(
        reported(&a),
        [
            "warning B2C-W0508 at b0006: This switch on `Action` has no case for `Deposit`, `ShowBalance` and \
             `Quit`, and no default part, so for those values none of its parts runs. Add a case for each \
             value, or a default part."
        ]
    );
    let a = switch_on_action(&["Withdraw", "ShowBalance"], None);
    assert_eq!(codes(&a), ["B2C-W0508"]);
    assert!(
        reported(&a)[0]
            .contains("has no case for `Deposit` and `Quit`, and no default part, so for those values"),
        "{:?}",
        reported(&a)
    );
}

#[test]
fn a_default_part_or_every_value_leaves_nothing_missed() {
    let a = switch_on_action(&["Deposit"], Some(vec![print(vec![text("other")])]));
    assert_clean(&a);
    // An empty default part counts too.
    let a = switch_on_action(&["Deposit"], Some(vec![]));
    assert_clean(&a);
    let a = switch_on_action(&["Quit", "ShowBalance", "Withdraw", "Deposit"], None);
    assert_clean(&a);
    // A switch on a whole number never misses values.
    let a = run_main(vec![
        declare("int", "n", Some(e("2"))),
        switch_cases(e("n"), vec![(e("1"), vec![])], None),
    ]);
    assert_clean(&a);
}

#[test]
fn cases_that_return_end_every_path_only_with_a_default_part() {
    // bank_menu's to_action: a switch on a whole number whose parts all
    // return a value of the enum.
    let to_action = |default: Option<Vec<Value>>| {
        let mut define = func(
            "to_action",
            "int",
            &[("choice", "int", "copy")],
            vec![switch_cases(
                e("choice"),
                vec![
                    (e("1"), vec![ret(Some(enum_value("Action", "Deposit")))]),
                    (e("2"), vec![ret(Some(enum_value("Action", "Withdraw")))]),
                    (e("3"), vec![ret(Some(enum_value("Action", "ShowBalance")))]),
                    (e("4"), vec![ret(Some(enum_value("Action", "Quit")))]),
                ],
                default,
            )],
        );
        define["fields"]["RETURNS"] = json!({"ref": sym("Action")});
        define
    };
    let a = run(vec![
        action(),
        to_action(Some(vec![ret(Some(enum_value("Action", "Deposit")))])),
        main(vec![]),
    ]);
    assert_clean(&a);
    let a = run(vec![action(), to_action(None), main(vec![])]);
    assert_codes(&a, &["B2C-E0410"]);
    // A switch on an enum with a case for every value that returns still
    // has a path past it without a default part, as in C++: the value may
    // be none of the enum's values.
    let mut name_of = func(
        "name_of",
        "std::string",
        &[("act", "int", "copy")],
        vec![switch_cases(
            e("act"),
            ["Deposit", "Withdraw", "ShowBalance", "Quit"]
                .iter()
                .map(|value| (enum_value("Action", value), vec![ret(Some(text(value)))]))
                .collect(),
            None,
        )],
    );
    name_of["extra"]["params"][0]["type"] = json!({"ref": sym("Action")});
    let a = run(vec![action(), name_of, main(vec![])]);
    assert_codes(&a, &["B2C-E0410"]);
}

// --- do … then repeat -----------------------------------------------------------------

#[test]
fn do_while_runs_its_body_before_the_condition() {
    let a = run_main(vec![
        declare("int", "count", Some(e("0"))),
        do_while("while", e("count < 3"), vec![change("count", e("1"))]),
        do_while("until", e("count >= 6"), vec![change("count", e("1"))]),
        print(vec![e("count")]),
    ]);
    assert_clean(&a);
    let stmts = main_json(&a);
    assert_eq!(stmts[1]["kind"]["do_while"]["until"], json!(false));
    assert_eq!(stmts[2]["kind"]["do_while"]["until"], json!(true));
    // A variable without a value, set in the body, has one in the
    // condition: the body runs at least once.
    let a = run_main(vec![
        declare("int", "answer", None),
        do_while("until", e("answer > 0"), vec![set("answer", e("5"))]),
    ]);
    assert_clean(&a);
}

#[test]
fn the_condition_of_do_while_does_not_see_the_bodys_variables() {
    let a = run_main(vec![do_while(
        "while",
        e("inner < 3"),
        vec![declare("int", "inner", Some(e("1")))],
    )]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0203 at b0002 COND[0..1]: `inner` can't be used here: it only exists from the block that \
          creates it to the end of that group of blocks (including the blocks nested inside them)."
        ]
    );
}

#[test]
fn leave_loop_in_do_while_leaves_it() {
    let a = run_main(vec![
        do_while("while", e("true"), vec![brk()]),
        print(vec![e("1")]),
    ]);
    assert_clean(&a);
}

// --- for each line in file ------------------------------------------------------------

#[test]
fn a_file_loop_gives_its_body_a_text_line() {
    let a = run_main(vec![
        declare("int", "count", Some(e("0"))),
        file_lines(
            "line",
            e("\"poem.txt\""),
            vec![change("count", e("1")), print(vec![e("line")])],
            Some(vec![exit(e("1"))]),
        ),
        print(vec![e("count")]),
    ]);
    assert_clean(&a);
    let stmts = main_json(&a);
    let lines = &stmts[1]["kind"]["file_lines"];
    assert!(lines["failed"].is_object(), "{lines}");
    let line = lines["line"].as_str().unwrap();
    let symbol = &a.program.symbols.symbols[&b2c_ir::ids::SymbolId::new(line).unwrap()];
    assert_eq!(symbol.ty, Type::String);
    let _ = BlockId::new("main").unwrap();
}

#[test]
fn the_line_is_visible_in_the_body_only_and_cannot_be_changed() {
    // After the loop, and in the part for a file that can't be opened.
    let a = run_main(vec![
        file_lines(
            "line",
            e("\"poem.txt\""),
            vec![],
            Some(vec![print(vec![e("line")])]),
        ),
        print(vec![e("line")]),
    ]);
    let outside = "`line` belongs to the loop that creates it and only exists inside that loop.";
    assert_eq!(
        reported(&a),
        [
            format!("error B2C-E0203 at b0001 ITEM0[0..1]: {outside}"),
            format!("error B2C-E0203 at b0003 ITEM0[0..1]: {outside}"),
        ]
    );
    let a = run_main(vec![file_lines(
        "line",
        e("\"poem.txt\""),
        vec![set("line", e("\"x\""))],
        None,
    )]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0308 at b0004 field VAR: `line` is read-only: it holds the line just read from the file, \
          and the blocks inside the loop can't change it. Create a variable set to it to change a copy."
        ]
    );
}

#[test]
fn the_path_is_text_and_file_loops_nest() {
    let a = run_main(vec![file_lines("line", e("42"), vec![], None)]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0301 at b0001 PATH[0..1]: This is a whole number, but this block needs text or a character."
        ]
    );
    let a = run_main(vec![
        declare("std::string", "name", Some(e("\"words.txt\""))),
        file_lines(
            "line",
            e("name"),
            vec![file_lines("word", e("line"), vec![print(vec![e("word")])], None)],
            None,
        ),
    ]);
    assert_clean(&a);
}

#[test]
fn a_file_loop_without_a_name_for_its_line_is_incomplete() {
    let mut block = file_lines("line", e("\"poem.txt\""), vec![], None);
    block["fields"] = json!({});
    let a = run_main(vec![block]);
    assert_eq!(
        reported(&a),
        ["error B2C-E0430 at b0001 field LINE: This 'for each line' loop needs a name for its line."]
    );
}

// --- go on into the next case, wait ----------------------------------------------------

fn fallthrough() -> Value {
    json!({"id": id(), "type": "control.fallthrough", "v": 1})
}

fn wait(seconds: Value) -> Value {
    json!({"id": id(), "type": "control.wait", "v": 1, "inputs": {"SECONDS": seconds}})
}

#[test]
fn go_on_into_the_next_case_stands_last_in_a_case_before_another() {
    let a = run_main(vec![
        declare("int", "n", Some(e("2"))),
        switch_cases(
            e("n"),
            vec![
                (e("1"), vec![print(vec![e("n")]), fallthrough()]),
                (e("2"), vec![print(vec![e("n")])]),
            ],
            None,
        ),
    ]);
    assert_clean(&a);
    // Not the last block of its case, in the last part, and outside a switch.
    let a = run_main(vec![
        declare("int", "n", Some(e("2"))),
        switch_cases(
            e("n"),
            vec![
                (e("1"), vec![fallthrough(), print(vec![e("n")])]),
                (e("2"), vec![fallthrough()]),
            ],
            None,
        ),
        fallthrough(),
    ]);
    let misplaced = "'go on into the next case' must be the last block of a case, directly in the case (not \
                     inside another block). Move it to the end of the case.";
    let last = "'go on into the next case' can't be used in the last part of a switch: there is no next case \
                to go on into. Remove it.";
    assert_eq!(
        reported(&a),
        [
            format!("error B2C-E0427 at b0007: {misplaced}"),
            format!("error B2C-E0427 at b0009: {last}"),
            format!("error B2C-E0427 at b0011: {misplaced}"),
        ]
    );
}

#[test]
fn wait_takes_a_number_of_seconds() {
    let a = run_main(vec![wait(e("0.25")), wait(e("2"))]);
    assert_clean(&a);
    let stmts = main_json(&a);
    assert!(stmts[0]["kind"]["wait"].is_object(), "{stmts}");
    let a = run_main(vec![wait(e("\"soon\""))]);
    assert_eq!(codes(&a), ["B2C-E0301"], "{}", render_all(&a));
}

// --- use header, check at compile time (package S) -------------------------------------

/// *use header [header]*, a definition on the canvas.
fn use_header(header: &str) -> Value {
    json!({"id": id(), "type": "program.use_header", "v": 1, "x": 400, "y": 40,
           "fields": {"HEADER": header}})
}

/// *check at compile time (cond) message [message]*.
fn check(cond: Value, message: &str) -> Value {
    json!({"id": id(), "type": "program.check", "v": 1, "inputs": {"COND": cond},
           "fields": {"MESSAGE": message}})
}

/// The headers of the program's *use header* items, in item order.
fn used_headers(a: &b2c_lang::Analysis) -> Vec<String> {
    let program = serde_json::to_value(&a.program).expect("serialise");
    program["modules"][0]["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|item| item["kind"]["use_header"]["header"].as_str().map(str::to_owned))
        .collect()
}

#[test]
fn use_header_is_a_definition_that_names_a_standard_header() {
    let a = run(vec![use_header("<numeric>"), main(vec![]), use_header("<cmath>")]);
    assert_clean(&a);
    assert_eq!(used_headers(&a), ["<numeric>", "<cmath>"]);
    // Without its field it is the catalog's default; a header the table does
    // not hold is the catalog's to report (B2C-E0607), so the analyser adds
    // nothing for it and reports nothing.
    let mut plain = use_header("<cmath>");
    plain["fields"] = json!({});
    let a = run(vec![plain, use_header("<bits/stdc++.h>"), main(vec![])]);
    assert_clean(&a);
    assert_eq!(used_headers(&a), ["<cmath>"]);
}

#[test]
fn check_at_compile_time_holds_for_a_fixed_condition() {
    let a = run_main(vec![
        declare_const("int", "k", e("3")),
        check(e("k * 2 == 6"), "k is three"),
        check(e("true"), "say \"why\""),
    ]);
    assert_clean(&a);
    let stmts = main_json(&a);
    let lowered = &stmts[1]["kind"]["static_check"];
    assert_eq!(lowered["message"], json!("k is three"), "{stmts}");
    assert_eq!(lowered["cond"]["ty"], json!("bool"), "{stmts}");
    assert_eq!(stmts[2]["kind"]["static_check"]["message"], json!("say \"why\""));
}

#[test]
fn check_at_compile_time_needs_a_fixed_condition_that_is_true() {
    let a = run_main(vec![
        declare("int", "n", Some(e("2"))),
        check(e("n > 0"), "n is positive"),
        check(e("1 > 2"), "one is more"),
    ]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0424 at b0002 COND[0..3]: The condition of 'check at compile time' must be fixed when the \
             program is built: use numbers, characters, true/false values and constants with fixed values, not \
             variables or the answers of functions.",
            "error B2C-E0428 at b0003 COND[0..3]: This check fails when the program is built: its condition is \
             false. Fix the values it checks, or remove the check.",
        ]
    );
}

#[test]
fn check_at_compile_time_needs_a_condition_and_a_message_it_can_print() {
    let mut empty = check(e("true"), "fine");
    empty["inputs"] = json!({});
    let a = run_main(vec![empty]);
    assert_eq!(
        reported(&a),
        ["error B2C-E0430 at b0001 input COND: This block needs a condition."]
    );
    let a = run_main(vec![check(e("true"), "nul \u{0} inside")]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0312 at b0002 field MESSAGE: This message can't be used: text cannot contain the NUL \
          character."
        ]
    );
    // A number as the condition is warned about, as in *if*.
    let a = run_main(vec![check(e("1"), "one")]);
    assert_eq!(codes(&a), ["B2C-W0519"], "{}", render_all(&a));
}

// --- open file, read line, write to file, file exists (package S) --------------------

/// *open file (path) for [mode] as [name]*, with its *if it can't be opened*
/// part when `failed` is given.
fn open_file(name: &str, mode: &str, path: Value, failed: Option<Vec<Value>>) -> Value {
    let mut block = json!({"id": id(), "type": "file.open", "v": 1,
                           "extra": {"hasFailed": failed.is_some()},
                           "fields": {"FILE": {"sym": sym(name), "name": name}, "MODE": mode},
                           "inputs": {"PATH": path}});
    if let Some(failed) = failed {
        block["statements"] = json!({"FAILED": failed});
    }
    block
}

/// *read line from [file] into [line]*, a block in an input.
fn read_line(file: &str, line: &str) -> Value {
    input(json!({"id": id(), "type": "file.read_line", "v": 1,
                 "fields": {"FILE": {"ref": sym(file)}, "LINE": {"ref": sym(line)}}}))
}

/// *write (items) … to [file]*, with a new line.
fn write_to(file: &str, items: Vec<Value>) -> Value {
    let inputs: Map<String, Value> = items
        .into_iter()
        .enumerate()
        .map(|(index, item)| (format!("ITEM{index}"), item))
        .collect();
    json!({"id": id(), "type": "file.write", "v": 1, "extra": {"itemCount": inputs.len()},
           "fields": {"FILE": {"ref": sym(file)}}, "inputs": inputs})
}

/// *file (path) exists*, a block in an input.
fn exists(path: Value) -> Value {
    input(json!({"id": id(), "type": "file.exists", "v": 1, "inputs": {"PATH": path}}))
}

/// The type of the program's symbol `name`, as JSON.
fn symbol_type(a: &b2c_lang::Analysis, name: &str) -> Value {
    let program = serde_json::to_value(&a.program).expect("serialise");
    program["symbols"]["symbols"][sym(name)]["ty"].clone()
}

#[test]
fn open_file_declares_a_stream_of_its_mode_for_the_blocks_after_it() {
    let a = run_main(vec![
        declare("std::string", "line", Some(e("\"\""))),
        open_file("log", "append", e("\"log.txt\""), None),
        write_to("log", vec![e("\"started \""), e("1")]),
        open_file(
            "data",
            "read",
            e("\"data.txt\""),
            Some(vec![print(vec![exists(e("\"data.txt\""))])]),
        ),
        while_loop(
            "while",
            read_line("data", "line"),
            vec![write_to("log", vec![e("line")])],
        ),
    ]);
    assert_clean(&a);
    assert_eq!(symbol_type(&a, "log"), json!("std::ofstream"));
    assert_eq!(symbol_type(&a, "data"), json!("std::ifstream"));
    let stmts = main_json(&a);
    assert_eq!(stmts[1]["kind"]["open_file"]["mode"], json!("append"), "{stmts}");
    assert!(stmts[3]["kind"]["open_file"]["failed"].is_object(), "{stmts}");
    assert!(
        stmts[4]["kind"]["while"]["cond"]["kind"]["read_line"].is_object(),
        "{stmts}"
    );
}

#[test]
fn an_opened_file_is_seen_in_its_failed_part_and_after_it_only() {
    // Its *if it can't be opened* part sees it; a block before it doesn't.
    let a = run_main(vec![
        write_to("out", vec![e("1")]),
        open_file(
            "out",
            "write",
            e("\"out.txt\""),
            Some(vec![write_to("out", vec![e("2")])]),
        ),
    ]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0202 at b0001 field FILE: `out` is used before it is created. Move this block below the block \
          that creates `out`."
        ]
    );
}

#[test]
fn the_path_of_open_file_cannot_read_the_name_it_opens() {
    // C++ declares the stream from its name on, also in its own
    // initialiser: `std::ifstream data(data);` reads the new stream, not
    // the input or the global called `data`, and does not compile.
    let opens_data = |path: Value| {
        let mut block = open_file("data", "read", path, None);
        block["fields"]["FILE"] = json!({"sym": sym("stream"), "name": "data"});
        block
    };
    let nested = opens_data(get("data"));
    let path_id = nested["inputs"]["PATH"]["block"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let open_id = nested["id"].as_str().unwrap().to_owned();
    let show = func(
        "show",
        "void",
        &[("data", "std::string", "copy")],
        vec![if_then(e("true"), vec![nested])],
    );
    let a = run(vec![show, main(Vec::new())]);
    assert_eq!(
        reported(&a),
        [
            format!(
                "error B2C-E0205 at {path_id} field VAR: This refers to an outer `data`, but a newer variable also \
                 called `data` hides it here, so C++ would use the newer one. Rename one of them."
            ),
            format!(
                "warning B2C-W0501 at {open_id} field FILE: `data` hides another variable called `data` from an \
                 enclosing block. This works, but it is easy to mix them up, so consider another name."
            ),
        ]
    );
    // A global, which no warning about hiding a variable names.
    let global = json!({"id": "gl_data", "type": "var.global", "v": 1,
                        "fields": {"KIND": "variable", "TYPE": "std::string",
                                   "NAME": {"sym": sym("data"), "name": "data"}}});
    let opened = opens_data(e("data"));
    let a = run(vec![global, main(vec![opened])]);
    assert_eq!(codes(&a), ["B2C-E0205", "B2C-W0501"], "{}", render_all(&a));
    // The path naming the new stream itself.
    let itself = opens_data(get("stream"));
    let path_id = itself["inputs"]["PATH"]["block"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let a = run_main(vec![itself]);
    assert_eq!(
        reported(&a),
        [format!(
            "error B2C-E0202 at {path_id} field VAR: `data` is the file this block opens, so its path can't use \
             it: C++ would read the new file there. Rename the file or the variable."
        )]
    );
}

#[test]
fn open_file_needs_a_name_for_its_file() {
    let mut block = open_file("f", "read", e("\"a.txt\""), None);
    block["fields"] = json!({"MODE": "read"});
    let a = run_main(vec![block]);
    assert_eq!(
        reported(&a),
        ["error B2C-E0430 at b0001 field FILE: This 'open file' block needs a name for its file."]
    );
}

#[test]
fn each_file_block_takes_a_stream_of_its_kind() {
    let a = run_main(vec![
        declare("std::string", "line", Some(e("\"\""))),
        open_file("out", "write", e("\"out.txt\""), None),
        open_file("data", "read", e("\"data.txt\""), None),
        print(vec![read_line("out", "line")]),
        write_to("data", vec![e("line")]),
    ]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0301 at b0004 field FILE: This is an output file, but this block needs an input file here, \
             as its other parts say.",
            "error B2C-E0301 at b0006 field FILE: This is an input file, but this block needs an output file here, \
             as its other parts say.",
        ]
    );
}

#[test]
fn read_line_changes_its_line_and_write_takes_printable_values() {
    let a = run_main(vec![
        declare_const("std::string", "fixed", e("\"x\"")),
        open_file("data", "read", e("\"data.txt\""), None),
        open_file("out", "write", e("\"out.txt\""), None),
        print(vec![read_line("data", "fixed")]),
        write_to("out", vec![e("data")]),
        print(vec![exists(e("3"))]),
    ]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0308 at b0004 field LINE: `fixed` was created as a constant, so it can't be changed. Untick \
             'const' in its 'create' block if it needs to change.",
            "error B2C-E0314 at b0006 ITEM0[0..1]: An input file can't be printed.",
            "error B2C-E0301 at b0007 PATH[0..1]: This is a whole number, but this block needs text or a character.",
        ]
    );
}

#[test]
fn file_blocks_need_their_files_chosen() {
    let mut read = read_line("data", "line");
    read["block"]["fields"] = json!({});
    let mut write = write_to("out", vec![e("1")]);
    write["fields"] = json!({});
    let a = run_main(vec![print(vec![read]), write]);
    assert_eq!(
        reported(&a),
        [
            "error B2C-E0430 at b0001 field FILE: Choose a file opened for reading in this block.",
            "error B2C-E0430 at b0001 field LINE: Choose a text variable for the line in this block.",
            "error B2C-E0430 at b0002 field FILE: Choose a file opened for writing in this block.",
        ]
    );
}
