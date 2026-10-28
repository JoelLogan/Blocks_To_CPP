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

use b2c_ir::types::TypeTerm;
use b2c_ir::{BlockId, DiagSource, Location, ModuleId, Part, Severity, SymbolId};
use b2c_model::limits::{MAX_BLOCKS, MAX_EXPR_TOKENS, MAX_FILE_BYTES, MAX_MODULES, MAX_STRING_BYTES};
use b2c_model::{Document, FieldValue, Input, SymbolRef, Token, load};
use common::{base, bytes, codes, failure, main_block, print_block};
use serde_json::{Value, json};

fn block_location(block: &str, part: Part) -> Location {
    Location {
        module: Some(ModuleId::new("mod_main").unwrap()),
        block: Some(BlockId::new(block).unwrap()),
        part,
    }
}

/// This app's version as major, minor and patch, without any pre-release or
/// build part.
fn this_app_version() -> [u64; 3] {
    let version = env!("CARGO_PKG_VERSION");
    let core = version.split(['-', '+']).next().unwrap();
    let numbers: Vec<u64> = core.split('.').map(|n| n.parse().unwrap()).collect();
    numbers.try_into().unwrap()
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
    document["formatVersion"] = json!(3);
    document["unknownFutureKey"] = json!(true);
    assert_eq!(codes(&document), ["B2C-E0108"]);
    // Version 2 is read: an unknown key in it is just unknown.
    document["formatVersion"] = json!(2);
    assert_eq!(codes(&document), ["B2C-E0110"]);
    let mut document = base();
    document["formatVersion"] = json!(0);
    assert_eq!(codes(&document), ["B2C-E0109"]);
}

#[test]
fn e0108_unknown_keys_in_a_file_from_a_newer_app() {
    // ADR-0013: a key this version does not know, in a file that a newer
    // Blocks2Cpp saved, is a key that version added: the file is "made with
    // a newer version", not misspelt.
    let mut document = base();
    document["generator"]["app"] = json!("9.0.0");
    document["project"]["lints"] = json!({"W0505": "error"});
    document["project"]["options"] = json!({"wrapLongLines": true});
    print_block(&mut document)["breakpoint"] = json!(true);
    let diagnostics = failure(&bytes(&document));
    let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
    let newer = "which this version of Blocks2Cpp does not know. The project was made with a newer version of \
                 Blocks2Cpp; update Blocks2Cpp to open it.";
    assert_eq!(
        messages,
        [
            format!("\"project\" has the key \"lints\", {newer}"),
            format!("\"project.options\" has the key \"wrapLongLines\", {newer}"),
            format!("This block has the key \"breakpoint\", {newer}"),
        ]
    );
    assert!(diagnostics.iter().all(|d| d.code.0 == "B2C-E0108"));
    assert_eq!(diagnostics[2].primary, block_location("b_print", Part::Whole));
    // The messages never name a version taken from the file (nor any other):
    // the only digit in them is the one in the app's name.
    for diagnostic in &diagnostics {
        assert!(
            !diagnostic
                .message
                .replace("Blocks2Cpp", "")
                .contains(|c: char| c.is_ascii_digit()),
            "{}",
            diagnostic.message
        );
    }
    // Any newer SemVer counts, however large; a newer pre-release too, and
    // build metadata does not hide a newer version. The cases are derived
    // from this app's version, so they stay newer when it is raised.
    let [major, minor, patch] = this_app_version();
    let next_patch = format!("{major}.{minor}.{}", patch + 1);
    for app in [
        String::from("99999999999999999999999.0.0"),
        format!("{next_patch}-alpha.1"),
        format!("{next_patch}+build.7"),
        format!("{}.0.0-alpha.1", major + 1),
        format!("{major}.{}.0+build.7", minor + 1),
    ] {
        let app = app.as_str();
        let mut document = base();
        document["generator"]["app"] = json!(app);
        document["project"]["lints"] = json!({});
        let diagnostics = failure(&bytes(&document));
        assert_eq!(diagnostics.len(), 1, "{app}");
        assert_eq!(diagnostics[0].code.0, "B2C-E0108", "{app}");
        assert!(!diagnostics[0].message.contains(app), "{app}");
    }
    // Other problems in such a file keep their own codes.
    let mut document = base();
    document["generator"]["app"] = json!("9.0.0");
    document["project"]["lints"] = json!({});
    document["modules"][0]["name"] = json!("Main");
    assert_eq!(codes(&document), ["B2C-E0108", "B2C-E0117"]);
}

#[test]
fn e0110_unknown_keys_in_a_file_from_this_or_an_older_app() {
    let this_app = env!("CARGO_PKG_VERSION");
    let prerelease = format!("{this_app}-beta.2");
    // The same or an older version, a pre-release of this one, and text
    // that is not a SemVer version: the key is simply unknown.
    for app in [
        this_app,
        "0.0.0",
        prerelease.as_str(),
        "9",
        "9.0",
        "v9.0.0",
        "09.0.0",
        "9.0.0-",
        "9.0.0-01",
        "9.0.0+",
        "9.0.0.0",
        "latest",
        "",
    ] {
        let mut document = base();
        document["generator"]["app"] = json!(app);
        document["project"]["lints"] = json!({});
        let diagnostics = failure(&bytes(&document));
        assert_eq!(diagnostics.len(), 1, "{app:?}");
        assert_eq!(diagnostics[0].code.0, "B2C-E0110", "{app:?}");
        assert_eq!(
            diagnostics[0].message,
            "\"project\" has an unknown key \"lints\". Remove it or check its spelling.",
            "{app:?}"
        );
    }
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
fn e0112_numbers_javascript_cannot_hold() {
    // Define values and free-form numbers stay within ±(2^53 − 1), which
    // the editor's JavaScript numbers hold exactly (spec §5.6).
    let mut document = base();
    document["project"]["build"] = json!({"defines": [
        {"name": "MAX", "value": {"int": 9_007_199_254_740_991_i64}},
        {"name": "MIN", "value": {"int": -9_007_199_254_740_991_i64}},
    ]});
    document["x-ext"] = json!({"max": 9_007_199_254_740_991_i64, "min": -9_007_199_254_740_991_i64});
    let loaded = loads(&document);
    assert_eq!(
        loaded.project.build.defines[0].value,
        b2c_model::DefineValue::Int(b2c_model::limits::MAX_SAFE_INTEGER)
    );

    let mut document = base();
    document["project"]["build"] = json!({"defines": [
        {"name": "BIG", "value": {"int": 9_007_199_254_740_993_i64}},
        {"name": "SMALL", "value": {"int": -9_007_199_254_740_992_i64}},
    ]});
    document["x-ext"] = json!({
        "a": 9_007_199_254_740_992_i64,
        "b": 12_345_678_901_234_567_890_u64,
        "c": [1, {"d": -1e16}],
        "e": 1e300,
    });
    print_block(&mut document)["extra"]["other"] = json!([1e20]);
    let diagnostics = failure(&bytes(&document));
    let messages: Vec<(&str, &str)> = diagnostics
        .iter()
        .map(|d| (d.code.0.as_str(), d.message.as_str()))
        .collect();
    let free = "but numbers in free-form data must be between -9007199254740991 and 9007199254740991, which the editor can hold exactly.";
    assert_eq!(
        messages,
        [
            (
                "B2C-E0112",
                "\"project.build.defines[0].value.int\" should be a whole number from -9007199254740991 to 9007199254740991, but it is the number 9007199254740993."
            ),
            (
                "B2C-E0112",
                "\"project.build.defines[1].value.int\" should be a whole number from -9007199254740991 to 9007199254740991, but it is the number -9007199254740992."
            ),
            (
                "B2C-E0112",
                &*format!("\"extra.other[0]\" in this block is the number 1e+20, {free}")
            ),
            (
                "B2C-E0112",
                &*format!("\"x-ext.a\" is the number 9007199254740992, {free}")
            ),
            (
                "B2C-E0112",
                &*format!("\"x-ext.b\" is the number 12345678901234567890, {free}")
            ),
            (
                "B2C-E0112",
                &*format!("\"x-ext.c[1].d\" is the number -1e+16, {free}")
            ),
            ("B2C-E0112", &*format!("\"x-ext.e\" is the number 1e+300, {free}")),
        ]
    );
}

#[test]
fn whole_free_form_numbers_are_stored_as_integers() {
    // As JavaScript reads them: `1.0` is the number 1 (spec §5.6), so a
    // save writes it `1`, and the editor's copy saves to the same text.
    let mut document = base();
    document["x-ext"] = json!({"one": 1.0, "zero": -0.0, "big": 2e15, "half": 0.5, "tiny": 1.5e-6});
    print_block(&mut document)["extra"]["scale"] = json!(64.0);
    let loaded = loads(&document);
    let ext = loaded.ext.as_ref().unwrap();
    assert!(ext["one"].is_u64() && ext["zero"].is_u64() && ext["big"].is_u64());
    assert_eq!(ext["one"], json!(1));
    assert_eq!(ext["zero"], json!(0));
    assert_eq!(ext["big"], json!(2_000_000_000_000_000_u64));
    assert!(ext["half"].is_f64());
    let print = &loaded.modules[0].workspace.blocks[0].statements["BODY"][0];
    assert_eq!(print.extra["scale"], json!(64));
    let text = b2c_model::to_canonical_json(&loaded);
    for line in [
        "\"big\": 2000000000000000,",
        "\"half\": 0.5,",
        "\"one\": 1,",
        "\"tiny\": 0.0000015,",
        "\"zero\": 0",
        "\"scale\": 64",
    ] {
        assert!(text.contains(line), "{line}\n{text}");
    }
    let again = load(text.as_bytes()).unwrap();
    assert_eq!(again.ext, loaded.ext);
    assert_eq!(b2c_model::to_canonical_json(&again), text);
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
    let body: Vec<Value> = (0..MAX_BLOCKS - 2).map(block).collect();
    main_block(&mut document)["statements"]["BODY"] = Value::Array(body);
    // main + function + the body = exactly MAX_BLOCKS: still fine.
    loads(&document);
    // More, nested in the function, are too many; they are reported once.
    let more: Vec<Value> = (0..4).map(|i| block(MAX_BLOCKS + i)).collect();
    document["modules"][0]["workspace"]["blocks"][1]["statements"] = json!({"BODY": more});
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code.0, "B2C-E0121");
    assert_eq!(
        diagnostics[0].message,
        "The project has more than 100000 blocks, which is the most a project can have. Split it into smaller projects."
    );
    assert_eq!(diagnostics[0].primary, Location::project());
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

// --- Type terms (types.md §2): the readings of an object, E0140 ------------

/// The `var.declare` block of [`base`], whose `TYPE` field holds a type.
fn declare_block(document: &mut Value) -> &mut Value {
    &mut main_block(document)["statements"]["BODY"][1]
}

/// The `type` of the parameter row of [`base`]'s function.
fn param_type(document: &mut Value) -> &mut Value {
    &mut document["modules"][0]["workspace"]["blocks"][1]["extra"]["params"][0]["type"]
}

/// `levels` `std::vector`s nested around `int`.
fn nested(levels: usize) -> Value {
    let mut term = json!("int");
    for _ in 0..levels {
        term = json!({ "std::vector": term });
    }
    term
}

/// A template with `count` arguments: `count + 1` names.
fn wide(count: usize) -> Value {
    json!({ "lib::tuple": (0..count).map(|i| format!("t{i}")).collect::<Vec<_>>() })
}

/// The codes of a document whose `TYPE` field holds `term`.
fn type_field_codes(term: Value) -> Vec<String> {
    let mut document = base();
    declare_block(&mut document)["fields"]["TYPE"] = term;
    codes(&document)
}

/// The codes of a document whose parameter row's `type` holds `term`.
fn row_codes(term: Value) -> Vec<String> {
    let mut document = base();
    *param_type(&mut document) = term;
    codes(&document)
}

fn sym(id: &str) -> SymbolId {
    SymbolId::new(id).unwrap()
}

fn name(text: &str) -> TypeTerm {
    TypeTerm::name(text).unwrap()
}

#[test]
fn objects_in_fields_are_read_three_ways() {
    let declared = |document: &Document| {
        document.modules[0].workspace.blocks[0].statements["BODY"][1].fields["TYPE"].clone()
    };
    // 1. With the key `ref`: a reference; in a type field, a user type.
    let mut document = base();
    declare_block(&mut document)["fields"]["TYPE"] = json!({"ref": "s_point"});
    assert_eq!(
        declared(&loads(&document)),
        FieldValue::Ref(SymbolRef {
            target: sym("s_point")
        })
    );
    // 2. Exactly one key, a qualified name: a template type term.
    declare_block(&mut document)["fields"]["TYPE"] =
        json!({"std::map": ["std::string", {"std::vector": {"ref": "s_point"}}]});
    let list = TypeTerm::app("std::vector", vec![TypeTerm::Ref(sym("s_point"))]).unwrap();
    let map = TypeTerm::app("std::map", vec![name("std::string"), list]).unwrap();
    assert_eq!(declared(&loads(&document)), FieldValue::Type(map));
    // Names are only checked against the name rule here: the catalog
    // decides which are types.
    declare_block(&mut document)["fields"]["TYPE"] = json!({"my lib::thing": ["auto", "void", "long long"]});
    assert!(matches!(
        declared(&loads(&document)),
        FieldValue::Type(TypeTerm::App(..))
    ));
    // 3. Anything else: a declaration, so a damaged declaration keeps its
    // own problems instead of becoming a template called `sm`.
    let mut document = base();
    declare_block(&mut document)["fields"]["NAME"] = json!({"sm": "s_x", "name": "x"});
    let diagnostics = failure(&bytes(&document));
    let messages: Vec<(&str, &str)> = diagnostics
        .iter()
        .map(|d| (d.code.0.as_str(), d.message.as_str()))
        .collect();
    assert_eq!(
        messages,
        [
            (
                "B2C-E0110",
                "\"fields.NAME\" in this block has an unknown key \"sm\". Remove it or check its spelling."
            ),
            ("B2C-E0111", "\"fields.NAME\" in this block is missing \"sym\"."),
        ]
    );
    // An unqualified key, or two qualified ones, make no template either.
    for object in [
        json!({"vector": "int"}),
        json!({"std::vector": "int", "std::set": "int"}),
        json!({"std:vector": "int"}),
        json!({"::vector": "int"}),
    ] {
        let found = type_field_codes(object.clone());
        assert!(found.contains(&"B2C-E0110".to_owned()), "{object}: {found:?}");
        assert!(found.contains(&"B2C-E0111".to_owned()), "{object}: {found:?}");
    }
    // From a newer app, the unknown key is one that app added.
    let mut document = base();
    document["generator"]["app"] = json!("9.0.0");
    declare_block(&mut document)["fields"]["NAME"] = json!({"sm": "s_x", "name": "x"});
    assert_eq!(codes(&document), ["B2C-E0108", "B2C-E0111"]);
}

#[test]
fn e0140_type_terms_too_large() {
    // 8 levels and 32 names fit; 9 levels and 33 names do not.
    let mut document = base();
    declare_block(&mut document)["fields"]["TYPE"] = nested(8);
    loads(&document);
    declare_block(&mut document)["fields"]["TYPE"] = wide(31);
    loads(&document);
    *param_type(&mut document) = nested(8);
    loads(&document);
    declare_block(&mut document)["fields"]["TYPE"] = nested(9);
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code.0, "B2C-E0140");
    assert_eq!(
        diagnostics[0].message,
        "\"fields.TYPE\" in this block is a type with more than 8 levels or more than 32 names, more than a type can have."
    );
    assert_eq!(
        diagnostics[0].primary,
        block_location("b_var", Part::Field { name: "TYPE".into() })
    );
    assert_eq!(type_field_codes(wide(32)), ["B2C-E0140"]);
    // References count as names.
    let refs = vec![json!({"ref": "s_point"}); 32];
    assert_eq!(type_field_codes(json!({"lib::tuple": refs})), ["B2C-E0140"]);
    // One problem, however broken the rest of the term is.
    let mut broken = json!([5, null, ["x"], {"__proto__": 1}, "a\u{202e}b"]);
    for _ in 0..12 {
        broken = json!({ "std::vector": broken });
    }
    assert_eq!(type_field_codes(broken), ["B2C-E0140"]);
    // In a parameter row too.
    let mut document = base();
    *param_type(&mut document) = wide(40);
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].message,
        "\"extra.params[0].type\" in this block is a type with more than 8 levels or more than 32 names, more than a type can have."
    );
    assert_eq!(diagnostics[0].primary, block_location("b_fn", Part::Whole));
}

#[test]
fn e0112_type_term_shapes() {
    let mut document = base();
    declare_block(&mut document)["fields"]["TYPE"] = json!({"std::vector": ["int"]});
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(
        diagnostics[0].message,
        "\"fields.TYPE[\"std::vector\"]\" in this block is a list of one argument. Write a template's only argument without the list, as in {\"std::vector\": \"int\"}."
    );
    assert_eq!(
        diagnostics[0].primary,
        block_location("b_var", Part::Field { name: "TYPE".into() })
    );
    let message = |term: Value| {
        let mut document = base();
        declare_block(&mut document)["fields"]["TYPE"] = term;
        let diagnostics = failure(&bytes(&document));
        assert_eq!(diagnostics.len(), 1, "{diagnostics:#?}");
        assert_eq!(diagnostics[0].code.0, "B2C-E0112");
        diagnostics[0].message.clone()
    };
    assert_eq!(
        message(json!({"std::vector": []})),
        "\"fields.TYPE[\"std::vector\"]\" in this block is an empty list, but a template needs its arguments: one written as it is, or two or more in a list."
    );
    assert_eq!(
        message(json!({"std::map": ["int", 5]})),
        "\"fields.TYPE[\"std::map\"][1]\" in this block should be a type: a name such as \"int\", a user type {\"ref\": …} or a template such as {\"std::vector\": \"int\"}, but it is the number 5."
    );
    assert_eq!(
        message(json!({"std::vector": "in t!"})),
        "\"fields.TYPE[\"std::vector\"]\" in this block is not a type name: \"in t!\" is not 1 to 64 letters, digits and _ in names joined by :: or single spaces, each name starting with a letter or _."
    );
    for term in [
        json!({"std::vector": null}),
        json!({"std::vector": true}),
        json!({"std::vector": 1.5}),
        json!({"std::map": ["int", ["int", "int"]]}),
        json!({"std::vector": {"vector": "int"}}),
        json!({"std::vector": {"a": "b", "c": "d"}}),
        json!({"std::vector": {}}),
        json!({"std::vector": ""}),
        json!({"std::vector": "a  b"}),
        json!({"std::vector": " int"}),
        json!({"std::vector": "1st"}),
        json!({"std::vector": "x".repeat(65)}),
        json!({"std::vector": {"std::set": ["int"]}}),
    ] {
        assert_eq!(type_field_codes(term.clone()), ["B2C-E0112"], "{term}");
    }
    // Every argument is checked.
    assert_eq!(
        type_field_codes(json!({"std::map": [5, "x y!", "int"]})),
        ["B2C-E0112", "B2C-E0112"]
    );
    // 64 characters is the longest name; two arguments make a list.
    let mut document = base();
    declare_block(&mut document)["fields"]["TYPE"] = json!({"std::vector": "x".repeat(64)});
    loads(&document);
    declare_block(&mut document)["fields"]["TYPE"] = json!({"std::map": ["int", "int"]});
    loads(&document);
}

#[test]
fn ids_reserved_keys_and_text_rules_in_type_terms() {
    assert_eq!(
        type_field_codes(json!({"std::vector": {"ref": "s\"><img>"}})),
        ["B2C-E0113"]
    );
    assert_eq!(
        type_field_codes(json!({"std::vector": {"ref": "s_point", "x": 1}})),
        ["B2C-E0110"]
    );
    assert_eq!(
        type_field_codes(json!({"std::vector": {"ref": 5}})),
        ["B2C-E0112"]
    );
    assert_eq!(
        type_field_codes(json!({"std::vector": {"__proto__": "int"}})),
        ["B2C-E0127"]
    );
    assert_eq!(
        type_field_codes(json!({"std::vector": {"a": 1, "constructor": 2}})),
        ["B2C-E0112", "B2C-E0127"]
    );
    assert_eq!(
        type_field_codes(json!({"std::vector": {"ref": "s", "prototype": 2}})),
        ["B2C-E0127"]
    );
    assert_eq!(
        type_field_codes(json!({"std::vector": "in\u{202e}t"})),
        ["B2C-E0126"]
    );
    assert_eq!(
        type_field_codes(json!({"std::vector": "int\u{0}"})),
        ["B2C-E0124"]
    );
    assert_eq!(
        type_field_codes(json!({"std::vector": "x".repeat(MAX_STRING_BYTES + 1)})),
        ["B2C-E0123"]
    );
    // A template name with a hidden character is no qualified name: the
    // object is a damaged declaration, and the message shows the character.
    let mut document = base();
    declare_block(&mut document)["fields"]["TYPE"] = json!({"std::vec\u{202e}tor": "int"});
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics[0].code.0, "B2C-E0110");
    assert!(
        diagnostics[0].message.contains("\\u{202E}"),
        "{}",
        diagnostics[0].message
    );
    // From a newer app, an unknown key in a reference is one it added.
    let mut document = base();
    document["generator"]["app"] = json!("9.0.0");
    declare_block(&mut document)["fields"]["TYPE"] = json!({"std::vector": {"ref": "s_point", "x": 1}});
    assert_eq!(codes(&document), ["B2C-E0108"]);
}

#[test]
fn parameter_row_types_are_checked_like_type_fields() {
    // Names, references and templates load; the row stays free-form data.
    let mut document = base();
    for term in [
        json!("int"),
        json!({"ref": "s_point"}),
        json!({"std::map": ["int", {"std::vector": {"ref": "s_point"}}]}),
    ] {
        *param_type(&mut document) = term.clone();
        let loaded = loads(&document);
        assert_eq!(
            loaded.modules[0].workspace.blocks[1].extra["params"][0]["type"],
            term
        );
    }
    assert_eq!(row_codes(json!({"std::vector": ["int"]})), ["B2C-E0112"]);
    assert_eq!(row_codes(json!({"std::vector": "in t!"})), ["B2C-E0112"]);
    assert_eq!(
        row_codes(json!({"std::vector": {"ref": "bad id"}})),
        ["B2C-E0113"]
    );
    assert_eq!(row_codes(json!({"ref": "bad id"})), ["B2C-E0113"]);
    assert_eq!(row_codes(json!({"ref": "s", "x": 1})), ["B2C-E0110"]);
    assert_eq!(row_codes(nested(9)), ["B2C-E0140"]);
    // Text rules and reserved keys are reported once, by the free-form
    // checks of `extra`.
    assert_eq!(row_codes(json!({"std::vector": "in\u{202e}t"})), ["B2C-E0126"]);
    assert_eq!(
        row_codes(json!({"std::vector": {"__proto__": "int"}})),
        ["B2C-E0127"]
    );
    assert_eq!(row_codes(json!({"ref": "s", "constructor": 1})), ["B2C-E0127"]);
    assert_eq!(
        row_codes(json!({"std::vector": {"a": 1, "__proto__": 2}})),
        ["B2C-E0112", "B2C-E0127"]
    );
    // A long name breaks the name rule, or the length limit, never both.
    assert_eq!(
        row_codes(json!({"std::vector": "x".repeat(MAX_STRING_BYTES)})),
        ["B2C-E0112"]
    );
    assert_eq!(
        row_codes(json!({"std::vector": "x".repeat(MAX_STRING_BYTES + 1)})),
        ["B2C-E0123"]
    );
    // Anything else is left to the catalog, which checks parameter rows, as
    // before type terms.
    for term in [
        json!({"vector": "int"}),
        json!({"sym": "x", "name": "y"}),
        json!(5),
        json!(["int"]),
        json!("in t!"),
    ] {
        *param_type(&mut document) = term;
        loads(&document);
    }
    // Rows of other keys are not type terms.
    let mut document = base();
    document["modules"][0]["workspace"]["blocks"][1]["extra"]["other"] =
        json!([{"type": {"std::vector": []}}]);
    loads(&document);
    // Rows that are not objects, and rows without a type, are left alone.
    document["modules"][0]["workspace"]["blocks"][1]["extra"]["params"] =
        json!([{"sym": "s_n", "name": "n"}, "row", 5]);
    loads(&document);
    document["modules"][0]["workspace"]["blocks"][1]["extra"]["params"] =
        json!({"type": {"std::vector": []}});
    loads(&document);
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
fn e0131_part_limits_are_exact() {
    // At most 64 parts, whether "extra" holds a count (whole, or a number
    // written with a fraction) or a list of rows.
    let mut document = base();
    print_block(&mut document)["extra"]["itemCount"] = json!(64);
    print_block(&mut document)["extra"]["rows"] = json!(vec![0; 64]);
    print_block(&mut document)["extra"]["scale"] = json!(64.0);
    loads(&document);
    print_block(&mut document)["extra"]["rows"] = json!(vec![0; 65]);
    print_block(&mut document)["extra"]["scale"] = json!(64.5);
    let diagnostics = failure(&bytes(&document));
    let found: Vec<(&str, &str)> = diagnostics
        .iter()
        .map(|d| (d.code.0.as_str(), d.message.as_str()))
        .collect();
    assert_eq!(
        found,
        [
            (
                "B2C-E0131",
                "\"extra.rows\" in this block has 65 entries, but a block can have at most 64."
            ),
            (
                "B2C-E0131",
                "\"extra.scale\" in this block is 64.5, but a block can have at most 64 parts."
            ),
        ]
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
    // The documented budget: at most 500,000 values in all "extra" maps and
    // "x-ext" together. Without the blocks' own "extra" data, "x-ext" holds
    // them all: the object, the list and the numbers in it.
    const BUDGET: usize = 500_000;
    let mut document = base();
    print_block(&mut document)
        .as_object_mut()
        .unwrap()
        .remove("extra");
    document["modules"][0]["workspace"]["blocks"][1]
        .as_object_mut()
        .unwrap()
        .remove("extra");
    document["x-ext"] = json!({ "list": vec![0; BUDGET - 2] });
    loads(&document);
    document["x-ext"] = json!({ "list": vec![0; BUDGET - 1] });
    let diagnostics = failure(&bytes(&document));
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code.0, "B2C-E0135");
    assert_eq!(diagnostics[0].primary, Location::project());
    // The blocks' "extra" values count toward the same budget.
    let mut document = base();
    document["x-ext"] = json!({ "list": vec![0; BUDGET - 2] });
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
