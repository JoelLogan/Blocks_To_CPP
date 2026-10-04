//! Regression tests: one or more per loader rule (spec §5.6), with the code,
//! the location and the wording users see.

// Helper functions outside `#[test]`s fail the test by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

mod common;

use b2c_ir::{BlockId, DiagSource, Location, ModuleId, Part, Severity};
use b2c_model::limits::{MAX_BLOCKS, MAX_EXPR_TOKENS, MAX_FILE_BYTES, MAX_MODULES, MAX_STRING_BYTES};
use b2c_model::{Document, FieldValue, Input, Token, load};
use common::{base, bytes, codes, failure, main_block, print_block};
use serde_json::{Value, json};

fn block_location(block: &str, part: Part) -> Location {
    Location {
        module: Some(ModuleId::new("mod_main").unwrap()),
        block: Some(BlockId::new(block).unwrap()),
        part,
    }
}

fn loads(value: &Value) -> Document {
    match load(&bytes(value)) {
        Ok(document) => document,
        Err(error) => panic!("{:#?}", error.diagnostics),
    }
}

#[test]
fn the_base_document_loads() {
    let document = loads(&base());
    assert_eq!(document.modules.len(), 1);
    let main = &document.modules[0].workspace.blocks[0];
    assert_eq!(main.block_type, "program.main");
    assert_eq!(main.x, Some(0));
    let print = &main.statements["BODY"][0];
    assert_eq!(print.fields["NEWLINE"], FieldValue::Bool(true));
    assert!(matches!(&print.inputs["ITEM0"], Input::Expr(e) if e.expr == [Token::Str("hi".into())]));
    assert_eq!(print.extra["itemCount"], json!(1));
}

#[test]
fn every_diagnostic_is_a_loader_error() {
    let mut document = base();
    document["project"]["colour"] = json!("red");
    for diagnostic in failure(&bytes(&document)) {
        assert_eq!(diagnostic.source, DiagSource::Loader);
        assert_eq!(diagnostic.severity, Severity::Error);
        assert!(diagnostic.code.0.starts_with("B2C-E01"));
    }
}

#[test]
fn every_problem_is_reported_not_just_the_first() {
    let mut document = base();
    document["project"]["colour"] = json!("red");
    print_block(&mut document)["fields"]["SEP"] = json!(5);
    document["modules"][0]["name"] = json!("Main");
    main_block(&mut document)["statements"]["BODY"][1]["id"] = json!("b_main");
    document["modules"][0]["workspace"]["viewport"] = json!({"x": 0, "y": 0, "scale": 9});
    assert_eq!(
        codes(&document),
        ["B2C-E0110", "B2C-E0117", "B2C-E0112", "B2C-E0114", "B2C-E0130"]
    );
}

// --- E0101–E0109: bytes, JSON and header (see also the unit tests) --------

#[test]
fn e0101_file_too_large() {
    let diagnostics = failure(&vec![b' '; MAX_FILE_BYTES + 1]);
    assert_eq!(diagnostics[0].code.0, "B2C-E0101");
    assert!(load(&vec![b' '; MAX_FILE_BYTES]).is_err_and(|e| e.diagnostics[0].code.0 == "B2C-E0103"));
}

#[test]
fn e0102_to_e0105_parse_errors() {
    assert_eq!(failure(b"{\"a\": \"\xc3\x28\"}")[0].code.0, "B2C-E0102");
    assert_eq!(failure(b"\xef\xbb\xbf{}")[0].code.0, "B2C-E0102");
    assert_eq!(failure(b"{,}")[0].code.0, "B2C-E0103");
    assert_eq!(failure("[".repeat(129).as_bytes())[0].code.0, "B2C-E0104");
    let text = String::from_utf8(bytes(&base())).unwrap().replacen(
        "\"SEP\": \"none\"",
        "\"SEP\": \"none\", \"SEP\": \"space\"",
        1,
    );
    let diagnostics = failure(text.as_bytes());
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code.0, "B2C-E0105");
    assert!(
        diagnostics[0]
            .message
            .starts_with("The key \"SEP\" appears twice")
    );
}

#[test]
fn e0106_too_many_values() {
    let mut text = String::from("[");
    text.push_str(&"0,".repeat(4 * 1024 * 1024));
    text.push('0');
    text.push(']');
    assert_eq!(failure(text.as_bytes())[0].code.0, "B2C-E0106");
}

#[test]
fn e0107_to_e0109_header() {
    let mut document = base();
    document["format"] = json!("blocks2cpp/clipboard");
    assert_eq!(codes(&document), ["B2C-E0107"]);
    let mut document = base();
    document["formatVersion"] = json!(2);
    document["unknownFutureKey"] = json!(true);
    assert_eq!(codes(&document), ["B2C-E0108"]);
    let mut document = base();
    document["formatVersion"] = json!(0);
    assert_eq!(codes(&document), ["B2C-E0109"]);
}

// --- E0110–E0113: shape of the file ---------------------------------------

#[test]
fn e0110_unknown_keys_everywhere() {
    let mut document = base();
    document["extra"] = json!(1);
    document["project"]["build"] = json!({"compilerFlags": "-fplugin=evil.so"});
    document["project"]["run"] = json!({"command": "rm -rf ~"});
    print_block(&mut document)["next"] = json!({});
    print_block(&mut document)["comment"] = json!({"text": "hi", "colour": "red"});
    let diagnostics = failure(&bytes(&document));
    let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "The project file has an unknown key \"extra\". Remove it or check its spelling.",
            "\"project.build\" has an unknown key \"compilerFlags\". Remove it or check its spelling.",
            "\"project.run\" has an unknown key \"command\". Remove it or check its spelling.",
            "This block has an unknown key \"next\". Remove it or check its spelling.",
            "\"comment\" in this block has an unknown key \"colour\". Remove it or check its spelling.",
        ]
    );
    assert!(diagnostics.iter().all(|d| d.code.0 == "B2C-E0110"));
    assert_eq!(diagnostics[3].primary, block_location("b_print", Part::Whole));
}

#[test]
fn e0111_missing_keys() {
    let mut document = base();
    document["project"].as_object_mut().unwrap().remove("name");
    print_block(&mut document).as_object_mut().unwrap().remove("type");
    print_block(&mut document)["inputs"]["ITEM0"] = json!({});
    main_block(&mut document)["statements"]["BODY"][1]
        .as_object_mut()
        .unwrap()
        .remove("id");
    let diagnostics = failure(&bytes(&document));
    let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "\"project\" is missing \"name\".",
            "This block is missing \"type\".",
            "\"inputs.ITEM0\" in this block should contain either a block (\"block\") or an expression (\"expr\").",
            "\"statements.BODY[1]\" in this block is a block without an \"id\".",
        ]
    );
    assert!(diagnostics.iter().all(|d| d.code.0 == "B2C-E0111"));
    assert_eq!(
        diagnostics[2].primary,
        block_location("b_print", Part::Input { name: "ITEM0".into() })
    );
    // The block without an ID is reported where it sits: in `main`.
    assert_eq!(diagnostics[3].primary, block_location("b_main", Part::Whole));
}

#[test]
fn e0112_wrong_values() {
    let mut document = base();
    document["formatVersion"] = json!(1);
    document["project"]["language"]["standard"] = json!("c++11");
    document["project"]["options"] = json!({"showAdvanced": "yes"});
    print_block(&mut document)["v"] = json!(-1);
    print_block(&mut document)["fields"]["SEP"] = json!(42);
    print_block(&mut document)["inputs"]["ITEM0"]["expr"][0] = json!({"str": "a", "num": "1"});
    let diagnostics = failure(&bytes(&document));
    let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
    assert_eq!(
        messages,
        [
            "\"project.language.standard\" should be one of \"c++17\", \"c++20\", \"c++23\" or \"c++26\", but it is the text \"c++11\".",
            "\"project.options.showAdvanced\" should be true or false, but it is the text \"yes\".",
            "\"v\" in this block should be a whole number from 0 to 4294967295, but it is the number -1.",
            "\"fields.SEP\" in this block should be text (numbers in fields are stored as text, for example \"42\"), but it is the number 42.",
            "\"inputs.ITEM0.expr[0]\" in this block should be a token such as {\"num\": \"42\"}, {\"str\": \"text\"} or {\"ref\": \"sym_x\"}, but it is an object.",
        ]
    );
    assert_eq!(
        diagnostics[4].primary,
        block_location(
            "b_print",
            Part::Tokens {
                input: "ITEM0".into(),
                start: 0,
                end: 1
            }
        )
    );
    assert_eq!(
        diagnostics[3].primary,
        block_location("b_print", Part::Field { name: "SEP".into() })
    );
}

#[test]
fn e0112_numbers_and_kinds() {
    let mut document = base();
    document["project"]["build"] = json!({"defines": [
        {"name": "A", "value": {"int": 9_223_372_036_854_775_808_u64}},
        {"name": "B", "value": {"float": 1.5}},
        {"name": "C", "value": "x"},
    ]});
    main_block(&mut document)["x"] = json!(1.5);
    main_block(&mut document)["collapsed"] = json!(null);
    assert_eq!(
        codes(&document),
        ["B2C-E0112", "B2C-E0110", "B2C-E0112", "B2C-E0112", "B2C-E0112"]
    );
}

#[test]
fn null_means_absent_for_optional_values() {
    let mut document = base();
    main_block(&mut document)["x"] = json!(null);
    main_block(&mut document)["comment"] = json!(null);
    document["x-ext"] = json!(null);
    document["modules"][0]["workspace"]["viewport"] = json!(null);
    let loaded = loads(&document);
    assert_eq!(loaded.modules[0].workspace.blocks[0].x, None);
    assert_eq!(loaded.ext, None);
}

#[test]
fn e0113_bad_ids() {
    let mut document = base();
    document["project"]["id"] = json!("prj test");
    print_block(&mut document)["id"] = json!("<script>");
    print_block(&mut document)["inputs"]["ITEM0"]["expr"][0] = json!({"ref": "x); system(\"ls\""});
    document["modules"][0]["id"] = json!("");
    let diagnostics = failure(&bytes(&document));
    assert!(
        diagnostics.iter().all(|d| d.code.0 == "B2C-E0113"),
        "{diagnostics:#?}"
    );
    assert_eq!(diagnostics.len(), 4);
    assert_eq!(
        diagnostics[0].message,
        "\"project.id\" is not a valid ID: \"prj test\" is not 1 to 32 characters from A–Z, a–z, 0–9 and _."
    );
}

// --- E0114–E0120: uniqueness and modules ----------------------------------

#[test]
fn e0114_duplicate_block_frame_and_note_ids() {
    let mut document = base();
    main_block(&mut document)["statements"]["BODY"][1]["inputs"]["VALUE"]["block"]["id"] = json!("b_print");
    document["modules"][0]["workspace"]["frames"] =
        json!([{"id": "b_fn", "title": "T", "x": 0, "y": 0, "w": 10, "h": 10, "color": "blue"}]);
    document["modules"][0]["workspace"]["notes"] = json!([{"id": "b_fn", "text": "n", "x": 0, "y": 0}]);
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 3);
    assert!(diagnostics.iter().all(|d| d.code.0 == "B2C-E0114"));
    assert_eq!(diagnostics[0].primary, block_location("b_print", Part::Whole));
    assert_eq!(
        diagnostics[0].related[0].location,
        block_location("b_print", Part::Whole)
    );
    assert_eq!(diagnostics[0].related[0].message, "the ID is first used here");
}

#[test]
fn e0115_duplicate_symbols() {
    let mut document = base();
    // A parameter and a variable share a symbol ID; a function name too.
    main_block(&mut document)["statements"]["BODY"][1]["fields"]["NAME"]["sym"] = json!("s_n");
    document["modules"][0]["workspace"]["blocks"][1]["fields"]["NAME"]["sym"] = json!("s_n");
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics.iter().all(|d| d.code.0 == "B2C-E0115"));
    // `extra` (the parameter row) is decoded before `fields` (the name).
    assert_eq!(diagnostics[0].primary, block_location("b_fn", Part::Whole));
    assert_eq!(
        diagnostics[1].primary,
        block_location("b_fn", Part::Field { name: "NAME".into() })
    );
    for diagnostic in &diagnostics {
        assert_eq!(
            diagnostic.related[0].location,
            block_location("b_var", Part::Field { name: "NAME".into() })
        );
    }
    // References may repeat freely.
    let mut document = base();
    print_block(&mut document)["inputs"]["ITEM0"]["expr"] =
        json!([{"ref": "s_x"}, {"op": "+"}, {"ref": "s_x"}]);
    loads(&document);
}

#[test]
fn e0116_to_e0119_modules() {
    let module = |id: &str, name: &str| json!({"id": id, "name": name, "workspace": {}});
    let mut document = base();
    let modules = document["modules"].as_array_mut().unwrap();
    modules.push(module("mod_main", "util"));
    modules.push(module("mod_a", "con"));
    modules.push(module("mod_b", "lpt9"));
    modules.push(module("mod_c", "com\u{b9}"));
    modules.push(module("mod_d", "../evil"));
    modules.push(module("mod_e", "Util"));
    modules.push(module("mod_f", "util"));
    assert_eq!(
        codes(&document),
        [
            "B2C-E0116",
            "B2C-E0118",
            "B2C-E0118",
            "B2C-E0118",
            "B2C-E0117",
            "B2C-E0117",
            "B2C-E0119",
            "B2C-E0119"
        ]
    );
    let diagnostics = failure(&bytes(&document));
    assert_eq!(
        diagnostics[1].message,
        "The module name \"con\" is reserved by Windows for a device, so it cannot be used as a file name. Choose another name."
    );
    assert_eq!(
        diagnostics[1].primary,
        Location {
            module: Some(ModuleId::new("mod_a").unwrap()),
            block: None,
            part: Part::Whole
        }
    );
}

#[test]
fn e0120_module_count() {
    let mut document = base();
    document["modules"] = json!([]);
    assert_eq!(codes(&document), ["B2C-E0120"]);
    let mut document = base();
    let modules: Vec<Value> = (0..=MAX_MODULES)
        .map(|i| json!({"id": format!("m{i}"), "name": format!("m{i}"), "workspace": {}}))
        .collect();
    document["modules"] = Value::Array(modules);
    assert_eq!(codes(&document), ["B2C-E0120"]);
    document["modules"].as_array_mut().unwrap().pop();
    loads(&document);
}

// --- E0121–E0123: counts and lengths -------------------------------------

#[test]
fn e0121_too_many_blocks_counts_nested_blocks() {
    let block = |i: usize| json!({"id": format!("b{i}"), "type": "control.break", "v": 1});
    let mut document = base();
    let body: Vec<Value> = (0..MAX_BLOCKS - 5).map(block).collect();
    main_block(&mut document)["statements"]["BODY"] = Value::Array(body);
    // main + function + the body = MAX_BLOCKS - 3: still fine.
    loads(&document);
    let more: Vec<Value> = (0..4).map(|i| block(MAX_BLOCKS + i)).collect();
    document["modules"][0]["workspace"]["blocks"][1]["statements"] = json!({"BODY": more});
    assert_eq!(codes(&document), ["B2C-E0121"]);
}

#[test]
fn e0122_too_many_tokens() {
    let mut document = base();
    let tokens: Vec<Value> = (0..MAX_EXPR_TOKENS).map(|_| json!({"num": "1"})).collect();
    print_block(&mut document)["inputs"]["ITEM0"]["expr"] = Value::Array(tokens.clone());
    loads(&document);
    let mut more = tokens;
    more.push(json!({"op": "+"}));
    print_block(&mut document)["inputs"]["ITEM0"]["expr"] = Value::Array(more);
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code.0, "B2C-E0122");
    assert_eq!(
        diagnostics[0].primary,
        block_location("b_print", Part::Input { name: "ITEM0".into() })
    );
}

#[test]
fn e0123_too_long() {
    let mut document = base();
    print_block(&mut document)["inputs"]["ITEM0"]["expr"][0]["str"] = json!("é".repeat(MAX_STRING_BYTES / 2));
    loads(&document);
    print_block(&mut document)["inputs"]["ITEM0"]["expr"][0]["str"] = json!("x".repeat(MAX_STRING_BYTES + 1));
    main_block(&mut document)["statements"]["BODY"][1]["fields"]["NAME"]["name"] = json!("n".repeat(65));
    document["modules"][0]["workspace"]["blocks"][1]["extra"]["params"][0]["name"] = json!("p".repeat(65));
    document["project"]["description"] = json!("d".repeat(MAX_STRING_BYTES + 1));
    let diagnostics = failure(&bytes(&document));
    assert!(diagnostics.iter().all(|d| d.code.0 == "B2C-E0123"));
    assert_eq!(diagnostics.len(), 4);
    assert_eq!(
        diagnostics[0].message,
        "\"project.description\" is 65537 bytes long, but text can be at most 65536 bytes (64 KiB)."
    );
}

#[test]
fn e0123_name_length_counts_characters() {
    // 40 two-byte characters are 80 bytes but only 40 characters: within the
    // limit (whether such a name is a valid identifier is the analyser's
    // business). The message never contradicts the limit.
    let mut document = base();
    main_block(&mut document)["statements"]["BODY"][1]["fields"]["NAME"]["name"] = json!("é".repeat(40));
    document["modules"][0]["workspace"]["blocks"][1]["extra"]["params"][0]["name"] = json!("é".repeat(40));
    loads(&document);
    main_block(&mut document)["statements"]["BODY"][1]["fields"]["NAME"]["name"] = json!("é".repeat(65));
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].message,
        "\"fields.NAME.name\" in this block is 65 characters long, but names can be at most 64."
    );
}

// --- E0124–E0127: text rules and reserved keys ----------------------------

#[test]
fn e0124_to_e0126_text_rules_in_every_kind_of_string() {
    let mut document = base();
    document["project"]["name"] = json!("evil\u{0}");
    print_block(&mut document)["comment"] = json!({"text": "clear \u{1b}[2J screen"});
    print_block(&mut document)["inputs"]["ITEM0"]["expr"][0]["str"] = json!("admin\u{202e} \u{2066}");
    main_block(&mut document)["statements"]["BODY"][1]["fields"]["NAME"]["name"] = json!("x\u{200f}");
    print_block(&mut document)["fields"]["SEP\u{202e}"] = json!("none");
    document["x-ext"] = json!({"tool": "line\rbreak"});
    document["project"]["run"] = json!({"args": ["a\u{0}b"]});
    let diagnostics = failure(&bytes(&document));
    let found: Vec<(&str, &str)> = diagnostics
        .iter()
        .map(|d| (d.code.0.as_str(), d.message.as_str()))
        .collect();
    assert_eq!(
        found,
        [
            (
                "B2C-E0124",
                "\"project.name\" contains the NUL character (U+0000), which is not allowed in project text."
            ),
            (
                "B2C-E0124",
                "\"project.run.args[0]\" contains the NUL character (U+0000), which is not allowed in project text."
            ),
            (
                "B2C-E0125",
                "\"comment.text\" in this block contains an escape character (U+001B), which is not allowed in project text. Only tabs and new lines are allowed."
            ),
            (
                "B2C-E0126",
                "\"fields\" in this block has a key that contains an invisible right-to-left override character (U+202E). Such characters can make text look different from what it really is, so they are not allowed."
            ),
            (
                "B2C-E0126",
                "\"inputs.ITEM0.expr[0].str\" in this block contains an invisible right-to-left override character (U+202E). Such characters can make text look different from what it really is, so they are not allowed."
            ),
            (
                "B2C-E0126",
                "\"fields.NAME.name\" in this block contains an invisible right-to-left mark character (U+200F). Such characters can make text look different from what it really is, so they are not allowed."
            ),
            (
                "B2C-E0125",
                "\"x-ext.tool\" contains a carriage return (U+000D), which is not allowed in project text. Only tabs and new lines are allowed."
            ),
        ]
    );
    // Tabs and new lines are fine everywhere.
    let mut document = base();
    print_block(&mut document)["comment"] = json!({"text": "a\tb\nc"});
    loads(&document);
}

#[test]
fn e0127_reserved_keys_outside_x_ext() {
    let mut document = base();
    print_block(&mut document)["__proto__"] = json!({"polluted": true});
    print_block(&mut document)["fields"]["constructor"] = json!("x");
    print_block(&mut document)["extra"]["deep"] = json!({"list": [{"prototype": 1}]});
    document["project"]["prototype"] = json!(1);
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 4, "{diagnostics:#?}");
    assert!(diagnostics.iter().all(|d| d.code.0 == "B2C-E0127"));
    // Also in single-key objects (tokens, define values) that are rejected
    // as a whole for having two keys.
    let mut document = base();
    print_block(&mut document)["inputs"]["ITEM0"]["expr"][0] = json!({"str": "hi", "__proto__": {"x": 1}});
    document["project"]["build"] = json!({"defines": [{"name": "A", "value": {"int": 1, "constructor": 2}}]});
    assert_eq!(
        codes(&document),
        ["B2C-E0112", "B2C-E0127", "B2C-E0112", "B2C-E0127"]
    );
    // Inside x-ext they are preserved, never interpreted.
    let mut document = base();
    document["x-ext"] = json!({"__proto__": {"constructor": {"prototype": 1}}});
    assert_eq!(loads(&document).ext, Some(document["x-ext"].clone()));
}

// --- E0128–E0135: positions, sizes and settings ---------------------------

#[test]
fn e0128_positions_only_on_canvas_blocks() {
    let mut document = base();
    print_block(&mut document)["x"] = json!(10);
    main_block(&mut document)["statements"]["BODY"][1]["inputs"]["VALUE"]["block"]["y"] = json!(10);
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 2);
    assert!(diagnostics.iter().all(|d| d.code.0 == "B2C-E0128"));
    assert_eq!(diagnostics[1].primary, block_location("b_num", Part::Whole));
}

#[test]
fn e0129_coordinates() {
    let mut document = base();
    main_block(&mut document)["x"] = json!(10_000_000);
    main_block(&mut document)["y"] = json!(-10_000_000);
    loads(&document);
    main_block(&mut document)["x"] = json!(10_000_001);
    main_block(&mut document)["y"] = json!(-2_147_483_649_i64);
    document["modules"][0]["workspace"]["frames"] = json!([{"id": "f1", "title": "T", "x": 0, "y": 0, "w": -1, "h": 18_446_744_073_709_551_615_u64, "color": "blue"}]);
    document["modules"][0]["workspace"]["notes"] =
        json!([{"id": "n1", "text": "n", "x": 0, "y": 99_999_999}]);
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 5);
    assert!(diagnostics.iter().all(|d| d.code.0 == "B2C-E0129"));
    assert_eq!(
        diagnostics[0].message,
        "\"x\" in this block is 10000001, but it must be between -10000000 and 10000000."
    );
}

#[test]
fn e0130_zoom() {
    // Integers count as numbers too (as with serde): 4 and 1 are fine.
    for (scale, ok) in [
        (json!(0.1), true),
        (json!(4), true),
        (json!(1), true),
        (json!(0.099), false),
        (json!(4.01), false),
        (json!(-1), false),
    ] {
        let mut document = base();
        document["modules"][0]["workspace"]["viewport"] = json!({"x": 0, "y": 0, "scale": scale});
        assert_eq!(load(&bytes(&document)).is_ok(), ok, "{scale}");
        if !ok {
            assert_eq!(codes(&document), ["B2C-E0130"]);
        }
    }
    let mut document = base();
    document["modules"][0]["workspace"]["viewport"] = json!({"x": 0, "y": 0, "scale": 1e308});
    assert_eq!(
        failure(&bytes(&document))[0].message,
        "\"modules[0].workspace.viewport.scale\" is 1e308, but the zoom must be between 0.1 and 4.0."
    );
}

#[test]
fn e0131_variadic_parts() {
    let mut document = base();
    print_block(&mut document)["extra"]["itemCount"] = json!(64);
    loads(&document);
    print_block(&mut document)["extra"]["itemCount"] = json!(65);
    print_block(&mut document)["extra"]["other"] = json!(1e300);
    let rows: Vec<Value> = (0..65)
        .map(|i| json!({"sym": format!("p{i}"), "name": format!("p{i}"), "type": "int", "mode": "copy"}))
        .collect();
    document["modules"][0]["workspace"]["blocks"][1]["extra"]["params"] = Value::Array(rows);
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 3);
    assert!(diagnostics.iter().all(|d| d.code.0 == "B2C-E0131"));
    assert_eq!(
        diagnostics[0].message,
        "\"extra.itemCount\" in this block is 65, but a block can have at most 64 parts."
    );
}

#[test]
fn e0132_and_e0134_defines() {
    let mut document = base();
    document["project"]["build"] = json!({"defines": [
        {"name": "GAME_VERSION", "value": {"int": 2}},
        {"name": "DEBUG_MODE", "value": {"bool": true}},
        {"name": "TITLE", "value": {"string": "My \"game\""}},
    ]});
    loads(&document);
    document["project"]["build"]["defines"] = json!([
        {"name": "int", "value": {"int": 1}},
        {"name": "abs", "value": {"int": 1}},
        {"name": "EOF", "value": {"int": 1}},
        {"name": "X -fplugin=evil.so", "value": {"int": 1}},
        {"name": "b2c_x", "value": {"int": 1}},
        {"name": "OK", "value": {"int": 1}},
        {"name": "OK", "value": {"int": 2}},
    ]);
    assert_eq!(
        codes(&document),
        [
            "B2C-E0132",
            "B2C-E0132",
            "B2C-E0132",
            "B2C-E0132",
            "B2C-E0132",
            "B2C-E0134"
        ]
    );
}

#[test]
fn e0133_library_names() {
    let mut document = base();
    document["project"]["build"] = json!({"libraries": ["sfml-graphics", "gtk+-3.0"]});
    loads(&document);
    document["project"]["build"] =
        json!({"libraries": ["", "a b", "-Wl,--wrap=x", "../lib", "x".repeat(65)]});
    assert_eq!(codes(&document), ["B2C-E0133"; 5]);
}

#[test]
fn e0136_and_e0137_packs() {
    let mut document = base();
    document["project"]["build"] =
        json!({"packs": [{"id": "std", "version": "^1.0"}, {"id": "sfml", "version": "*"}]});
    loads(&document);
    document["project"]["build"] = json!({"packs": [
        {"id": "../evil", "version": "^1.0"},
        {"id": "std", "version": "1.0; rm -rf ~"},
        {"id": "std", "version": "^1.0"},
        {"id": "sfml", "version": ""},
    ]});
    assert_eq!(
        codes(&document),
        ["B2C-E0136", "B2C-E0136", "B2C-E0136", "B2C-E0137"]
    );
    let diagnostics = failure(&bytes(&document));
    assert_eq!(
        diagnostics[0].message,
        "The library pack ID \"../evil\" is not valid: use 1 to 64 characters, a lower-case letter first, then lower-case letters, digits, _ or -."
    );
    assert_eq!(
        diagnostics[3].message,
        "The library pack \"std\" is listed more than once. Keep only one entry for it."
    );
}

#[test]
fn e0135_free_form_budget() {
    let mut document = base();
    document["x-ext"] = json!({ "list": Value::Array(vec![json!(0); 499_000]) });
    loads(&document);
    document["x-ext"] = json!({ "list": Value::Array(vec![json!(0); 500_001]) });
    assert_eq!(codes(&document), ["B2C-E0135"]);
}

#[test]
fn e0199_problem_list_is_capped() {
    let mut document = base();
    for i in 0..1500 {
        document["project"][format!("k{i}")] = json!(1);
    }
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 1001);
    let last = diagnostics.last().unwrap();
    assert_eq!(last.code.0, "B2C-E0199");
    assert!(last.message.starts_with("500 more problem(s) were found"));
}

#[test]
fn x_ext_is_preserved_as_is() {
    let mut document = base();
    let ext = json!({"b": [1, 2.5, -3, true, null, "text"], "a": {"nested": {}}});
    document["x-ext"] = ext.clone();
    assert_eq!(loads(&document).ext, Some(ext));
}

#[test]
fn x_ext_must_be_an_object() {
    // Spec §5.6: "x-ext" is an object of tooling metadata.
    for ext in [json!(5), json!("text"), json!([1, 2]), json!(true)] {
        let mut document = base();
        document["x-ext"] = ext;
        let diagnostics = failure(&bytes(&document));
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code.0, "B2C-E0112");
        assert!(
            diagnostics[0]
                .message
                .starts_with("\"x-ext\" should be an object, but it is")
        );
    }
}
