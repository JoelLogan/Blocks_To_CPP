//! Shared helpers for the facade tests. They call the exports exactly as the
//! editor does (strings in, compact JSON out) and parse the JSON.

#![allow(dead_code, reason = "each test crate uses a different subset")]

use std::path::{Path, PathBuf};

use serde_json::Value;

/// The repository root.
pub(crate) fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// One example project.
pub(crate) struct Example {
    /// The file stem, e.g. `hello_world`.
    pub(crate) name: String,
    /// The file text.
    pub(crate) text: String,
}

/// Every `examples/*.b2c`, sorted by name.
pub(crate) fn examples() -> Vec<Example> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(repo_root().join("examples"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "b2c"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 15, "examples are missing");
    paths
        .into_iter()
        .map(|path| Example {
            name: path.file_stem().unwrap().to_str().unwrap().to_owned(),
            text: std::fs::read_to_string(&path).unwrap(),
        })
        .collect()
}

/// Parses an export's output, checking that it is compact JSON (no line
/// breaks; strings escape theirs) and an object.
pub(crate) fn parse(output: &str) -> Value {
    assert!(!output.contains('\n'), "the output is not compact: {output:.200}");
    let value: Value =
        serde_json::from_str(output).unwrap_or_else(|e| panic!("not JSON ({e}): {output:.200}"));
    assert!(value.is_object(), "not an object: {output:.200}");
    value
}

/// `preview` with the given indent width.
pub(crate) fn preview(document: &str, indent_width: u8) -> Value {
    parse(&b2c_core_wasm::preview(
        document,
        &format!(r#"{{"indentWidth":{indent_width}}}"#),
    ))
}

/// The codes of a list of diagnostics, in order.
pub(crate) fn codes(diagnostics: &Value) -> Vec<String> {
    diagnostics
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap().to_owned())
        .collect()
}

/// Whether any diagnostic in the list is an error.
pub(crate) fn has_errors(diagnostics: &Value) -> bool {
    diagnostics
        .as_array()
        .unwrap()
        .iter()
        .any(|d| d["severity"] == "error")
}

/// The contents of a generated file, by path.
pub(crate) fn file<'a>(preview: &'a Value, path: &str) -> Option<&'a str> {
    preview["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == path)
        .map(|f| f["contents"].as_str().unwrap())
}

/// The analysis composed directly from the crates (load, resolve,
/// analyse), for comparing with the facade.
pub(crate) fn native_analysis(text: &str) -> b2c_lang::Analysis {
    let loaded = b2c_model::load(text.as_bytes()).unwrap();
    let (document, _) = b2c_catalog::resolve(&loaded, b2c_catalog::core_catalog());
    b2c_lang::analyze(&document)
}

/// An example by file stem.
pub(crate) fn example(name: &str) -> String {
    std::fs::read_to_string(repo_root().join("examples").join(format!("{name}.b2c"))).unwrap()
}

/// The names in a JSON list of symbols, in order.
pub(crate) fn names(symbols: &Value) -> Vec<String> {
    symbols
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["name"].as_str().unwrap().to_owned())
        .collect()
}

/// The front half of the pipeline composed directly from the crates (load,
/// resolve, analyse, generate), for comparing with the facade.
pub(crate) fn native_generation(text: &str, indent_width: u8) -> b2c_codegen::Generation {
    let loaded = b2c_model::load(text.as_bytes()).unwrap();
    let (document, _) = b2c_catalog::resolve(&loaded, b2c_catalog::core_catalog());
    let analysis = b2c_lang::analyze(&document);
    let options = b2c_codegen::CodegenOptions {
        project_name: document.project.name.clone(),
        app_version: String::from(env!("CARGO_PKG_VERSION")),
        do_not_edit_banner: true,
        indent_width,
        helper_placement: b2c_codegen::HelperPlacement::Inline,
    };
    b2c_codegen::generate_with_report(&analysis.program, &options)
}

/// Characters that must never reach generated text or a message raw:
/// controls other than line feed, bidirectional controls and invisible
/// format characters (spec 08 §8.4).
pub(crate) fn is_unsafe_to_show(c: char) -> bool {
    (c.is_control() && c != '\n')
        || b2c_ir::text::is_invisible(c)
        || matches!(c,
            '\u{00AD}' | '\u{061C}' | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}')
}
