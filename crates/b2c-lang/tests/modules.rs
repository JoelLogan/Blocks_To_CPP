//! The analyser's part of W3's contracts step for modules
//! (`docs/plans/m3-contracts/w3-modules.md` §3.1): `B2C-E0221`, the shared
//! flag of items and symbols, and the layout every program gets until the
//! packages that fill headers arrive. Functions across modules are tested
//! in `modules_functions.rs` (package F), types and globals in
//! `modules_types.rs` (package T).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::print_stderr)]

mod common;

use b2c_ir::Severity;
use b2c_ir::sast::{ItemKind, Program};
use common::{analyze, assert_clean, call, call_stmt, doc, doc_modules, func, main, ret, sym};
use serde_json::{Value, json};

const RESERVED: &str =
    "Module names that start with `b2c` are kept for files that Blocks2Cpp writes. Rename the module.";

/// `define <name>` with *share with other modules* ticked or not.
fn define(name: &str, shared: bool) -> Value {
    let mut block = func(name, "void", &[], vec![]);
    block["fields"]["SHARED"] = json!(shared);
    block
}

/// `global variable [int] [name] = 0`, ticked or not.
fn global(name: &str, shared: bool) -> Value {
    json!({
        "id": format!("g_{name}"), "type": "var.global", "v": 1,
        "fields": {"KIND": "variable", "TYPE": "int", "NAME": {"sym": sym(name), "name": name},
                   "SHARED": shared},
        "inputs": {"VALUE": {"expr": [{"num": "0"}]}}
    })
}

/// `struct [name]` with no fields, ticked or not.
fn structure(name: &str, shared: bool) -> Value {
    json!({
        "id": format!("s_{name}"), "type": "type.struct", "v": 1,
        "fields": {"NAME": {"sym": sym(name), "name": name}, "SHARED": shared},
        "extra": {"fields": []}
    })
}

/// The names of a program's shared items, module by module.
fn shared_items(program: &Program) -> Vec<Vec<String>> {
    program
        .modules
        .iter()
        .map(|module| {
            module
                .items
                .iter()
                .filter(|item| item.shared)
                .map(|item| {
                    let symbol = match &item.kind {
                        ItemKind::Function(def) => &def.symbol,
                        ItemKind::Struct(def) => &def.symbol,
                        ItemKind::Enum(def) => &def.symbol,
                        ItemKind::Global(def) => &def.symbol,
                        ItemKind::Main(_) | ItemKind::UseHeader { .. } => unreachable!("never shared"),
                    };
                    program
                        .symbols
                        .get(symbol)
                        .expect("symbol")
                        .name
                        .as_str()
                        .to_owned()
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_module_name_starting_with_b2c_is_e0221_in_a_project_of_one_module() {
    let mut document = doc(vec![main(vec![])]);
    document.modules[0].name = String::from("b2c_main");
    let analysis = analyze(&document);
    let found: Vec<_> = analysis
        .diagnostics
        .iter()
        .map(|d| (d.code.0.as_str(), d.severity, d.message.as_str()))
        .collect();
    assert_eq!(found, [("B2C-E0221", Severity::Error, RESERVED)]);
    let primary = &analysis.diagnostics[0].primary;
    assert_eq!(
        primary.module.as_ref().map(b2c_ir::ids::ModuleId::as_str),
        Some("mod_0")
    );
    assert_eq!(primary.block, None);
    // The file is named by the module's place, never by its name.
    assert_eq!(analysis.program.layout.modules[0].stem.as_str(), "module1");
}

#[test]
fn a_module_name_starting_with_b2c_is_e0221_in_a_project_of_several_modules() {
    let document = doc_modules(vec![
        ("main", vec![main(vec![])]),
        ("b2c-tools", vec![]),
        ("tools_b2c", vec![]),
    ]);
    let analysis = analyze(&document);
    let found: Vec<_> = analysis
        .diagnostics
        .iter()
        .map(|d| {
            (
                d.code.0.as_str(),
                d.primary.module.as_ref().map(|m| m.as_str().to_owned()),
            )
        })
        .collect();
    assert_eq!(found, [("B2C-E0221", Some(String::from("mod_1")))]);
    assert_eq!(analysis.diagnostics[0].message, RESERVED);
    let stems: Vec<&str> = analysis
        .program
        .layout
        .modules
        .iter()
        .map(|files| files.stem.as_str())
        .collect();
    assert_eq!(stems, ["main", "module2", "tools_b2c"]);
}

#[test]
fn definitions_are_shared_only_in_a_project_of_several_modules() {
    let blocks = || {
        vec![
            main(vec![]),
            define("helper", true),
            define("other", false),
            global("count", true),
            structure("Point", true),
        ]
    };
    // One module: the ticks are kept in the file but share nothing.
    let single = analyze(&doc(blocks()));
    assert!(single.diagnostics.is_empty(), "{:?}", single.diagnostics);
    assert_eq!(shared_items(&single.program), [Vec::<String>::new()]);
    assert!(single.symbol_infos().iter().all(|info| !info.shared));

    // Two modules: the ticked definitions are shared, in either module (in
    // item order, by block ID).
    let multi = analyze(&doc_modules(vec![
        ("main", blocks()),
        ("utils", vec![define("tool", true), define("spare", false)]),
    ]));
    assert!(multi.diagnostics.is_empty(), "{:?}", multi.diagnostics);
    assert_eq!(
        shared_items(&multi.program),
        [vec!["helper", "count", "Point"], vec!["tool"]]
    );
    let mut shared: Vec<String> = multi
        .symbol_infos()
        .into_iter()
        .filter(|info| info.shared)
        .map(|info| info.name)
        .collect();
    shared.sort();
    assert_eq!(shared, ["Point", "count", "helper", "tool"]);
    // The SAST writes `shared` only when it is true: `spare`, then `tool`.
    let json = serde_json::to_value(&multi.program.modules[1].items).expect("json");
    assert!(json[0].get("shared").is_none(), "{json}");
    assert_eq!(json[1]["shared"], true);
}

#[test]
fn shared_definitions_of_every_kind_have_headers() {
    // Package F put shared functions into headers; package T adds shared
    // structs, enums and globals (w3-modules.md §1.2, §1.3).
    let analysis = analyze(&doc_modules(vec![
        (
            "main",
            vec![main(vec![]), define("helper", false), global("count", true)],
        ),
        ("utils", vec![define("tool", true), structure("Point", true)]),
    ]));
    assert!(analysis.diagnostics.is_empty(), "{:?}", analysis.diagnostics);
    let layout = &analysis.program.layout;
    assert!(layout.multi_module());
    let names = |module: usize, order: &[usize]| -> Vec<String> {
        order
            .iter()
            .map(|index| {
                let symbol = match &analysis.program.modules[module].items[*index].kind {
                    ItemKind::Function(def) => &def.symbol,
                    ItemKind::Struct(def) => &def.symbol,
                    ItemKind::Global(def) => &def.symbol,
                    other => panic!("{other:?} in the header"),
                };
                analysis
                    .program
                    .symbols
                    .get(symbol)
                    .expect("symbol")
                    .name
                    .as_str()
                    .to_owned()
            })
            .collect()
    };
    let main_files = &layout.modules[0];
    assert!(main_files.header);
    assert_eq!(names(0, &main_files.header_order), ["count"]);
    assert_eq!(
        main_files.source_includes,
        [analysis.program.modules[0].id.clone()]
    );
    let utils = &layout.modules[1];
    assert!(utils.header);
    // Types before functions, as in a `.cpp` (06 §6.7).
    assert_eq!(names(1, &utils.header_order), ["Point", "tool"]);
    assert_eq!(utils.source_includes, [analysis.program.modules[1].id.clone()]);
    for files in &layout.modules {
        assert!(files.header_includes.is_empty() && files.forward.is_empty());
    }
    assert_eq!(layout.modules[0].source_order.len(), 2);
    assert_eq!(layout.modules[1].source_order.len(), 2);
    // No namespace and no spelling other than the default.
    assert!(analysis.program.spellings.is_empty());
    let items = analysis.program.modules.iter().flat_map(|module| &module.items);
    assert!(items.clone().all(|item| item.namespace.is_none()));
}

/// A program of `modules` modules, each sharing `per_module` structs,
/// functions and globals; every struct holds the one of the module before it
/// by value and every function calls the one of the module before it, so
/// each module's header includes the one before it and each `.cpp` uses it.
fn layered_program(modules: usize, per_module: usize) -> b2c_model::Document {
    let names: Vec<String> = (0..modules).map(|m| format!("m{m}")).collect();
    let mut all = Vec::new();
    for (m, name) in names.iter().enumerate() {
        let mut blocks = Vec::new();
        if m == 0 {
            blocks.push(main(vec![call_stmt(&format!("f{}_0", modules - 1), vec![])]));
        }
        for k in 0..per_module {
            let field = if m == 0 {
                json!({"sym": sym(&format!("x{m}_{k}")), "name": "x", "type": "int"})
            } else {
                json!({"sym": sym(&format!("x{m}_{k}")), "name": "inner",
                       "type": {"ref": sym(&format!("S{}_{k}", m - 1))}})
            };
            let name = format!("S{m}_{k}");
            blocks.push(json!({
                "id": format!("st_{name}"), "type": "type.struct", "v": 1,
                "fields": {"NAME": {"sym": sym(&name), "name": name}, "SHARED": true},
                "extra": {"fields": [field]}
            }));
            let value = if m == 0 {
                common::e("1")
            } else {
                call(&format!("f{}_{k}", m - 1), vec![])
            };
            let mut function = func(&format!("f{m}_{k}"), "int", &[], vec![ret(Some(value))]);
            function["fields"]["SHARED"] = json!(true);
            blocks.push(function);
            blocks.push(global(&format!("g{m}_{k}"), true));
        }
        all.push((name.as_str(), blocks));
    }
    doc_modules(all)
}

/// The layout of a program of 64 modules sharing 7,680 definitions, which
/// name definitions of other modules 5,041 times, is computed in about linear time
/// in its declarations and uses (w3-modules.md §5.3 item 7) and never
/// panics: the analysis, the layout included, takes about 1.3 s in a debug
/// build, twice as long with twice as many definitions per module, and the
/// layout's own part grows linearly with the modules too.
#[test]
fn the_layout_is_computed_in_about_linear_time() {
    let document = layered_program(64, 40);
    let started = std::time::Instant::now();
    let analysis = analyze(&document);
    let took = started.elapsed();
    assert_clean(&analysis);
    let layout = &analysis.program.layout;
    assert_eq!(layout.modules.len(), 64);
    assert!(
        layout
            .modules
            .iter()
            .all(|files| files.header && files.header_order.len() == 120)
    );
    // Each header includes the one before it, for the struct it holds.
    for (index, files) in layout.modules.iter().enumerate().skip(1) {
        assert_eq!(
            files.header_includes,
            [analysis.program.modules[index - 1].id.clone()]
        );
    }
    assert!(took < std::time::Duration::from_secs(10), "{took:?}");
}

/// The generated files of a file of the malicious-project suite
/// (`tests/security/projects/`), which must analyse cleanly, by path.
fn security_suite_files(name: &str) -> Vec<String> {
    let path = common::repo_root().join("tests/security/projects").join(name);
    let document = common::resolved(&std::fs::read(path).unwrap(), name);
    let analysis = analyze(&document);
    assert_clean(&analysis);
    let project = b2c_codegen::generate(&analysis.program, &b2c_codegen::CodegenOptions::default());
    project.files.into_iter().map(|file| file.path).collect()
}

/// The malicious-project suite's project of 256 modules, the most a project
/// may have (w3-modules.md §5.3 item 7): each module but the first gets its
/// `.cpp` and a header.
#[test]
fn the_security_suites_256_modules_give_a_file_each() {
    let paths = security_suite_files("most-modules.b2c");
    let mut expected = vec![String::from("main.cpp")];
    for k in 1..256 {
        expected.extend([format!("m{k}.cpp"), format!("m{k}.hpp")]);
    }
    let mut sorted = paths.clone();
    sorted.sort();
    expected.sort();
    assert_eq!(sorted, expected);
}

/// The malicious-project suite's module names at the limits (w3-modules.md
/// §5.3 item 7): one letter, 64 characters, a hyphen and a device-like
/// `com0x` are file stems as they are.
#[test]
fn the_security_suites_module_names_at_the_limits_are_their_stems() {
    let long = format!("a{}c", "b".repeat(62));
    let mut expected = vec![String::from("main.cpp")];
    for stem in ["a", long.as_str(), "module-2", "com0x"] {
        expected.extend([format!("{stem}.cpp"), format!("{stem}.hpp")]);
    }
    let mut paths = security_suite_files("module-names-at-limits.b2c");
    paths.sort();
    expected.sort();
    assert_eq!(paths, expected);
}
