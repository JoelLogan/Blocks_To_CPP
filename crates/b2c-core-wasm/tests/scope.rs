//! The scope query, the block types and the conversion table through the
//! exports, as the editor calls them: the scope query answers from the
//! analysis that the last `preview` kept (06 §6.5), and the conversion
//! table is the analyser's own rule (06 §6.6).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests fail by panicking"
)]

mod common;

use b2c_core_wasm::{PreviewOptions, Session};
use b2c_ir::Type;
use common::{example, examples, names, parse, preview};
use serde_json::{Value, json};

fn scope(block: &str, input: Option<&str>) -> Value {
    let output = b2c_core_wasm::symbols_in_scope(block, input.map(str::to_owned));
    assert!(!output.contains('\n'), "not compact: {output}");
    let value: Value = serde_json::from_str(&output).unwrap();
    assert!(value.is_array(), "not a list: {output}");
    value
}

#[test]
fn the_guessing_game_ask_block_sees_guess_and_secret() {
    preview(&example("guessing_game"), 4);
    assert_eq!(names(&scope("b005", None)), ["guess", "secret"]);
    // At `create secret` nothing is declared yet (and the name being
    // created is not visible in its own starting value).
    assert_eq!(names(&scope("b002", None)), Vec::<String>::new());
    assert_eq!(names(&scope("b001", None)), Vec::<String>::new());
    assert_eq!(names(&scope("b003", None)), ["secret"]);
    // The start of main's list, and the loop's body.
    assert_eq!(names(&scope("b011", Some("BODY"))), Vec::<String>::new());
    assert_eq!(names(&scope("b010", Some("BODY"))), ["guess", "secret"]);
    // A value input answers at the block.
    assert_eq!(names(&scope("b010", Some("COND"))), ["guess", "secret"]);
    // The records are the spec's SymbolInfo shape.
    assert_eq!(
        scope("b005", None)[0],
        json!({"id": "s_guess", "name": "guess", "kind": "variable", "isConst": false,
               "type": "int", "module": "mod_main", "declBlock": "b003"})
    );
}

#[test]
fn loop_counters_parameters_and_functions() {
    preview(&example("factorial"), 4);
    // Inside the function: its parameter and every function of the module.
    assert_eq!(names(&scope("b003", None)), ["factorial", "n"]);
    assert_eq!(names(&scope("b004", Some("BODY"))), ["factorial", "n"]);
    // In main: the loop counter only inside the loop.
    assert_eq!(names(&scope("b007", None)), ["factorial"]);
    assert_eq!(names(&scope("b007", Some("BODY"))), ["factorial", "i"]);
    assert_eq!(names(&scope("b005", None)), ["factorial", "i"]);
    let function = scope("b006", None)[0].clone();
    assert_eq!(function["kind"], "function");
    assert_eq!(function["returns"], "int");
    assert_eq!(function["params"], json!(["s_n"]));
    let parameter = scope("b003", None)[1].clone();
    assert_eq!(parameter["kind"], "parameter");
    assert_eq!(parameter["mode"], "copy");
}

#[test]
fn unknown_blocks_and_ids_give_nothing() {
    preview(&example("guessing_game"), 4);
    for block in ["nope", "", "b 005", "<script>", &"b".repeat(100)] {
        assert_eq!(scope(block, None), json!([]), "{block:?}");
    }
    // An unknown input name answers at the block.
    assert_eq!(names(&scope("b005", Some("NOT_AN_INPUT"))), ["guess", "secret"]);
    assert_eq!(names(&scope("b005", Some(""))), ["guess", "secret"]);
}

#[test]
fn the_query_answers_from_the_last_preview_only() {
    // A preview that does not load forgets the analysis.
    let failed = preview("{", 4);
    assert_eq!(failed["stage"], "load");
    assert_eq!(scope("b005", None), json!([]));

    preview(&example("guessing_game"), 4);
    assert_eq!(names(&scope("b005", None)), ["guess", "secret"]);
    // Invalid options run no preview and keep the analysis.
    let refused = parse(&b2c_core_wasm::preview("{", "{}"));
    assert_eq!(refused["error"]["kind"], "invalidOptions");
    assert_eq!(names(&scope("b005", None)), ["guess", "secret"]);
    // Another document replaces it.
    preview(&example("factorial"), 2);
    assert_eq!(scope("b005", None).as_array().unwrap().len(), 2);
    assert_eq!(names(&scope("b005", None)), ["factorial", "i"]);
    preview("[]", 4);
    assert_eq!(scope("b005", None), json!([]));
}

#[test]
fn a_session_without_a_preview_knows_nothing() {
    let session = Session::new();
    assert!(!session.has_preview());
    assert!(session.symbols_in_scope("b005", None).is_empty());
    let mut session = session;
    session.preview(&example("guessing_game"), &PreviewOptions::default());
    assert!(session.has_preview());
    assert_eq!(session.symbols_in_scope("b005", None).len(), 2);
}

#[test]
fn every_example_answers_the_same_as_the_analyser() {
    for example in examples() {
        let mut session = Session::new();
        session.preview(&example.text, &PreviewOptions::default());
        let analysis = common::native_analysis(&example.text);
        let document: Value = serde_json::from_str(&example.text).unwrap();
        let mut ids = Vec::new();
        collect_ids(&document, &mut ids);
        assert!(!ids.is_empty());
        for id in ids {
            let block = b2c_ir::BlockId::new(&id).unwrap();
            for input in [None, Some("BODY"), Some("DO0"), Some("ELSE"), Some("VALUE")] {
                assert_eq!(
                    session.symbols_in_scope(&id, input),
                    analysis.symbols_in_scope(&block, input),
                    "{} {id} {input:?}",
                    example.name
                );
            }
        }
    }
}

/// Every block ID in a document's JSON.
fn collect_ids(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if let (Some(Value::String(id)), Some(_), Some(_)) =
                (map.get("id"), map.get("type"), map.get("v"))
            {
                out.push(id.clone());
            }
            map.values().for_each(|v| collect_ids(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| collect_ids(v, out)),
        _ => {}
    }
}

#[test]
fn block_types_of_the_examples() {
    let result = preview(&example("guessing_game"), 4);
    // `random 1 to 100` is the only value block.
    assert_eq!(result["blockTypes"], json!({"b001": "int"}));
    let factorial = preview(&example("factorial"), 4);
    assert_eq!(factorial["blockTypes"], json!({"b005": "int"}));
    assert_eq!(
        names(&factorial["symbols"]),
        ["factorial", "i", "n"],
        "every symbol, sorted by name"
    );
}

#[test]
fn block_types_follow_the_values() {
    let mut document: Value = serde_json::from_str(&example("hello_world")).unwrap();
    let print = &mut document["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"][0];
    print["extra"]["itemCount"] = json!(3);
    print["inputs"]["ITEM0"] = json!({"block": {"id": "t_join", "type": "text.join", "v": 1,
        "extra": {"itemCount": 2},
        "inputs": {"ITEM0": {"expr": [{"str": "a"}]}, "ITEM1": {"expr": [{"str": "b"}]}}}});
    print["inputs"]["ITEM1"] = json!({"block": {"id": "t_cmp", "type": "math.compare", "v": 1,
        "fields": {"OP": "lt"},
        "inputs": {"A": {"expr": [{"num": "1"}]}, "B": {"expr": [{"num": "2.5"}]}}}});
    print["inputs"]["ITEM2"] = json!({"block": {"id": "t_rand", "type": "math.random_int", "v": 1,
        "inputs": {"LOW": {"expr": [{"num": "1"}]}, "HIGH": {"expr": [{"num": "6"}]}}}});
    let text = serde_json::to_string(&document).unwrap();
    let result = preview(&text, 4);
    assert_eq!(result["stage"], "generate", "{}", result["diagnostics"]);
    assert_eq!(
        result["blockTypes"],
        json!({"t_cmp": "bool", "t_join": "string", "t_rand": "int"})
    );
}

#[test]
fn the_conversion_table_is_the_analysers_rule() {
    let output = b2c_core_wasm::conversion_table();
    assert!(!output.contains('\n'));
    let table: Vec<Value> = serde_json::from_str(&output).unwrap();
    assert_eq!(table.len(), 49);
    let types = b2c_core_wasm::STATIC_TYPES;
    let mut index = 0;
    for from in &types {
        for to in &types {
            let row = &table[index];
            assert_eq!(row.as_object().unwrap().len(), 3);
            assert_eq!(row["from"], serde_json::to_value(from).unwrap());
            assert_eq!(row["to"], serde_json::to_value(to).unwrap());
            assert_eq!(row["conversion"], b2c_lang::conversion(from, to).name());
            index += 1;
        }
    }
    // A few rows by name, as the connection checker reads them.
    let find = |from: Type, to: Type| {
        let row = &table[types.iter().position(|t| *t == from).unwrap() * 7
            + types.iter().position(|t| *t == to).unwrap()];
        row["conversion"].as_str().unwrap().to_owned()
    };
    assert_eq!(find(Type::Int, Type::Double), "widening");
    assert_eq!(find(Type::Double, Type::Int), "narrowing");
    assert_eq!(find(Type::Bool, Type::Int), "boolNumber");
    assert_eq!(find(Type::String, Type::Int), "invalid");
    assert_eq!(find(Type::Void, Type::Int), "invalid");
    assert_eq!(find(Type::Error, Type::String), "same");
    assert_eq!(find(Type::Char, Type::String), "invalid");
}
