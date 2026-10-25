//! W2's text, input and output, math and error blocks in the analyser
//! (docs/plans/m3-contracts/w2.md §1.3, §1.4, §1.7, §2.5; package B): the
//! types they give, the fit of their parts with each message, and the rules
//! of *try … catch*, *throw* and *throw the error again*. The blocks come
//! as the project file holds them, analysed with the catalog's signatures.

// Test code: unwrap/expect/panic, printing to stderr and builders that take
// JSON by value are fine here.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    clippy::needless_pass_by_value
)]

mod common;

use std::collections::BTreeMap;

use b2c_ir::diag::Part;
use b2c_ir::ids::BlockId;
use b2c_ir::types::{LibType, Type};
use b2c_lang::Analysis;
use common::*;
use serde_json::{Map, Value, json};

/// Analyses a program with these top-level blocks with the catalog's
/// signatures, which type W2's blocks.
fn check(blocks: Vec<Value>) -> Analysis {
    analyze(&doc(blocks))
}

/// Analyses a program whose `main` holds these statements.
fn check_main(body: Vec<Value>) -> Analysis {
    check(vec![main(body)])
}

/// The rendered diagnostics, one per line.
fn reported(a: &Analysis) -> Vec<String> {
    a.diagnostics.iter().map(render).collect()
}

/// A reporter block of `kind` with these fields and inputs, as an input.
fn value(kind: &str, fields: Value, inputs: Value) -> (String, Value) {
    let block_id = id();
    let block = json!({"id": block_id, "type": kind, "v": 1, "fields": fields, "inputs": inputs});
    (block_id, input(block))
}

/// A statement block of `kind`.
fn statement(kind: &str, fields: Value, inputs: Value) -> Value {
    json!({"id": id(), "type": kind, "v": 1, "fields": fields, "inputs": inputs})
}

/// The static type of each block, by ID.
fn types(a: &Analysis) -> BTreeMap<BlockId, Type> {
    a.block_types()
}

/// The static type of block `id`.
fn type_of(a: &Analysis, block: &str) -> Type {
    types(a)
        .get(&BlockId::new(block).unwrap())
        .cloned()
        .unwrap_or_else(|| panic!("no type for {block}"))
}

fn lib(ty: LibType, args: Vec<Type>) -> Type {
    Type::lib(ty, args).unwrap()
}

/// *try … catch* with one catch part per `(error, name, statements)` and
/// `body` in the try part. Each name is the symbol `sym_<name>_<part>`, so
/// parts may share a name.
fn try_catch(body: Vec<Value>, parts: Vec<(&str, &str, Vec<Value>)>) -> Value {
    let mut fields = Map::new();
    let mut statements = Map::new();
    statements.insert(String::from("TRY"), Value::Array(body));
    let count = parts.len();
    for (index, (error, name, part)) in parts.into_iter().enumerate() {
        fields.insert(format!("ERROR{index}"), json!(error));
        fields.insert(
            format!("NAME{index}"),
            json!({"sym": catch_sym(name, index), "name": name}),
        );
        statements.insert(format!("CATCH{index}"), Value::Array(part));
    }
    json!({"id": id(), "type": "error.try", "v": 1, "extra": {"catchCount": count},
           "fields": fields, "statements": statements})
}

/// The symbol of a catch part's variable.
fn catch_sym(name: &str, index: usize) -> String {
    format!("{}_{index}", sym(name))
}

/// *message of* the caught error of a part.
fn message_of(name: &str, index: usize) -> Value {
    input(json!({"id": id(), "type": "error.message", "v": 1,
                 "fields": {"ERROR": {"ref": catch_sym(name, index)}}}))
}

/// *throw* of this error with this message.
fn throw(error: &str, message: Value) -> Value {
    statement(
        "error.throw",
        json!({"ERROR": error}),
        json!({"MESSAGE": message}),
    )
}

fn rethrow() -> Value {
    json!({"id": id(), "type": "error.rethrow", "v": 1})
}

// --- Text ------------------------------------------------------------------------

#[test]
fn text_blocks_give_their_types() {
    let (length, length_input) = value("text.length", json!({}), json!({"TEXT": text("hello")}));
    let (letter, letter_input) = value(
        "text.letter",
        json!({}),
        json!({"INDEX": num("1"), "TEXT": text("hello")}),
    );
    let (lower, lower_input) = value("text.case", json!({"CASE": "lower"}), json!({"TEXT": text("Hi")}));
    let (int, int_input) = value(
        "text.to_number",
        json!({"TO": "int"}),
        json!({"TEXT": text("42")}),
    );
    let (real, real_input) = value(
        "text.to_number",
        json!({"TO": "double"}),
        json!({"TEXT": text("4.2")}),
    );
    let (split, split_input) = value(
        "text.split",
        json!({}),
        json!({"TEXT": text("a b"), "SEPARATOR": text(" ")}),
    );
    let mut words = declare("std::string", "words", None);
    words["fields"]["TYPE"] = json!({"std::vector": "std::string"});
    words["inputs"] = json!({"VALUE": split_input});
    let a = check_main(vec![
        print(vec![
            length_input,
            letter_input,
            lower_input,
            int_input,
            real_input,
        ]),
        words,
    ]);
    assert_clean(&a);
    assert_eq!(type_of(&a, &length), Type::Int);
    assert_eq!(type_of(&a, &letter), Type::Char);
    assert_eq!(type_of(&a, &lower), Type::String);
    assert_eq!(type_of(&a, &int), Type::Int);
    assert_eq!(type_of(&a, &real), Type::Double);
    assert_eq!(type_of(&a, &split), lib(LibType::Vector, vec![Type::String]));
}

#[test]
fn the_length_of_a_character_is_refused() {
    let (_, length) = value("text.length", json!({}), json!({"TEXT": chr("a")}));
    let a = check_main(vec![print(vec![length])]);
    assert_eq!(
        reported(&a),
        [format!(
            "error B2C-E0301 at {}: 'length of' needs text, but this is a character. Use a text value here.",
            a.diagnostics[0].primary.block.as_ref().unwrap()
        )]
    );
}

/// The codes and messages reported, in order.
fn messages(a: &Analysis) -> Vec<String> {
    a.diagnostics
        .iter()
        .map(|d| format!("{} {}", d.code.0, d.message))
        .collect()
}

#[test]
fn text_parts_report_their_fit_by_name() {
    let (_, letter) = value(
        "text.letter",
        json!({}),
        json!({"INDEX": num("1.5"), "TEXT": text("hello")}),
    );
    let (_, split) = value(
        "text.split",
        json!({}),
        json!({"TEXT": chr("a"), "SEPARATOR": num("1")}),
    );
    let (_, number) = value("text.to_number", json!({"TO": "int"}), json!({"TEXT": chr("7")}));
    let mut words = declare("std::string", "words", None);
    words["fields"]["TYPE"] = json!({"std::vector": "std::string"});
    words["inputs"] = json!({"VALUE": split});
    let a = check_main(vec![
        print(vec![letter]),
        words,
        print(vec![number]),
        statement("io.decimals", json!({}), json!({"DIGITS": text("2")})),
    ]);
    assert_eq!(
        messages(&a),
        [
            "B2C-W0518 The position in 'letter of' should be a whole number, but this is a decimal number. \
             Use the 'convert' block to make it a whole number.",
            "B2C-E0301 The text to split needs text, but this is a character. Use the 'join' block to turn \
             it into text.",
            "B2C-E0301 This is a whole number, but this block needs text or a character.",
            "B2C-E0301 'as number' needs text, but this is a character. Use the 'join' block to turn it into \
             text.",
            "B2C-E0301 This is text, but this block needs a whole number.",
        ]
    );
}

#[test]
fn as_text_takes_what_can_be_printed() {
    let (as_text, value_input) = value("text.as_text", json!({}), json!({"VALUE": num("2.5")}));
    let a = check_main(vec![print(vec![value_input])]);
    assert_clean(&a);
    assert_eq!(type_of(&a, &as_text), Type::String);

    let mut list = declare("int", "xs", None);
    list["fields"]["TYPE"] = json!({"std::vector": "int"});
    let (_, of_list) = value("text.as_text", json!({}), json!({"VALUE": get("xs")}));
    let a = check_main(vec![list, print(vec![of_list])]);
    assert_eq!(
        messages(&a),
        ["B2C-E0314 A list can't be joined into text in one go. Use 'for each' to go through its items."]
    );
}

#[test]
fn text_tests_and_parts_give_their_types() {
    let (find, find_input) = value(
        "text.find",
        json!({"OP": "starts"}),
        json!({"TEXT": text("hello"), "PART": text("he")}),
    );
    let (part, part_input) = value(
        "text.part",
        json!({}),
        json!({"TEXT": text("hello"), "START": num("1"), "LENGTH": num("3")}),
    );
    let (trim, trim_input) = value("text.trim", json!({}), json!({"TEXT": text(" a ")}));
    let (replace, replace_input) = value(
        "text.replace",
        json!({}),
        json!({"OLD": text("a"), "NEW": text("o"), "TEXT": text("banana")}),
    );
    let a = check_main(vec![print(vec![
        find_input,
        part_input,
        trim_input,
        replace_input,
    ])]);
    assert_clean(&a);
    assert_eq!(type_of(&a, &find), Type::Bool);
    for block in [&part, &trim, &replace] {
        assert_eq!(type_of(&a, block), Type::String);
    }
    // The text first, as `b2c::replace_all(text, old, new)` takes it.
    let body = main_json(&a);
    let args = &body[0]["kind"]["print"]["items"][3]["kind"]["text_call"]["args"];
    let literal = |i: usize| args[i]["kind"]["str"].as_str().unwrap().to_owned();
    assert_eq!([literal(0), literal(1), literal(2)], ["banana", "a", "o"]);
}

// --- Input and output, math ---------------------------------------------------------

#[test]
fn math_blocks_give_the_type_of_their_inputs() {
    let (sqrt_of_int, sqrt_input) = value("math.fn", json!({"FN": "sqrt"}), json!({"VALUE": num("9")}));
    let (abs_char, abs_input) = value("math.abs", json!({}), json!({"VALUE": chr("a")}));
    let (abs_real, abs_real_input) = value("math.abs", json!({}), json!({"VALUE": num("-2.5")}));
    let (max, max_input) = value(
        "math.minmax",
        json!({"FN": "max"}),
        json!({"A": boolean(true), "B": num("2")}),
    );
    let (min, min_input) = value(
        "math.minmax",
        json!({"FN": "min"}),
        json!({"A": chr("a"), "B": num("2.5")}),
    );
    let (power, power_input) = value(
        "math.fn_two",
        json!({"FN": "pow"}),
        json!({"A": num("2"), "B": num("10")}),
    );
    let (clamp, clamp_input) = value(
        "math.clamp",
        json!({}),
        json!({"VALUE": chr("z"), "LOW": num("0"), "HIGH": num("10")}),
    );
    let (random, random_input) = value(
        "math.random_real",
        json!({}),
        json!({"LOW": num("0"), "HIGH": num("1")}),
    );
    let a = check_main(vec![print(vec![
        sqrt_input,
        abs_input,
        abs_real_input,
        max_input,
        min_input,
        power_input,
        clamp_input,
        random_input,
    ])]);
    assert_clean(&a);
    // A whole number's square root is a decimal number (w2.md §5.2).
    assert_eq!(type_of(&a, &sqrt_of_int), Type::Double);
    assert_eq!(type_of(&a, &abs_char), Type::Int);
    assert_eq!(type_of(&a, &abs_real), Type::Double);
    assert_eq!(type_of(&a, &max), Type::Int);
    assert_eq!(type_of(&a, &min), Type::Double);
    assert_eq!(type_of(&a, &power), Type::Double);
    assert_eq!(type_of(&a, &clamp), Type::Int);
    assert_eq!(type_of(&a, &random), Type::Double);
}

#[test]
fn math_and_decimals_refuse_text() {
    let (_, abs) = value("math.abs", json!({}), json!({"VALUE": text("3")}));
    let (_, random) = value(
        "math.random_real",
        json!({}),
        json!({"LOW": num("1"), "HIGH": text("2")}),
    );
    let a = check_main(vec![print(vec![abs, random])]);
    assert_eq!(
        messages(&a),
        [
            "B2C-E0301 This is text, but this block needs a number.",
            "B2C-E0301 This is text, but this block needs a number.",
        ]
    );
}

// --- Errors ---------------------------------------------------------------------

#[test]
fn each_catch_part_declares_its_error_for_its_part_only() {
    let a = check_main(vec![try_catch(
        vec![throw("std::invalid_argument", text("bad"))],
        vec![
            (
                "std::invalid_argument",
                "e",
                vec![print(vec![message_of("e", 0)])],
            ),
            ("std::out_of_range", "e", vec![]),
            ("std::runtime_error", "e", vec![print(vec![message_of("e", 2)])]),
            ("std::exception", "other", vec![]),
        ],
    )]);
    assert_clean(&a);
    let symbol = |name: &str, index: usize| {
        a.program
            .symbols
            .symbols
            .iter()
            .find(|(id, _)| id.as_str() == catch_sym(name, index))
            .map(|(_, symbol)| symbol.ty.clone())
            .unwrap()
    };
    assert_eq!(symbol("e", 0), lib(LibType::InvalidArgument, vec![]));
    assert_eq!(symbol("e", 1), lib(LibType::OutOfRange, vec![]));
    assert_eq!(symbol("e", 2), lib(LibType::RuntimeError, vec![]));
    assert_eq!(symbol("other", 3), lib(LibType::Exception, vec![]));

    // The error of one part is not visible in another, nor after the block.
    let a = check_main(vec![
        try_catch(
            vec![],
            vec![
                ("std::invalid_argument", "e", vec![]),
                ("std::runtime_error", "f", vec![print(vec![message_of("e", 0)])]),
            ],
        ),
        print(vec![message_of("f", 1)]),
    ]);
    assert_eq!(
        messages(&a),
        [
            "B2C-E0203 `e` is the error caught by a 'catch' part and only exists inside that part.",
            "B2C-E0203 `f` is the error caught by a 'catch' part and only exists inside that part.",
        ]
    );
}

#[test]
fn a_catch_part_cannot_declare_its_error_name_again() {
    let a = check_main(vec![try_catch(
        vec![],
        vec![("std::exception", "e", vec![declare("int", "e", Some(num("1")))])],
    )]);
    assert_eq!(
        messages(&a),
        ["B2C-E0210 There is already a variable called `e` here. Choose another name."]
    );
}

#[test]
fn message_of_needs_a_caught_error() {
    let a = check_main(vec![
        declare("int", "x", Some(num("1"))),
        print(vec![input(json!({"id": id(), "type": "error.message", "v": 1,
                                "fields": {"ERROR": {"ref": sym("x")}}}))]),
    ]);
    assert_eq!(
        messages(&a),
        ["B2C-E0301 This is a whole number, but this block needs an error here."]
    );
}

#[test]
fn a_part_after_one_that_catches_its_error_never_runs() {
    let parts = |errors: &[&'static str]| -> Vec<(&'static str, &'static str, Vec<Value>)> {
        errors.iter().map(|error| (*error, "e", vec![])).collect()
    };
    // Any error first hides every later part.
    for later in ["std::invalid_argument", "std::out_of_range", "std::runtime_error"] {
        let a = check_main(vec![try_catch(vec![], parts(&["std::exception", later]))]);
        let noun = match later {
            "std::invalid_argument" => "invalid argument error",
            "std::out_of_range" => "out-of-range error",
            _ => "runtime error",
        };
        assert_eq!(
            messages(&a),
            [format!(
                "B2C-W0507 This part never runs: a part before it catches every error, so every {noun} too. \
                 Move this part above that one."
            )]
        );
        let warning = only(&a, "B2C-W0507");
        // At the part that never runs, pointing to the one that hides it.
        assert_eq!(
            warning.primary.part,
            Part::Field {
                name: String::from("ERROR1")
            }
        );
        assert_eq!(warning.related.len(), 1);
    }
    // The same error twice, with another part between.
    for error in [
        "std::exception",
        "std::invalid_argument",
        "std::out_of_range",
        "std::runtime_error",
    ] {
        let other = if error == "std::runtime_error" {
            "std::out_of_range"
        } else {
            "std::runtime_error"
        };
        let a = check_main(vec![try_catch(vec![], parts(&[error, other, error]))]);
        let codes: Vec<String> = codes(&a);
        if error == "std::exception" {
            // Any error hides both later parts.
            assert_eq!(codes, ["B2C-W0507", "B2C-W0507"], "{error}");
        } else {
            assert_eq!(codes, ["B2C-W0507"], "{error}");
            let noun = messages(&a)[0].clone();
            assert!(noun.contains("a part before it already catches every"), "{noun}");
        }
    }
    // Every other order: no part hides another (input_validation's order
    // among them).
    for order in [
        [
            "std::invalid_argument",
            "std::out_of_range",
            "std::runtime_error",
            "std::exception",
        ],
        [
            "std::runtime_error",
            "std::out_of_range",
            "std::invalid_argument",
            "std::exception",
        ],
    ] {
        let a = check_main(vec![try_catch(vec![], parts(&order))]);
        assert_clean(&a);
    }
}

#[test]
fn throw_ends_its_path() {
    // A function whose every path throws or returns gives back what it must.
    let a = check(vec![
        main(vec![print(vec![call("half", vec![num("4")])])]),
        func(
            "half",
            "int",
            &[("n", "int", "copy")],
            vec![
                if_then(
                    compare("lt", get("n"), num("0")),
                    vec![throw("std::out_of_range", text("negative"))],
                ),
                ret(Some(arith("div", get("n"), num("2")))),
            ],
        ),
        func(
            "fail",
            "int",
            &[],
            vec![throw("std::runtime_error", text("never"))],
        ),
    ]);
    assert_clean(&a);
    // A block after a throw never runs.
    let a = check_main(vec![
        throw("std::runtime_error", text("stop")),
        print(vec![num("1")]),
    ]);
    assert_codes(&a, &["B2C-W0502"]);
    // The message is text.
    let a = check_main(vec![throw("std::runtime_error", num("1"))]);
    assert_eq!(
        messages(&a),
        ["B2C-E0301 This is a whole number, but this block needs text or a character."]
    );
}

#[test]
fn throw_again_needs_a_catch_part_around_it() {
    let a = check_main(vec![rethrow()]);
    assert_eq!(
        messages(&a),
        [
            "B2C-E0402 'throw the error again' can only be used inside a 'catch' part, where there is an \
             error to throw again."
        ]
    );
    // Anywhere inside a catch part, also in an `if` or a loop there, but
    // not in the try part.
    let a = check_main(vec![try_catch(
        vec![rethrow()],
        vec![(
            "std::exception",
            "e",
            vec![
                if_then(boolean(true), vec![rethrow()]),
                repeat(num("2"), vec![rethrow()]),
            ],
        )],
    )]);
    assert_eq!(codes(&a), ["B2C-E0402"]);
}

#[test]
fn a_caught_error_cannot_be_changed_and_ends_with_its_part() {
    // Changing it is refused with its own reason (and *set* takes no error
    // type at all).
    let set_error = json!({"id": id(), "type": "var.set", "v": 1,
                           "fields": {"VAR": {"ref": catch_sym("e", 0)}},
                           "inputs": {"VALUE": text("other")}});
    let a = check_main(vec![try_catch(
        vec![],
        vec![("std::exception", "e", vec![set_error])],
    )]);
    assert_eq!(
        messages(&a),
        [
            "B2C-E0308 `e` is the error caught by a 'catch' part, so it can't be changed. Create a variable \
             for a value that changes.",
            "B2C-E0301 This is an error, but this block needs a value that a variable can hold here.",
        ]
    );
    // A block put after the try block does not see it: the scope query
    // answers there with what is visible at the try block.
    let block = try_catch(vec![], vec![("std::invalid_argument", "e", vec![])]);
    let try_id = BlockId::new(block["id"].as_str().unwrap()).unwrap();
    let a = check_main(vec![declare("int", "n", Some(num("1"))), block]);
    assert_clean(&a);
    let names = |infos: Vec<b2c_ir::scope_info::SymbolInfo>| -> Vec<String> {
        infos.into_iter().map(|info| info.name).collect()
    };
    assert_eq!(names(a.scope_after(&try_id).unwrap()), ["n"]);
    assert_eq!(a.scope_after(&try_id), a.scope_at(&try_id, None));
    // Inside the part, it is visible.
    assert_eq!(names(a.scope_at(&try_id, Some("CATCH0")).unwrap()), ["e", "n"]);
}
