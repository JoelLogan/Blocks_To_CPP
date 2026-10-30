//! Tests of the files W3's contracts step prints for projects of several
//! modules (`docs/plans/m3-contracts/w3-modules.md` §1.7, §3.1, §5.2 P0):
//! file names from the layout's stems, the helpers in `b2c_support.hpp`
//! from two modules on, each module's non-shared code in the anonymous
//! namespace with an enum's `b2c::to_text` at global scope, a module's
//! header, and definitions in user namespaces. Each program goes through
//! the stages a build runs (load, resolve, analyse, generate); the ones the
//! analyser accepts are compiled and run with every g++.

use std::path::Path;

use b2c_ir::ids::{ModuleId, SymbolId};
use b2c_ir::sast::{Origin, Program, Symbol, SymbolKind};
use b2c_ir::source_map::{FileKind, GeneratedProject};
use b2c_ir::text::Ident;
use b2c_ir::types::Type;
use serde_json::{Value, json};

use super::tests::{STRICT, command, compilers};
use crate::CodegenOptions;

/// A project of these modules (`(id, name, blocks)`).
fn project(modules: &[(&str, &str, Value)]) -> Value {
    let modules: Vec<Value> = modules
        .iter()
        .map(|(id, name, blocks)| json!({"id": id, "name": name, "workspace": {"blocks": blocks}}))
        .collect();
    json!({
        "format": "blocks2cpp/project",
        "formatVersion": 1,
        "generator": {"app": "0.1.0", "catalog": "1.0.0"},
        "project": {"id": "prj_colours", "name": "Colours", "language": {"standard": "c++17"}},
        "modules": modules
    })
}

/// `main`'s blocks: an enum that is joined, a function in the module that
/// uses it, and `ask<int>`, which calls a helper.
fn colours_main() -> Value {
    json!([
        {"id": "b_colour", "type": "type.enum", "v": 1,
         "fields": {"NAME": {"sym": "s_Colour", "name": "Colour"}},
         "extra": {"values": [{"name": "Red", "sym": "s_Red"}, {"name": "Green", "sym": "s_Green"}]}},
        {"id": "b_describe", "type": "func.define", "v": 1,
         "fields": {"NAME": {"sym": "s_describe", "name": "describe"}, "RETURNS": "std::string"},
         "extra": {"params": [{"mode": "copy", "name": "colour", "sym": "s_colour", "type": {"ref": "s_Colour"}}]},
         "statements": {"BODY": [
            {"id": "b_return", "type": "func.return", "v": 1,
             "inputs": {"VALUE": {"block": {"id": "b_join", "type": "text.join", "v": 1,
                "extra": {"itemCount": 2},
                "inputs": {"ITEM0": {"expr": [{"str": "colour "}]},
                           "ITEM1": {"expr": [{"ref": "s_colour"}]}}}}}}
         ]}},
        {"id": "b_main", "type": "program.main", "v": 1, "statements": {"BODY": [
            {"id": "b_n", "type": "var.declare", "v": 1,
             "fields": {"CONST": false, "NAME": {"sym": "s_n", "name": "n"}, "TYPE": "int"},
             "inputs": {"VALUE": {"expr": [{"num": "0"}]}}},
            {"id": "b_ask", "type": "io.ask", "v": 1, "fields": {"MODE": "keep_asking", "VAR": {"ref": "s_n"}},
             "inputs": {"PROMPT": {"expr": [{"str": "Number? "}]}}},
            {"id": "b_print", "type": "io.print", "v": 1, "extra": {"itemCount": 2},
             "fields": {"NEWLINE": true, "SEP": "space", "STREAM": "out"},
             "inputs": {
                "ITEM0": {"block": {"id": "b_call", "type": "func.call", "v": 1, "extra": {"argCount": 1},
                    "fields": {"FUNC": {"ref": "s_describe"}},
                    "inputs": {"ARG0": {"block": {"id": "b_green", "type": "type.enum_value", "v": 1,
                        "fields": {"ENUM": {"ref": "s_Colour"}, "VALUE": {"ref": "s_Green"}}}}}}},
                "ITEM1": {"expr": [{"ref": "s_n"}]}}}
        ]}}
    ])
}

/// A module with one enum nothing uses (an unused type draws no warning).
fn sizes_module() -> Value {
    json!([
        {"id": "b_size", "type": "type.enum", "v": 1,
         "fields": {"NAME": {"sym": "s_Size", "name": "Size"}},
         "extra": {"values": [{"name": "Small", "sym": "s_Small"}]}}
    ])
}

/// Loads, resolves and analyses a project as a build does, checking the
/// codes the analysis reports.
fn analyse(value: &Value, codes: &[&str]) -> Program {
    let document = b2c_model::load(&serde_json::to_vec(value).unwrap()).unwrap();
    let catalog = b2c_catalog::core_catalog();
    let resolved = b2c_catalog::resolve_report(&document, catalog);
    assert!(resolved.diagnostics.is_empty(), "{:#?}", resolved.diagnostics);
    let analysis = b2c_lang::analyze_checked_with(
        &resolved.document,
        &resolved.diagnostics,
        &resolved.unchecked,
        catalog.sigs(),
    );
    let found: Vec<&str> = analysis.diagnostics.iter().map(|d| d.code.0.as_str()).collect();
    assert_eq!(found, codes, "{:#?}", analysis.diagnostics);
    analysis.program
}

/// Generates a program, with no error placeholder.
fn generate(program: &Program) -> GeneratedProject {
    let options = CodegenOptions {
        project_name: String::from("Colours"),
        app_version: String::from("1.0.0"),
        ..CodegenOptions::default()
    };
    let generation = crate::generate_with_report(program, &options);
    assert_eq!(generation.placeholders, 0);
    generation.project
}

/// Every file, under a line with its name.
fn render(project: &GeneratedProject) -> String {
    project
        .files
        .iter()
        .map(|file| format!("==> {} <==\n{}", file.path, file.contents))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The files' names, kinds and modules, with the source map's.
fn files(project: &GeneratedProject) -> Vec<(String, FileKind, Option<String>)> {
    for (file, map) in project.files.iter().zip(&project.source_map.files) {
        assert_eq!((&file.path, &file.module), (&map.path, &map.module));
    }
    assert_eq!(project.files.len(), project.source_map.files.len());
    project
        .files
        .iter()
        .map(|file| {
            (
                file.path.clone(),
                file.kind,
                file.module.as_ref().map(|id| id.as_str().to_owned()),
            )
        })
        .collect()
}

/// Writes the project's files into a folder of `dir`, compiles its `.cpp`
/// files with each of `compilers` under C++17 at the *strict* level with
/// `-Werror` and links them, then runs each program with `input` and
/// checks it prints `output`.
fn compile_and_run(
    project: &GeneratedProject,
    compilers: &[std::path::PathBuf],
    dir: &Path,
    input: &str,
    output: &str,
) {
    use std::io::Write as _;
    let sources = dir.join("src");
    std::fs::create_dir_all(&sources).unwrap();
    for file in &project.files {
        std::fs::write(sources.join(&file.path), &file.contents).unwrap();
    }
    let units: Vec<std::path::PathBuf> = project
        .files
        .iter()
        .filter(|file| file.kind == FileKind::Source)
        .map(|file| sources.join(&file.path))
        .collect();
    let all = render(project);
    for (index, gxx) in compilers.iter().enumerate() {
        let program = dir.join(format!("program-{index}"));
        let built = command(gxx)
            .arg("-std=c++17")
            .args(STRICT)
            .arg("-iquote")
            .arg(&sources)
            .args(&units)
            .arg("-o")
            .arg(&program)
            .output()
            .unwrap();
        assert!(
            built.status.success(),
            "{} rejected the program:\n{}\n--- files ---\n{all}",
            gxx.display(),
            String::from_utf8_lossy(&built.stderr)
        );
        let mut run = command(&program)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        run.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        let run = run.wait_with_output().unwrap();
        assert!(run.status.success(), "{}: {:?}", gxx.display(), run.status);
        let stdout = String::from_utf8_lossy(&run.stdout).replace("\r\n", "\n");
        assert_eq!(stdout, output, "{}", gxx.display());
    }
}

/// `main.cpp` of a single-module program, then of the same program once a
/// second, empty module is added (w3-modules.md §2.13): the helpers move to
/// `b2c_support.hpp`, the module's own code into the anonymous namespace,
/// and the enum's `b2c::to_text` stays at global scope. Both compile and
/// run with every g++, the second as two units.
#[test]
fn a_second_module_moves_the_code_of_the_first() {
    let single = analyse(&project(&[("mod_main", "main", colours_main())]), &[]);
    let single = generate(&single);
    assert_eq!(
        files(&single),
        [(
            String::from("main.cpp"),
            FileKind::Source,
            Some(String::from("mod_main"))
        )]
    );
    insta::assert_snapshot!("w3_single_module", render(&single));

    let two = analyse(
        &project(&[
            ("mod_main", "main", colours_main()),
            ("mod_extra", "extra", json!([])),
        ]),
        &[],
    );
    let two = generate(&two);
    assert_eq!(
        files(&two),
        [
            (
                String::from("main.cpp"),
                FileKind::Source,
                Some(String::from("mod_main"))
            ),
            (
                String::from("extra.cpp"),
                FileKind::Source,
                Some(String::from("mod_extra"))
            ),
            (String::from("b2c_support.hpp"), FileKind::Header, None),
        ]
    );
    insta::assert_snapshot!("w3_second_empty_module", render(&two));
    // The build's options and the preview's give the same files: the module
    // count places the helpers, not `helper_placement`.
    let header = CodegenOptions {
        helper_placement: crate::HelperPlacement::Header,
        project_name: String::from("Colours"),
        app_version: String::from("1.0.0"),
        ..CodegenOptions::default()
    };
    let program = analyse(
        &project(&[
            ("mod_main", "main", colours_main()),
            ("mod_extra", "extra", json!([])),
        ]),
        &[],
    );
    assert_eq!(crate::generate(&program, &header), two);

    let compilers = compilers();
    if compilers.is_empty() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let output = "Number? colour Green 7\n";
    compile_and_run(&single, &compilers, &dir.path().join("single"), "7\n", output);
    compile_and_run(&two, &compilers, &dir.path().join("two"), "7\n", output);
}

/// Two modules that share nothing, each with code of its own: the second
/// module's enum is in its anonymous namespace too. Compiled and run.
#[test]
fn two_modules_share_nothing() {
    let program = analyse(
        &project(&[
            ("mod_main", "main", colours_main()),
            ("mod_sizes", "sizes", sizes_module()),
        ]),
        &[],
    );
    let project = generate(&program);
    insta::assert_snapshot!("w3_two_modules", render(&project));
    let compilers = compilers();
    if compilers.is_empty() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    compile_and_run(
        &project,
        &compilers,
        dir.path(),
        "x\n3\n",
        "Number? Please enter a whole number.\nNumber? colour Green 3\n",
    );
}

/// A module whose name starts with `b2c` is `B2C-E0221`, and its file is
/// named by its place, `module2.cpp`, never by its name.
#[test]
fn a_module_named_b2c_is_written_by_its_place() {
    let program = analyse(
        &project(&[
            ("mod_main", "main", colours_main()),
            ("mod_x", "b2c_x", sizes_module()),
        ]),
        &["B2C-E0221"],
    );
    let project = generate(&program);
    assert_eq!(
        files(&project),
        [
            (
                String::from("main.cpp"),
                FileKind::Source,
                Some(String::from("mod_main"))
            ),
            (
                String::from("module2.cpp"),
                FileKind::Source,
                Some(String::from("mod_x"))
            ),
            (String::from("b2c_support.hpp"), FileKind::Header, None),
        ]
    );
    insta::assert_snapshot!("w3_module_named_b2c", render(&project));
}

/// A module with a function that is shared (ticked in the file), with a
/// default value.
fn maths_module() -> Value {
    json!([
        {"id": "b_twice", "type": "func.define", "v": 1,
         "fields": {"NAME": {"sym": "s_twice", "name": "twice"}, "RETURNS": "int", "SHARED": true},
         "extra": {"params": [{"mode": "copy", "name": "value", "sym": "s_value", "type": "int"}]},
         "inputs": {"DEFAULT0": {"expr": [{"num": "2"}]}},
         "statements": {"BODY": [
            {"id": "b_twice_return", "type": "func.return", "v": 1,
             "inputs": {"VALUE": {"expr": [{"ref": "s_value"}]}}}
         ]}}
    ])
}

/// A module's header, as packages F and T will fill it: the layout says
/// what it declares (set here by hand, since no package puts anything into
/// a header yet). `maths.hpp` declares the shared `twice` with its default
/// value after a forward declaration of `Size`, and `maths.cpp` includes it
/// and defines `twice` at global scope, without the default; `sizes.hpp`
/// defines the enum `Size`, which `sizes.cpp` then leaves out. Compiled and
/// run.
#[test]
fn a_header_prints_what_the_layout_lists() {
    let mut program = analyse(
        &project(&[
            ("mod_main", "main", colours_main()),
            ("mod_sizes", "sizes", sizes_module()),
            ("mod_maths", "maths", maths_module()),
        ]),
        &[],
    );
    // The analyser shares what is ticked in a project of several modules.
    assert!(program.modules[2].items[0].shared);
    program.modules[1].items[0].shared = true;
    let files = &mut program.layout.modules;
    for files in files.iter_mut().skip(1) {
        files.header = true;
        files.header_order = vec![0];
        files.source_includes = vec![files.module.clone()];
    }
    // A forward declaration of another module's type, and a symbol that is
    // no type, which is left out.
    files[2].forward = vec![
        SymbolId::new("s_Size").unwrap(),
        SymbolId::new("s_twice").unwrap(),
    ];
    let project = generate(&program);
    assert_eq!(
        self::files(&project),
        [
            (
                String::from("main.cpp"),
                FileKind::Source,
                Some(String::from("mod_main"))
            ),
            (
                String::from("sizes.cpp"),
                FileKind::Source,
                Some(String::from("mod_sizes"))
            ),
            (
                String::from("sizes.hpp"),
                FileKind::Header,
                Some(String::from("mod_sizes"))
            ),
            (
                String::from("maths.cpp"),
                FileKind::Source,
                Some(String::from("mod_maths"))
            ),
            (
                String::from("maths.hpp"),
                FileKind::Header,
                Some(String::from("mod_maths"))
            ),
            (String::from("b2c_support.hpp"), FileKind::Header, None),
        ]
    );
    insta::assert_snapshot!("w3_headers", render(&project));
    let compilers = compilers();
    if compilers.is_empty() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    compile_and_run(
        &project,
        &compilers,
        dir.path(),
        "4\n",
        "Number? colour Green 4\n",
    );
}

/// Definitions in a user namespace (`Item::namespace`, set here by hand
/// until package N's block exists) are printed inside `namespace geo { … }`,
/// non-shared ones of a multi-module project inside `namespace geo {
/// namespace { … } }`; the enum's `b2c::to_text` is at global scope and
/// names it `geo::Colour`; `main` is at global scope.
#[test]
fn definitions_in_a_namespace_are_printed_inside_it() {
    let in_geo = |mut program: Program| {
        let geo = SymbolId::new("s_geo").unwrap();
        program.symbols.symbols.insert(
            geo.clone(),
            Symbol {
                name: Ident::new("geo").unwrap(),
                kind: SymbolKind::Namespace { parent: None },
                ty: Type::Void,
                origin: Origin::whole(
                    ModuleId::new("mod_main").unwrap(),
                    b2c_ir::ids::BlockId::new("b_geo").unwrap(),
                ),
            },
        );
        for item in &mut program.modules[0].items {
            if !matches!(item.kind, b2c_ir::sast::ItemKind::Main(_)) {
                item.namespace = Some(geo.clone());
            }
        }
        generate(&program)
    };
    let single = in_geo(analyse(&project(&[("mod_main", "main", colours_main())]), &[]));
    insta::assert_snapshot!("w3_namespace_single", render(&single));
    let two = in_geo(analyse(
        &project(&[
            ("mod_main", "main", colours_main()),
            ("mod_extra", "extra", json!([])),
        ]),
        &[],
    ));
    insta::assert_snapshot!("w3_namespace_two", &two.files[0].contents);
    // Every namespace opened is closed.
    for file in single.files.iter().chain(&two.files) {
        assert_eq!(
            file.contents.matches("\nnamespace geo {").count()
                + file.contents.matches("\nnamespace {").count(),
            file.contents.matches("}  // namespace geo").count()
                + file.contents.matches("}  // namespace\n").count(),
            "{}",
            file.contents
        );
    }
}
