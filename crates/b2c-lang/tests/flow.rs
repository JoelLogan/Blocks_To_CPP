//! Structure and flow checks (spec §6.6): loops, returns, reachability,
//! definite assignment, endless loops, `main` blocks and damaged blocks.

// Test code: unwrap/expect/panic and printing to stderr are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use b2c_ir::diag::{Location, Part, Severity};
use b2c_lang::analyze;
use common::*;
use serde_json::json;

#[test]
fn break_and_continue_need_a_loop() {
    let a = run_main(vec![brk(), cont()]);
    assert_codes(&a, &["B2C-E0401", "B2C-E0401"]);
    assert!(a.diagnostics[0].message.contains("'leave loop'"));
    assert!(a.diagnostics[1].message.contains("'skip to next round'"));
    // Inside a loop, also nested in an if: fine.
    let a = run_main(vec![repeat(
        num("2"),
        vec![if_then(e("true"), vec![cont()]), brk()],
    )]);
    assert_clean(&a);
    // A function body is not inside the caller's loop.
    let a = run(vec![
        main(vec![forever(vec![call_stmt("f", vec![]), brk()])]),
        func("f", "void", &[], vec![brk()]),
    ]);
    assert_codes(&a, &["B2C-E0401"]);
}

#[test]
fn missing_return() {
    let a = run(vec![main(vec![]), func("f", "int", &[], vec![])]);
    let d = only(&a, "B2C-E0410");
    assert_eq!(
        d.primary.block.as_ref().map(b2c_ir::BlockId::as_str),
        Some("fn_f")
    );
    assert!(
        d.message.contains("must give back a whole number"),
        "{}",
        d.message
    );

    // Only some paths return.
    let a = run(vec![
        main(vec![]),
        func(
            "f",
            "int",
            &[("n", "int", "copy")],
            vec![if_then(e("n > 0"), vec![ret(Some(num("1")))])],
        ),
    ]);
    assert_codes(&a, &["B2C-E0410"]);

    // Every path returns.
    let a = run(vec![
        main(vec![]),
        func(
            "f",
            "int",
            &[("n", "int", "copy")],
            vec![if_else(
                vec![
                    (e("n > 0"), vec![ret(Some(num("1")))]),
                    (e("n < 0"), vec![ret(Some(num("-1")))]),
                ],
                Some(vec![ret(Some(num("0")))]),
            )],
        ),
    ]);
    assert_clean(&a);

    // `stop program` and endless loops also end a path.
    let a = run(vec![
        main(vec![]),
        func("f", "int", &[], vec![exit(num("1"))]),
        func(
            "g",
            "int",
            &[],
            vec![forever(vec![if_then(e("true"), vec![ret(Some(num("1")))])])],
        ),
        func(
            "h",
            "int",
            &[],
            vec![while_loop("while", e("true"), vec![ret(Some(num("1")))])],
        ),
        func(
            "k",
            "int",
            &[],
            vec![while_loop("until", e("false"), vec![ret(Some(num("1")))])],
        ),
    ]);
    assert_clean(&a);

    // A forever loop with a break can fall through.
    let a = run(vec![
        main(vec![]),
        func("f", "int", &[], vec![forever(vec![brk()])]),
    ]);
    assert_codes(&a, &["B2C-E0410"]);
    // A conditional loop can run zero times.
    let a = run(vec![
        main(vec![]),
        func("f", "int", &[], vec![repeat(num("3"), vec![ret(Some(num("1")))])]),
    ]);
    assert_codes(&a, &["B2C-E0410"]);
    // Void functions and main need no return.
    let a = run(vec![
        main(vec![print(vec![text("x")])]),
        func("v", "void", &[], vec![]),
    ]);
    assert_clean(&a);
}

#[test]
fn unreachable_blocks() {
    let a = run_main(vec![
        exit(num("0")),
        print(vec![text("never")]),
        print(vec![text("never")]),
    ]);
    let d = only(&a, "B2C-W0502");
    assert_eq!(d.severity, Severity::Warning);
    assert!(d.message.contains("can never run"), "{}", d.message);

    let a = run_main(vec![forever(vec![brk(), print(vec![text("x")])])]);
    assert_codes(&a, &["B2C-W0502"]);
    let a = run_main(vec![repeat(num("2"), vec![cont(), print(vec![text("x")])])]);
    assert_codes(&a, &["B2C-W0502"]);
    let a = run(vec![
        main(vec![]),
        func("f", "void", &[], vec![ret(None), print(vec![text("x")])]),
    ]);
    assert_codes(&a, &["B2C-W0502"]);
    // After an if whose branches all leave.
    let a = run(vec![
        main(vec![]),
        func(
            "f",
            "int",
            &[],
            vec![
                if_else(
                    vec![(e("true"), vec![ret(Some(num("1")))])],
                    Some(vec![ret(Some(num("2")))]),
                ),
                print(vec![text("x")]),
            ],
        ),
    ]);
    assert_codes(&a, &["B2C-W0502"]);
    // After an endless loop.
    let a = run_main(vec![forever(vec![exit(num("0"))]), print(vec![text("x")])]);
    assert_codes(&a, &["B2C-W0502"]);
    // Not after an if without else.
    let a = run_main(vec![repeat(
        num("1"),
        vec![if_then(e("true"), vec![brk()]), print(vec![text("x")])],
    )]);
    assert_clean(&a);
}

#[test]
fn use_before_assignment() {
    let a = run_main(vec![declare("int", "x", None), print(vec![get("x")])]);
    let d = only(&a, "B2C-W0503");
    assert!(d.message.contains("before it is given a value"), "{}", d.message);

    // Assigned first: fine. Text variables start empty, which is fine too.
    assert_clean(&run_main(vec![
        declare("int", "x", None),
        set("x", num("1")),
        print(vec![get("x")]),
    ]));
    assert_clean(&run_main(vec![
        declare("std::string", "s", None),
        print(vec![get("s")]),
    ]));
    assert_clean(&run_main(vec![
        declare("int", "x", None),
        ask("x", None),
        print(vec![get("x")]),
    ]));

    // `change by` reads the variable.
    assert_codes(
        &run_main(vec![declare("int", "x", None), change("x", num("1"))]),
        &["B2C-W0503"],
    );

    // Reported once per variable.
    let a = run_main(vec![
        declare("double", "x", None),
        print(vec![get("x")]),
        print(vec![e("x + x")]),
    ]);
    assert_codes(&a, &["B2C-W0503"]);

    // Assigned in some branch: no warning (it may have a value).
    let a = run_main(vec![
        declare("int", "x", None),
        if_then(e("true"), vec![set("x", num("1"))]),
        print(vec![get("x")]),
    ]);
    assert_clean(&a);

    // Assigned later in a loop: the value may come from the previous round.
    let a = run_main(vec![
        declare("int", "last", None),
        repeat(num("3"), vec![print(vec![get("last")]), set("last", num("1"))]),
    ]);
    assert_clean(&a);

    // Passed to an editable parameter: the function fills it in.
    let a = run(vec![
        main(vec![
            declare("int", "x", None),
            call_stmt("fill", vec![get("x")]),
            print(vec![get("x")]),
        ]),
        func(
            "fill",
            "void",
            &[("out", "int", "editable")],
            vec![set("out", num("1"))],
        ),
    ]);
    assert_clean(&a);
    // But passing it by copy reads it.
    let a = run(vec![
        main(vec![declare("int", "x", None), call_stmt("show", vec![get("x")])]),
        func(
            "show",
            "void",
            &[("v", "int", "copy")],
            vec![print(vec![get("v")])],
        ),
    ]);
    assert_codes(&a, &["B2C-W0503"]);
    // Calls inside expressions too.
    let a = run(vec![
        main(vec![
            declare("int", "x", None),
            declare("int", "y", Some(e("fill(x) + x"))),
        ]),
        func(
            "fill",
            "int",
            &[("out", "int", "editable")],
            vec![set("out", num("1")), ret(Some(num("0")))],
        ),
    ]);
    assert_clean(&a);
    // A loop that assigns through an editable argument.
    let a = run(vec![
        main(vec![
            declare("int", "x", None),
            repeat(
                num("2"),
                vec![print(vec![get("x")]), call_stmt("fill", vec![get("x")])],
            ),
        ]),
        func(
            "fill",
            "void",
            &[("out", "int", "editable")],
            vec![set("out", num("1"))],
        ),
    ]);
    assert_clean(&a);
}

#[test]
fn endless_forever_loop_is_info() {
    let a = run_main(vec![forever(vec![print(vec![text("tick")])])]);
    let d = only(&a, "B2C-I0513");
    assert_eq!(d.severity, Severity::Info);
    // A break in a nested loop does not leave the outer loop.
    let a = run_main(vec![forever(vec![repeat(num("2"), vec![brk()])])]);
    assert_codes(&a, &["B2C-I0513"]);
    // break, return or exit inside: fine.
    assert_clean(&run_main(vec![forever(vec![if_then(e("true"), vec![brk()])])]));
    assert_clean(&run_main(vec![forever(vec![repeat(
        num("1"),
        vec![exit(num("0"))],
    )])]));
    let a = run(vec![
        main(vec![]),
        func("f", "void", &[], vec![forever(vec![ret(None)])]),
    ]);
    assert_clean(&a);
}

#[test]
fn bad_steps() {
    let a = run_main(vec![for_range(
        "i",
        num("10"),
        "down_to",
        num("0"),
        Some(num("-1")),
        vec![],
    )]);
    let d = only(&a, "B2C-W0522");
    assert!(d.message.contains("more than 0"), "{}", d.message);
    assert_codes(
        &run_main(vec![for_range(
            "i",
            num("0"),
            "to",
            num("10"),
            Some(e("0")),
            vec![],
        )]),
        &["B2C-W0522"],
    );
    assert_clean(&run_main(vec![for_range(
        "i",
        num("0"),
        "to",
        num("10"),
        Some(e("+2")),
        vec![],
    )]));
}

#[test]
fn main_blocks() {
    let a = analyze(&doc(vec![func("f", "void", &[], vec![])]));
    let d = only(&a, "B2C-E0405");
    assert_eq!(d.primary, Location::project());
    assert!(d.message.contains("Add a 'when program starts' block"));

    let mut disabled = main(vec![]);
    disabled["disabled"] = json!(true);
    let a = analyze(&doc(vec![disabled]));
    let d = only(&a, "B2C-E0405");
    assert!(d.message.contains("disabled"), "{}", d.message);
    assert_eq!(
        d.primary.block.as_ref().map(b2c_ir::BlockId::as_str),
        Some("main")
    );

    let mut second = main(vec![brk()]);
    second["id"] = json!("main2");
    let a = analyze(&doc_modules(vec![
        ("main", vec![main(vec![])]),
        ("other", vec![second]),
    ]));
    // The extra main is reported, and its body is still checked.
    assert_codes(&a, &["B2C-E0406", "B2C-E0401"]);
    let d = &a.diagnostics[0];
    assert_eq!(
        d.primary.block.as_ref().map(b2c_ir::BlockId::as_str),
        Some("main2")
    );
    assert_eq!(
        d.related[0].location.block.as_ref().map(b2c_ir::BlockId::as_str),
        Some("main")
    );
    // Only the first becomes the program's main.
    assert_eq!(a.program.modules[0].items.len(), 1);
    assert!(a.program.modules[1].items.is_empty());

    // No modules at all.
    let mut empty = doc(vec![]);
    empty.modules.clear();
    let a = analyze(&empty);
    assert_codes(&a, &["B2C-E0405"]);
    assert!(a.program.modules.is_empty());
}

#[test]
fn missing_pieces_are_reported() {
    let a = run_main(vec![
        json!({"id": "s1", "type": "var.set", "v": 1, "fields": {"VAR": {"ref": "sym_x"}}}),
        json!({"id": "d1", "type": "var.declare", "v": 1, "fields": {"TYPE": "int"}}),
        json!({"id": "p1", "type": "io.print", "v": 1, "extra": {"itemCount": 2},
               "inputs": {"ITEM0": {"expr": []}}}),
        json!({"id": "i1", "type": "control.if", "v": 1}),
        json!({"id": "c1", "type": "func.call_stmt", "v": 1}),
        json!({"id": "f1", "type": "control.for_range", "v": 1,
               "inputs": {"FROM": {"expr": [{"num": "0"}]}, "TO": {"expr": [{"num": "1"}]}}}),
    ]);
    let rendered = render_all(&a);
    assert_codes(
        &a,
        &[
            "B2C-E0201",
            "B2C-E0430",
            "B2C-E0430",
            "B2C-E0430",
            "B2C-E0430",
            "B2C-E0430",
            "B2C-E0430",
            "B2C-E0430",
        ],
    );
    assert!(
        rendered.contains("s1 input VALUE: This block needs a value."),
        "{rendered}"
    );
    assert!(rendered.contains("d1 field NAME"), "{rendered}");
    assert!(
        rendered.contains("p1 input ITEM0: This block needs item 1."),
        "{rendered}"
    );
    assert!(
        rendered.contains("p1 input ITEM1: This block needs item 2."),
        "{rendered}"
    );
    assert!(
        rendered.contains("i1 input COND0: This block needs a condition."),
        "{rendered}"
    );
    assert!(rendered.contains("c1 field FUNC"), "{rendered}");
    assert!(rendered.contains("f1 field VAR"), "{rendered}");
}

#[test]
fn damaged_settings_are_reported() {
    let a = run_main(vec![
        json!({"id": "d1", "type": "var.declare", "v": 1,
               "fields": {"TYPE": "long double", "NAME": {"sym": "sym_a", "name": "a"}}}),
        json!({"id": "d2", "type": "var.declare", "v": 1,
               "fields": {"TYPE": "void", "NAME": {"sym": "sym_b", "name": "b"}}}),
        json!({"id": "d3", "type": "var.declare", "v": 1,
               "fields": {"TYPE": "int", "NAME": {"sym": "sym_c", "name": "c"}, "CONST": "yes"}}),
        json!({"id": "p1", "type": "io.print", "v": 1, "fields": {"SEP": "tabs"},
               "inputs": {"ITEM0": {"expr": [{"num": "1"}]}}}),
        json!({"id": "p2", "type": "io.print", "v": 1, "fields": {"SEP": true},
               "inputs": {"ITEM0": {"expr": [{"num": "1"}]}}}),
    ]);
    assert_codes(&a, &["B2C-E0430"; 5]);
    let a = run(vec![
        main(vec![]),
        json!({"id": "f", "type": "func.define", "v": 1,
        "fields": {"NAME": {"sym": "sym_f", "name": "f"}, "RETURNS": "auto"},
        "extra": {"params": [
            {"sym": "p1", "name": "a", "type": "void", "mode": "copy"},
            {"sym": "p2", "name": "b", "type": "int", "mode": "moved"},
            {"name": "c"}
        ]}}),
        json!({"id": "g", "type": "func.define", "v": 1}),
    ]);
    assert_codes(&a, &["B2C-E0430"; 5]);
    let parts: Vec<&Part> = a.diagnostics.iter().map(|d| &d.primary.part).collect();
    assert_eq!(
        parts[1],
        &Part::Field {
            name: String::from("params[0]")
        }
    );
    assert_eq!(
        parts[3],
        &Part::Field {
            name: String::from("params[2]")
        }
    );
}

#[test]
fn disabled_reporter_in_an_input() {
    let a = run_main(vec![print(vec![
        json!({"block": {"id": "n", "type": "math.number", "v": 1,
        "disabled": true, "fields": {"VALUE": "1"}}}),
    ])]);
    let d = only(&a, "B2C-E0430");
    assert!(d.message.contains("disabled"), "{}", d.message);
    // In an optional input, it just means "no value".
    let a = run_main(vec![
        declare(
            "int",
            "x",
            Some(json!({"block": {"id": "n", "type": "math.number", "v": 1,
        "disabled": true, "fields": {"VALUE": "1"}}})),
        ),
        set("x", num("1")),
    ]);
    assert_clean(&a);
}

#[test]
fn deeply_nested_blocks_are_cut_off() {
    // Statement nesting.
    let mut body = vec![print(vec![text("deep")])];
    for _ in 0..100 {
        body = vec![if_then(e("true"), body)];
    }
    let a = run_main(body);
    assert_codes(&a, &["B2C-E0431"]);
    // Value nesting.
    let mut value = num("1");
    for _ in 0..100 {
        value = arith("add", value, num("1"));
    }
    let a = run_main(vec![print(vec![value])]);
    assert_codes(&a, &["B2C-E0431"]);
    // The limit is 64 blocks: `print` plus 62 operators plus the number.
    let nested = |levels: usize| {
        let mut value = num("1");
        for _ in 0..levels {
            value = arith("add", value, num("1"));
        }
        run_main(vec![print(vec![value])])
    };
    assert_clean(&nested(62));
    assert_codes(&nested(63), &["B2C-E0431"]);
    // A slot at the deepest level can still nest 64 levels itself, and the
    // whole tree is analysed without exhausting the stack.
    let deep_slot = format!("{}1{}", "(-".repeat(32), ")".repeat(32));
    let mut value = e(&deep_slot);
    for _ in 0..61 {
        value = arith("add", value, num("1"));
    }
    assert_clean(&run_main(vec![print(vec![value])]));
}

#[test]
fn blocks_that_expand_too_deeply_are_reported() {
    // An `and` block with 32 conditions becomes 31 levels of `&&`, so a few
    // of them nested in each other are too deep for the later stages, even
    // though the blocks themselves nest only 8 levels.
    let nested = |levels: usize| {
        let mut value = boolean(true);
        for _ in 0..levels {
            let mut items = vec![value];
            items.extend((1..32).map(|_| boolean(true)));
            value = logic("and", items);
        }
        run_main(vec![if_then(value, vec![print(vec![text("deep")])])])
    };
    let a = nested(8);
    let d = only(&a, "B2C-E0431");
    assert!(d.message.contains("nested too deeply"), "{}", d.message);
    assert_eq!(a.diagnostics.len(), 1, "{}", render_all(&a));
    // The program is still produced, and the generator does not need to cut
    // anything off when there is no error.
    assert_eq!(a.program.modules[0].items.len(), 1);
    let a = nested(6);
    assert_clean(&a);
    let project = b2c_codegen::generate(&a.program, &b2c_codegen::CodegenOptions::default());
    assert!(!project.files[0].contents.contains("/* error */"));
}
