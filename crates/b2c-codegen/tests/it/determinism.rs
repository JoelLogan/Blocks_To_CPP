//! Determinism: the same program always gives byte-identical files and source
//! maps, whatever the order of its top-level items (spec §6.1, invariant 4).

use b2c_codegen::generate;
use b2c_ir::sast::{PassMode, Program};
use b2c_ir::types::Type;
use proptest::prelude::*;

use crate::builder::Builder;
use crate::{examples, gxx, options};

/// Two overloads of `show` plus other functions, given in an arbitrary order.
fn overloads() -> Program {
    let b = Builder::new();
    let show_int = b.function("show", Type::Void, &[("value", Type::Int, PassMode::Copy)]);
    let show_text = b.function("show", Type::Void, &[("value", Type::String, PassMode::ReadOnly)]);
    let show_both = b.function(
        "show",
        Type::Void,
        &[
            ("value", Type::Int, PassMode::Copy),
            ("label", Type::String, PassMode::ReadOnly),
        ],
    );
    let defs = [&show_int, &show_text].map(|f| {
        let param = f.params[0].clone();
        b.define(f, vec![b.print(vec![b.get(&param)])])
    });
    let both_def = b.define(
        &show_both,
        vec![b.print(vec![b.get(&show_both.params[1]), b.get(&show_both.params[0])])],
    );
    let main = b.main(vec![
        b.eval(b.call(&show_int, vec![b.int("1")])),
        b.eval(b.call(&show_text, vec![b.str("one")])),
        b.eval(b.call(&show_both, vec![b.int("2"), b.str("two: ")])),
    ]);
    let [show_int_def, show_text_def] = defs;
    b.program(vec![show_text_def, main, both_def, show_int_def])
}

#[test]
fn generating_twice_gives_identical_output() {
    for program in [examples::guessing_game(), examples::functions(), overloads()] {
        let first = generate(&program, &options("P"));
        let second = generate(&program, &options("P"));
        assert_eq!(first, second);
    }
}

#[test]
fn overloads_compile_and_are_ordered_by_signature() {
    let project = generate(&overloads(), &options("Overloads"));
    let contents = &project.files[0].contents;
    let declarations: Vec<&str> = contents
        .lines()
        .filter(|l| l.starts_with("void show(") && l.ends_with(';'))
        .collect();
    assert_eq!(
        declarations,
        [
            "void show(int value);",
            "void show(int value, const std::string& label);",
            "void show(const std::string& value);"
        ]
    );
    gxx::syntax_check(&project, "c++20");
}

proptest! {
    #[test]
    fn item_order_does_not_change_the_output(seed in any::<u64>()) {
        for program in [examples::functions(), overloads()] {
            let reference = generate(&program, &options("P"));
            let mut shuffled = program.clone();
            let items = &mut shuffled.modules[0].items;
            // A deterministic shuffle driven by the seed.
            let mut state = seed;
            for i in (1..items.len()).rev() {
                state = state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
                let j = usize::try_from(state >> 33).unwrap() % (i + 1);
                items.swap(i, j);
            }
            prop_assert_eq!(generate(&shuffled, &options("P")), reference);
        }
    }
}
