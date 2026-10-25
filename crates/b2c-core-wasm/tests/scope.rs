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
use common::{example, examples, names, parse, update};
use serde_json::{Value, json};

/// The export's answer: a list, or `null` for no answer.
fn scope(block: &str, input: Option<&str>) -> Value {
    let output = b2c_core_wasm::symbols_in_scope(block, input.map(str::to_owned));
    assert!(!output.contains('\n'), "not compact: {output}");
    let value: Value = serde_json::from_str(&output).unwrap();
    assert!(
        value.is_array() || value.is_null(),
        "not a list or null: {output}"
    );
    value
}

#[test]
fn the_guessing_game_ask_block_sees_guess_and_secret() {
    update(&example("guessing_game"), 4);
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
    update(&example("factorial"), 4);
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
fn unknown_blocks_and_ids_have_no_answer() {
    update(&example("guessing_game"), 4);
    for block in ["nope", "", "b 005", "<script>", &"b".repeat(100)] {
        assert_eq!(scope(block, None), Value::Null, "{block:?}");
    }
    // An unknown input name answers at the block.
    assert_eq!(names(&scope("b005", Some("NOT_AN_INPUT"))), ["guess", "secret"]);
    assert_eq!(names(&scope("b005", Some(""))), ["guess", "secret"]);
    // A block that sees nothing has an answer: the empty list.
    assert_eq!(scope("b002", None), json!([]));
}

/// The guessing game with `edit` applied to its JSON.
fn edited_game(edit: impl FnOnce(&mut Value)) -> String {
    let mut document: Value = serde_json::from_str(&example("guessing_game")).unwrap();
    edit(&mut document);
    document.to_string()
}

/// A `print "text"` block with this ID.
fn print_block(id: &str, text: &str) -> Value {
    json!({"id": id, "type": "io.print", "v": 1, "extra": {"itemCount": 1},
           "fields": {"NEWLINE": true, "SEP": "none", "STREAM": "out"},
           "inputs": {"ITEM0": {"expr": [{"str": text}]}}})
}

#[test]
fn parked_blocks_answer_with_the_end_of_main() {
    // 06 §6.5: a loose stack, a block in it and a block nested in that one,
    // and a loose getter all see what is visible at the end of main's body.
    let text = edited_game(|document| {
        let mut head = print_block("loose_head", "parked");
        head["x"] = json!(600);
        head["y"] = json!(40);
        let repeat = json!({"id": "loose_loop", "type": "control.forever", "v": 1,
                            "statements": {"BODY": [print_block("loose_inner", "inside")]}});
        head["stack"] = json!([repeat]);
        let getter = json!({"id": "loose_get", "type": "var.get", "v": 1, "x": 600, "y": 400,
                            "fields": {"VAR": {"ref": "s_guess"}}});
        let blocks = document["modules"][0]["workspace"]["blocks"]
            .as_array_mut()
            .unwrap();
        blocks.push(head);
        blocks.push(getter);
    });
    let result = update(&text, 4);
    assert_ne!(result["stage"], "load", "{}", result["diagnostics"]);
    for id in ["loose_head", "loose_loop", "loose_inner", "loose_get"] {
        assert_eq!(names(&scope(id, None)), ["guess", "secret"], "{id}");
    }
    assert_eq!(names(&scope("loose_loop", Some("BODY"))), ["guess", "secret"]);
    // The same answer as the analyser's.
    let analysis = common::native_analysis(&text);
    let session = {
        let mut session = Session::new();
        session.update(&text, &PreviewOptions::default());
        session
    };
    assert_eq!(
        session.symbols_in_scope("loose_get", None),
        Some(analysis.parked_scope(&b2c_ir::ModuleId::new("mod_main").unwrap()))
    );
}

#[test]
fn blocks_in_a_disabled_loop_answer_like_the_loop() {
    // The loop (b010) and everything in it: what is visible at the loop.
    let text = edited_game(|document| {
        document["modules"][0]["workspace"]["blocks"][0]["statements"]["BODY"][3]["disabled"] = json!(true);
    });
    update(&text, 4);
    for id in ["b010", "b005", "b009", "b006", "b008"] {
        assert_eq!(names(&scope(id, None)), ["guess", "secret"], "{id}");
    }
    assert_eq!(names(&scope("b009", Some("DO1"))), ["guess", "secret"]);
}

#[test]
fn the_query_answers_from_the_last_document_that_loaded() {
    // Nothing has loaded yet: no answer.
    let failed = update("{", 4);
    assert_eq!(failed["stage"], "load");
    assert_eq!(scope("b005", None), Value::Null);

    update(&example("guessing_game"), 4);
    assert_eq!(names(&scope("b005", None)), ["guess", "secret"]);
    // Invalid options run no preview and keep the analysis.
    let refused = parse(&b2c_core_wasm::update("{", "{}"));
    assert_eq!(refused["error"]["kind"], "invalidOptions");
    assert_eq!(names(&scope("b005", None)), ["guess", "secret"]);
    // Another document replaces it.
    update(&example("factorial"), 2);
    assert_eq!(scope("b005", None).as_array().unwrap().len(), 2);
    assert_eq!(names(&scope("b005", None)), ["factorial", "i"]);
    // A document that does not load keeps it: the editor still shows the
    // last document that loaded, and its menus ask about that one.
    let failed = update("[]", 4);
    assert_eq!(failed["stage"], "load");
    assert_eq!(failed["textHash"], Value::Null);
    assert_eq!(names(&scope("b005", None)), ["factorial", "i"]);
}

#[test]
fn a_session_without_a_preview_knows_nothing() {
    let session = Session::new();
    assert!(!session.has_preview());
    assert_eq!(session.symbols_in_scope("b005", None), None);
    let mut session = session;
    session.update(&example("guessing_game"), &PreviewOptions::default());
    assert!(session.has_preview());
    assert_eq!(session.symbols_in_scope("b005", None).map(|s| s.len()), Some(2));
}

#[test]
fn every_example_answers_the_same_as_the_analyser() {
    for example in examples() {
        let mut session = Session::new();
        session.update(&example.text, &PreviewOptions::default());
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
                    analysis.scope_at(&block, input),
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
    let result = update(&example("guessing_game"), 4);
    // `random 1 to 100` is the only value block.
    assert_eq!(result["blockTypes"], json!({"b001": "int"}));
    let factorial = update(&example("factorial"), 4);
    assert_eq!(factorial["blockTypes"], json!({"b005": "int"}));
    assert_eq!(
        names(&factorial["symbols"]),
        ["factorial", "i", "n"],
        "every symbol, sorted by name"
    );
}

/// `hello_world` printing a join, a comparison and a random number, each
/// with expression slots: `(print ID, document)`.
fn typed_values() -> (String, String) {
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
    let id = print["id"].as_str().unwrap().to_owned();
    (id, serde_json::to_string(&document).unwrap())
}

#[test]
fn block_types_follow_the_values() {
    let (_, text) = typed_values();
    let result = update(&text, 4);
    assert_eq!(result["stage"], "generate", "{}", result["diagnostics"]);
    assert_eq!(
        result["blockTypes"],
        json!({"t_cmp": "bool", "t_join": "string", "t_rand": "int"})
    );
}

/// `update()` carries `slotTypes` (slots.md §4.1): the type of each
/// expression slot by block and input, never a block's own type.
#[test]
fn slot_types_follow_the_expression_slots() {
    let (print, text) = typed_values();
    let result = update(&text, 4);
    assert_eq!(
        result["slotTypes"],
        json!({"t_cmp": {"A": "int", "B": "double"}, "t_join": {"ITEM0": "string", "ITEM1": "string"},
               "t_rand": {"HIGH": "int", "LOW": "int"}})
    );
    // The print block's inputs hold blocks: their types are block types.
    assert!(result["slotTypes"].get(&print).is_none());
    for block in ["t_cmp", "t_join", "t_rand"] {
        assert!(result["blockTypes"].get(block).is_some(), "{block}");
    }
    let game = update(&example("guessing_game"), 4);
    assert_eq!(
        game["slotTypes"]["b009"],
        json!({"COND0": "bool", "COND1": "bool"})
    );
    assert_eq!(game["slotTypes"]["b003"], json!({"VALUE": "int"}));
    assert_eq!(game["slotTypes"]["b005"], json!({"PROMPT": "string"}));
    // Always present: empty when nothing loads.
    assert_eq!(update("{", 4)["slotTypes"], json!({}));
    assert_eq!(
        update(&example("hello_world"), 4)["slotTypes"]
            .as_object()
            .unwrap()
            .len(),
        1
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

/// The table over the sample types is the shared type fixture's
/// `conversions` (types.md §3.2, §9), with the samples in the same order.
#[test]
fn the_conversion_table_is_the_type_fixtures() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../packages/blockly-ext/src/types/fixture.json"
    );
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let samples = b2c_ir::types::sample_types();
    let fixture_samples = fixture["samples"].as_array().unwrap();
    assert_eq!(fixture_samples.len(), samples.len());
    for (ty, sample) in samples.iter().zip(fixture_samples) {
        assert_eq!(
            serde_json::to_value(b2c_ir::TermForm(ty)).unwrap(),
            sample["type"]
        );
    }
    let table = b2c_core_wasm::scope::conversion_table();
    assert_eq!(table.len(), samples.len() * samples.len());
    for (index, row) in table.iter().enumerate() {
        let expected = &fixture["conversions"][index / samples.len()][index % samples.len()];
        assert_eq!(row.from, samples[index / samples.len()]);
        assert_eq!(row.to, samples[index % samples.len()]);
        assert_eq!(row.conversion.name(), expected, "{:?} → {:?}", row.from, row.to);
    }
    // The export writes M2's rows of the same table until the switch.
    let exported: Vec<Value> = serde_json::from_str(&b2c_core_wasm::conversion_table()).unwrap();
    let m2: Vec<Value> = table
        .iter()
        .filter(|row| {
            b2c_core_wasm::STATIC_TYPES.contains(&row.from) && b2c_core_wasm::STATIC_TYPES.contains(&row.to)
        })
        .map(|row| serde_json::to_value(row).unwrap())
        .collect();
    assert_eq!(exported, m2);
}
