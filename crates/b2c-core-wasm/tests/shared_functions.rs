//! Other modules' shared functions in a paste (docs/plans/m3-contracts/
//! w3-modules.md §2.1, §5.2 F): a pasted call binds by name to the target
//! module's own function when it has one, and otherwise to the function
//! another module shares, as the scope query lists them.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests fail by panicking"
)]

mod common;

use common::parse;
use serde_json::{Value, json};

/// A project of these modules (`(id, name, blocks)`).
fn project(modules: &[(&str, &str, Value)]) -> String {
    let modules: Vec<Value> = modules
        .iter()
        .map(|(id, name, blocks)| json!({"id": id, "name": name, "workspace": {"blocks": blocks}}))
        .collect();
    json!({
        "format": "blocks2cpp/project", "formatVersion": 1,
        "generator": {"app": "0.1.0", "catalog": "1.3.0"},
        "project": {"id": "prj_paste", "name": "Paste", "language": {"standard": "c++20"}},
        "modules": modules
    })
    .to_string()
}

/// `define area` returning `int`, declaring `sym`, ticked to share or not.
fn area(id: &str, sym: &str, shared: bool) -> Value {
    json!({"id": id, "type": "func.define", "v": 1,
           "fields": {"NAME": {"sym": sym, "name": "area"}, "RETURNS": "int", "SHARED": shared},
           "extra": {"params": []},
           "statements": {"BODY": [{"id": format!("{id}_r"), "type": "func.return", "v": 1,
               "inputs": {"VALUE": {"expr": [{"num": "1"}]}}}]}})
}

/// `print area()` with this block ID, calling the function `sym`.
fn print_area(id: &str, sym: &str) -> Value {
    json!({"id": id, "type": "io.print", "v": 1, "extra": {"itemCount": 1},
           "fields": {"NEWLINE": true, "SEP": "none", "STREAM": "out"},
           "inputs": {"ITEM0": {"expr": [{"ref": sym}, {"op": "("}, {"op": ")"}]}}})
}

/// The symbol references of pasted blocks.
fn refs(blocks: &Value) -> Vec<String> {
    let mut found = Vec::new();
    let mut stack = vec![blocks];
    while let Some(value) = stack.pop() {
        match value {
            Value::Object(map) => {
                if let Some(Value::String(sym)) = map.get("ref") {
                    found.push(sym.clone());
                }
                stack.extend(map.values());
            }
            Value::Array(items) => stack.extend(items),
            _ => {}
        }
    }
    found
}

#[test]
fn a_pasted_call_binds_to_the_module_s_own_function_before_another_module_s_shared_one() {
    // The copy: a call to `area` in a project of one module.
    let source = project(&[(
        "mod_src",
        "main",
        json!([
            {"id": "src_main", "type": "program.main", "v": 1, "statements": {"BODY": [print_area("src_print", "s_src_area")]}},
            area("src_area", "s_src_area", false)
        ]),
    )]);
    let made = parse(&b2c_core_wasm::clipboard_make(&source, r#"["src_print"]"#));
    assert_eq!(made["ok"], true, "{made}");
    let payload = made["payload"].as_str().unwrap();
    // The target: `main` has an `area` of its own (which is `B2C-E0213`
    // there), `geometry` shares one, `stats` has none.
    let target = project(&[
        (
            "mod_main",
            "main",
            json!([
                {"id": "t_main", "type": "program.main", "v": 1, "statements": {"BODY": [print_area("t_print", "s_area_main")]}},
                area("t_area_main", "s_area_main", false)
            ]),
        ),
        (
            "mod_geometry",
            "geometry",
            json!([area("t_area_geo", "s_area_geo", true)]),
        ),
        (
            "mod_stats",
            "stats",
            json!([{"id": "t_report", "type": "func.define", "v": 1,
                    "fields": {"NAME": {"sym": "s_report", "name": "report"}, "RETURNS": "void"},
                    "extra": {"params": []},
                    "statements": {"BODY": [{"id": "t_stop", "type": "func.return", "v": 1}]}}]),
        ),
    ]);
    let paste = |module: &str, block: &str| -> Value {
        let at = json!({"module": module, "block": block, "input": null}).to_string();
        let pasted = parse(&b2c_core_wasm::paste_prepare(
            payload,
            &target,
            &at,
            &format!("{:064x}", 7),
        ));
        assert_eq!(pasted["ok"], true, "{pasted}");
        assert_eq!(pasted["unresolved"], json!([]), "{pasted}");
        pasted
    };
    assert_eq!(refs(&paste("mod_main", "t_print")["blocks"]), ["s_area_main"]);
    assert_eq!(refs(&paste("mod_stats", "t_stop")["blocks"]), ["s_area_geo"]);
}
