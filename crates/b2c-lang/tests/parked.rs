//! Parked blocks (ADR-0012, spec 05 §5.4, 06 §6.4): a statement, reporter or
//! predicate left on the canvas outside `when program starts` and every
//! function is not lowered and does not run. The analyser reports each
//! enabled parked head once, as the warning `B2C-W0505`.

// Test code: unwrap/expect/panic and printing to stderr (the shared helpers) are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use b2c_catalog::{FieldKind, Shape};
use b2c_codegen::{CodegenOptions, generate};
use b2c_ir::diag::{DiagSource, Diagnostic, Part, Severity};
use b2c_ir::ids::BlockId;
use b2c_lang::Analysis;
use common::*;
use serde_json::{Value, json};

const PARKED: &str = "B2C-W0505";

const STATEMENT_MESSAGE: &str =
    "This block isn't attached to 'when program starts' or a function, so it doesn't run.";

const VALUE_MESSAGE: &str =
    "This block gives a value, but it isn't plugged into another block, so it isn't used.";

/// The `W0505` diagnostics of an analysis.
fn parked(a: &Analysis) -> Vec<&Diagnostic> {
    a.diagnostics.iter().filter(|d| d.code.0 == PARKED).collect()
}

/// A top-level block with `x`, `y` and the given ID, as the canvas saves it.
fn on_canvas(mut block: Value, id: &str) -> Value {
    block["id"] = json!(id);
    block["x"] = json!(400);
    block["y"] = json!(40);
    block
}

/// The block inside an input (`{"block": …}`), for a value block that the
/// helpers return ready to plug in.
fn bare(mut input: Value) -> Value {
    input["block"].take()
}

#[test]
fn a_parked_statement_is_reported_once_as_a_warning() {
    let a = run(vec![
        main(vec![print(vec![text("attached")])]),
        on_canvas(print(vec![text("parked")]), "p1"),
    ]);
    assert_codes(&a, &[PARKED]);
    let d = &a.diagnostics[0];
    assert_eq!(d.severity, Severity::Warning);
    assert_eq!(d.source, DiagSource::Analyser);
    assert_eq!(d.primary.block.as_ref().map(BlockId::as_str), Some("p1"));
    assert_eq!(d.primary.part, Part::Whole);
    assert_eq!(d.message, STATEMENT_MESSAGE);
}

#[test]
fn a_parked_stack_is_reported_once_on_its_head_counting_the_stacked_blocks() {
    let mut head = on_canvas(print(vec![text("one")]), "p1");
    head["stack"] = json!([
        print(vec![text("two")]),
        // A disabled stacked block is still below the head.
        {"id": "p3", "type": "io.print", "v": 1, "disabled": true},
        repeat(num("3"), vec![print(vec![text("three")])]),
    ]);
    let a = run(vec![main(vec![]), head.clone()]);
    let found = parked(&a);
    assert_eq!(found.len(), 1, "{}", render_all(&a));
    assert_eq!(found[0].primary.block.as_ref().map(BlockId::as_str), Some("p1"));
    assert_eq!(
        found[0].message,
        "This block and the 3 blocks below it aren't attached to 'when program starts' or a function, so they don't run."
    );

    head["stack"] = json!([print(vec![text("two")])]);
    let a = run(vec![main(vec![]), head]);
    assert_eq!(
        parked(&a)[0].message,
        "This block and the block below it aren't attached to 'when program starts' or a function, so they don't run."
    );
}

#[test]
fn every_enabled_parked_head_is_reported() {
    let a = run(vec![
        main(vec![]),
        on_canvas(print(vec![text("a")]), "p1"),
        on_canvas(set("ghost", num("1")), "p2"),
        on_canvas(bare(arith("add", num("1"), num("2"))), "p3"),
    ]);
    let heads: Vec<&str> = parked(&a)
        .iter()
        .map(|d| d.primary.block.as_ref().unwrap().as_str())
        .collect();
    assert_eq!(heads, ["p1", "p2", "p3"], "{}", render_all(&a));
    // Parked code is not analysed: `ghost` is declared nowhere, and no error
    // says so until the block is attached.
    assert!(
        a.diagnostics.iter().all(|d| d.code.0 == PARKED),
        "{}",
        render_all(&a)
    );
}

#[test]
fn a_parked_value_block_has_its_own_wording() {
    for (block, what) in [
        (bare(num("1")), "a reporter"),
        (bare(boolean(true)), "a predicate"),
        (bare(compare("lt", num("1"), num("2"))), "a comparison"),
        (bare(get("x")), "a getter"),
        (bare(call("f", vec![])), "a call that gives a value"),
    ] {
        let a = run(vec![main(vec![]), on_canvas(block, "p1")]);
        assert_codes(&a, &[PARKED]);
        assert_eq!(a.diagnostics[0].message, VALUE_MESSAGE, "{what}");
    }
}

#[test]
fn a_disabled_head_is_silent() {
    let mut head = on_canvas(print(vec![text("off")]), "p1");
    head["disabled"] = json!(true);
    let a = run(vec![main(vec![]), head.clone()]);
    assert_clean(&a);
    // Its stack is parked with it, even when the stacked blocks are enabled.
    head["stack"] = json!([print(vec![text("two")])]);
    let a = run(vec![main(vec![]), head]);
    assert_clean(&a);
    let mut value = on_canvas(bare(num("1")), "p2");
    value["disabled"] = json!(true);
    assert_clean(&run(vec![main(vec![]), value]));
}

#[test]
fn definitions_and_unknown_types_are_never_parked() {
    let a = run(vec![
        main(vec![print(vec![text("hi")])]),
        func("helper", "void", &[], vec![]),
    ]);
    assert_clean(&a);
    // A second `when program starts` stays an error, never parked.
    let a = run(vec![main(vec![]), on_canvas(main(vec![]), "zz_second")]);
    assert_codes(&a, &["B2C-E0406"]);
    // A block type the analyser does not know is the catalog's to report
    // (B2C-E0601).
    let a = run(vec![
        main(vec![]),
        json!({"id": "p1", "type": "raw.cpp", "v": 1, "x": 0, "y": 0}),
    ]);
    assert_clean(&a);
}

/// The analyser's idea of a parked head matches the catalog's shapes: every
/// statement, reporter and predicate of the catalog is reported when it is
/// left on the canvas, with the value wording for reporters and predicates,
/// and no hat or definition is.
#[test]
fn every_catalog_block_is_parked_by_its_shape() {
    for def in b2c_catalog::core_catalog().blocks.values() {
        let block = json!({"id": "p1", "type": def.id, "v": def.version, "x": 0, "y": 0});
        let a = run(vec![main(vec![]), block]);
        let found = parked(&a);
        let expected = match def.shape {
            Shape::Hat | Shape::Definition => None,
            Shape::Statement => Some(STATEMENT_MESSAGE),
            Shape::Reporter | Shape::Predicate => Some(VALUE_MESSAGE),
        };
        match expected {
            None => assert!(found.is_empty(), "{}: {}", def.id, render_all(&a)),
            Some(message) => {
                assert_eq!(found.len(), 1, "{}: {}", def.id, render_all(&a));
                assert_eq!(found[0].message, message, "{}", def.id);
            }
        }
    }
}

#[test]
fn parked_blocks_never_change_the_program() {
    let attached = main(vec![declare("int", "n", Some(num("3"))), print(vec![get("n")])]);
    let options = CodegenOptions::default();
    let without = analyse(&doc(vec![attached.clone()]));
    // A parked stack that declares and changes a variable of its own, and
    // changes and reads main's `n`.
    let mut stack_head = on_canvas(declare("int", "m", Some(num("7"))), "p2");
    stack_head["stack"] = json!([set("n", get("m")), print(vec![get("n")])]);
    let with = analyse(&doc(vec![
        attached,
        on_canvas(print(vec![text("never")]), "p1"),
        stack_head,
        on_canvas(bare(arith("add", get("n"), num("1"))), "p3"),
    ]));
    assert_eq!(parked(&with).len(), 3, "{}", render_all(&with));
    assert_eq!(with.program, without.program);
    assert_eq!(
        generate(&with.program, &options),
        generate(&without.program, &options)
    );
}

/// The catalog's and the analyser's diagnostics, as the build and the
/// editor's preview list them (`code part`).
fn checked(blocks: Vec<Value>) -> Vec<String> {
    let report = b2c_catalog::resolve_report(&doc(blocks), b2c_catalog::core_catalog());
    let analysis = analyze_checked(&report.document, &report.diagnostics, &report.unchecked);
    report
        .diagnostics
        .iter()
        .chain(&analysis.diagnostics)
        .map(|d| match &d.primary.part {
            Part::Field { name } => format!("{} {name}", d.code.0),
            _ => d.code.0.clone(),
        })
        .collect()
}

/// A block straight from the toolbox, in which nothing is chosen yet: the
/// editor saves an unchosen name or reference as an absent field. Parked,
/// it is only the warning, also below a `create` in a loose stack (the
/// Scratch habit of building on the side, ADR-0012); disabled, it is
/// nothing; attached, it is one `B2C-E0430` at each such field. Every block of the
/// catalog with a name or reference field, so a new one cannot bring the
/// catalog's error back.
#[test]
fn a_parked_block_with_nothing_chosen_yet_is_only_a_warning() {
    let mut seen = 0;
    for def in b2c_catalog::core_catalog().blocks.values() {
        // One `B2C-E0430` at each name or reference field, in catalog
        // order (*for each key value* has three); a repeated field (w2.md
        // §2.1) is reported at its first part.
        let missing: Vec<String> = def
            .field
            .iter()
            .filter(|f| matches!(f.kind, FieldKind::SymbolDecl | FieldKind::SymbolRef))
            .map(|field| {
                if field.repeat.is_some() {
                    format!("B2C-E0430 {}0", field.name)
                } else {
                    format!("B2C-E0430 {}", field.name)
                }
            })
            .collect();
        if missing.is_empty() {
            continue;
        }
        seen += 1;
        let mut block = json!({"id": "p1", "type": def.id, "v": def.version, "x": 400, "y": 40});
        // A value for every input without a default (the `set` of the
        // Variables category comes with one). *for each* goes through text.
        for input in &def.input {
            if input.default.is_empty() && !input.optional && input.repeat.is_none() {
                block["inputs"][&input.name] = if def.id == "coll.for_each" {
                    text("abc")
                } else {
                    num("0")
                };
            }
        }
        if def.shape == Shape::Definition {
            block["extra"] = json!({"params": []});
            assert_eq!(checked(vec![main(vec![]), block.clone()]), missing, "{}", def.id);
            block["disabled"] = json!(true);
            let found = checked(vec![main(vec![]), block]);
            assert!(found.is_empty(), "{}: {found:?}", def.id);
            continue;
        }
        assert_eq!(checked(vec![main(vec![]), block.clone()]), [PARKED], "{}", def.id);
        let mut disabled = block.clone();
        disabled["disabled"] = json!(true);
        let found = checked(vec![main(vec![]), disabled]);
        assert!(found.is_empty(), "{}: {found:?}", def.id);
        let attached = match def.shape {
            Shape::Statement => main(vec![block]),
            _ => main(vec![print(vec![json!({"block": block})])]),
        };
        assert_eq!(checked(vec![attached]), missing, "{}", def.id);
    }
    assert!(seen >= 9, "{seen}");

    // Built on the side: a `create` with an `ask` below it, whose variable
    // can be chosen once the stack is attached (M3, ADR-0012).
    let mut head = on_canvas(declare("int", "guess", Some(num("1"))), "p2");
    head["stack"] =
        json!([{"id": "p3", "type": "io.ask", "v": 1, "inputs": {"PROMPT": text("Your answer: ")}}]);
    assert_eq!(checked(vec![main(vec![]), head]), [PARKED]);
}
