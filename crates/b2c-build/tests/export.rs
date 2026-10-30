//! Export as a C++ project (`docs/spec/06-compiler-pipeline.md` §6.11,
//! `docs/plans/m3-contracts/w3-modules.md` §2.8): exported examples, and a
//! hand-built project of two modules with a header, build with `make` and
//! with CMake, without Blocks2Cpp, and the programs do what their golden run
//! data says (the same check as `golden.rs`); a replacement keeps the files
//! a student changed. The builds need g++ (required with `B2C_REQUIRE_GXX`),
//! and `make` (or MinGW's `mingw32-make`) or `cmake` on `PATH`; a tool that
//! is missing skips its part, except `make` with `B2C_REQUIRE_MAKE` (as in
//! CI). CMake is used on Linux only (on Windows it would choose MSVC).

// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};

use b2c_build::export::{self, EXPORT_MANIFEST, ExportMode, FolderState, KeptReason};
use b2c_ir::source_map::{FileKind, GeneratedFile, GeneratedProject, SourceMap};
use common::repo_root;
use common::run_data::RunData;

/// The examples built here: plain output, input, vectors and text.
const EXAMPLES: [&str; 4] = ["hello_world", "greeting", "matrix_sum", "palindrome"];

/// A command for a build tool. Production code spawns processes only
/// through b2c-process; tests drive the tools directly.
#[allow(clippy::disallowed_methods)]
fn tool(name: &str) -> std::process::Command {
    std::process::Command::new(name)
}

/// Whether `name` runs (`--version`).
fn available(name: &str) -> bool {
    tool(name)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// GNU make: `make`, or on Windows MinGW's `mingw32-make` (MSYS2's UCRT64
/// `make` package); `None` without one. With `B2C_REQUIRE_MAKE` set (as in
/// CI) a missing make fails the test instead of skipping its part.
fn make() -> Option<&'static str> {
    let names: &[&'static str] = if cfg!(windows) {
        &["make", "mingw32-make"]
    } else {
        &["make"]
    };
    let found = names.iter().copied().find(|name| available(name));
    assert!(
        found.is_some() || std::env::var_os("B2C_REQUIRE_MAKE").is_none(),
        "B2C_REQUIRE_MAKE is set but make was not found"
    );
    found
}

fn exported(name: &str, folder: &Path) -> export::Export {
    let bytes = std::fs::read(repo_root().join("examples").join(format!("{name}.b2c"))).unwrap();
    let export = export::prepare(&bytes, 4, "0.0.0-test").unwrap();
    let written = export::write(folder, &export, ExportMode::New).unwrap();
    assert!(written.kept.is_empty());
    export
}

fn run(tool_output: &std::process::Output, what: &str) {
    assert!(
        tool_output.status.success(),
        "{what} failed:\n{}\n{}",
        String::from_utf8_lossy(&tool_output.stdout),
        String::from_utf8_lossy(&tool_output.stderr)
    );
}

fn check(name: &str, executable: &Path) {
    let data = RunData::read(&repo_root().join("tests/golden").join(name)).unwrap();
    data.check(executable)
        .unwrap_or_else(|problem| panic!("{name}: {problem}"));
}

fn program(folder: &Path, name: &str) -> PathBuf {
    folder.join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// `example` with its first module renamed to `module` and its standard set
/// to `standard`, as a project file.
fn changed_example(example: &str, module: &str, standard: &str) -> Vec<u8> {
    let bytes = std::fs::read(repo_root().join("examples").join(format!("{example}.b2c"))).unwrap();
    let mut project: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    project["modules"][0]["name"] = serde_json::Value::from(module);
    project["project"]["language"]["standard"] = serde_json::Value::from(standard);
    project.to_string().into_bytes()
}

#[test]
fn exported_examples_build_with_make() {
    let Some(make) = make() else {
        return;
    };
    if common::gxx().is_none() {
        return;
    }
    for name in EXAMPLES {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("export");
        let export = exported(name, &folder);
        // Each configuration builds a program file of its own: `make` and
        // then `make CONFIG=release` compiles again.
        for config in ["debug", "release"] {
            let output = tool(make)
                .arg("-C")
                .arg(&folder)
                .arg(format!("CONFIG={config}"))
                .output()
                .unwrap();
            run(&output, &format!("make {name} {config}"));
            assert!(
                String::from_utf8_lossy(&output.stdout).contains(&format!("-o {}-{config}", export.name)),
                "make {name} {config} did not compile"
            );
            check(name, &program(&folder, &format!("{}-{config}", export.name)));
        }
    }
}

#[cfg(unix)]
#[test]
fn exported_examples_build_with_cmake() {
    if common::gxx().is_none() || !available("cmake") {
        return;
    }
    for name in &EXAMPLES[..2] {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("export");
        let export = exported(name, &folder);
        for config in ["Debug", "Release"] {
            let build = folder.join(format!("build-{config}"));
            run(
                &tool("cmake")
                    .arg("-S")
                    .arg(&folder)
                    .arg("-B")
                    .arg(&build)
                    .arg(format!("-DCMAKE_BUILD_TYPE={config}"))
                    .arg("-DCMAKE_CXX_COMPILER=g++")
                    .output()
                    .unwrap(),
                &format!("cmake configure {name} {config}"),
            );
            if config == "Debug" {
                sanitizers_reach_the_build(&build);
            }
            run(
                &tool("cmake").arg("--build").arg(&build).output().unwrap(),
                &format!("cmake build {name} {config}"),
            );
            check(name, &build.join(&export.name));
        }
    }
}

/// The debug configuration's sanitizers are on wherever this g++ can link
/// them: CMake's compile check of a sanitizer passes exactly when its link
/// check does, and the target's flags have it.
#[cfg(unix)]
fn sanitizers_reach_the_build(build: &Path) {
    let cache = std::fs::read_to_string(build.join("CMakeCache.txt")).unwrap();
    let result = |name: &str| {
        cache
            .lines()
            .find_map(|line| line.strip_prefix(&format!("{name}:INTERNAL=")))
            .map(str::to_owned)
    };
    let flag = "B2C_HAS__fsanitize_address_undefined";
    let link = result(&format!("{flag}_LINK"));
    assert_eq!(result(&format!("{flag}_COMPILE")), link, "{cache}");
    if link.as_deref() == Some("1") {
        let flags = std::fs::read_to_string(build.join("CMakeFiles/b2c_program.dir/flags.make")).unwrap();
        assert!(flags.contains("-fsanitize=address,undefined"), "{flags}");
    }
}

#[test]
fn reserved_program_names_and_cpp26_are_written_for_both_tools() {
    for (module, standard) in [("install", "c++26"), ("clean", "c++20"), ("all", "c++17")] {
        let export =
            export::prepare(&changed_example("hello_world", module, standard), 4, "0.0.0-test").unwrap();
        let file = |path: &str| {
            export
                .files
                .iter()
                .find(|file| file.path == path)
                .map(|file| file.contents.clone())
                .unwrap()
        };
        let cmake = file("CMakeLists.txt");
        assert!(cmake.contains("add_executable(b2c_program"), "{cmake}");
        assert!(cmake.contains(&format!("OUTPUT_NAME {module})")), "{cmake}");
        assert!(!cmake.contains(&format!("({module} PRIVATE")), "{cmake}");
        let make = file("Makefile");
        assert!(make.contains(&format!("PROGRAM := {module}\n")), "{make}");
        assert!(make.contains("$(OUT): $(SOURCES)"), "{make}");
        // The recipe is the compiler alone, which cmd.exe runs too.
        let recipe: Vec<&str> = make
            .split("$(OUT): $(SOURCES) $(HEADERS)\n")
            .nth(1)
            .unwrap()
            .lines()
            .take_while(|line| line.starts_with('\t'))
            .collect();
        assert_eq!(recipe.len(), 1, "{make}");
        assert!(recipe[0].starts_with("\t$(CXX) "), "{make}");
        assert!(!make.contains("mkdir"), "{make}");
        if standard == "c++26" {
            assert!(!cmake.contains("CMAKE_CXX_STANDARD"), "{cmake}");
            assert!(cmake.contains("PRIVATE -std=c++2c)"), "{cmake}");
            assert!(make.contains("-std=c++2c"), "{make}");
        }
    }
}

/// A program named after a target CMake reserves, or after the Makefile's
/// own `clean`, builds with both tools; so does a C++26 project, where g++
/// takes `-std=c++2c` (GCC 14 and newer).
#[cfg(unix)]
#[test]
fn exports_with_reserved_names_build() {
    if common::gxx().is_none() {
        return;
    }
    let cpp26 = tool("g++")
        .args(["-std=c++2c", "-x", "c++", "-fsyntax-only", "-"])
        .stdin(std::process::Stdio::null())
        .output()
        .is_ok_and(|output| output.status.success());
    for (module, standard) in [("install", "c++20"), ("clean", "c++20"), ("install", "c++26")] {
        if standard == "c++26" && !cpp26 {
            continue;
        }
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("export");
        let export =
            export::prepare(&changed_example("hello_world", module, standard), 4, "0.0.0-test").unwrap();
        export::write(&folder, &export, ExportMode::New).unwrap();
        if available("cmake") {
            let build = folder.join("build");
            run(
                &tool("cmake")
                    .arg("-S")
                    .arg(&folder)
                    .arg("-B")
                    .arg(&build)
                    .arg("-DCMAKE_CXX_COMPILER=g++")
                    .output()
                    .unwrap(),
                &format!("cmake configure {module} {standard}"),
            );
            run(
                &tool("cmake").arg("--build").arg(&build).output().unwrap(),
                &format!("cmake build {module} {standard}"),
            );
            check("hello_world", &build.join(module));
        }
        if let Some(make) = make() {
            run(
                &tool(make).arg("-C").arg(&folder).output().unwrap(),
                &format!("make {module} {standard}"),
            );
            let built = folder.join(format!("{module}-debug"));
            check("hello_world", &built);
            run(
                &tool(make).arg("-C").arg(&folder).arg("clean").output().unwrap(),
                &format!("make clean {module}"),
            );
            assert!(!built.exists());
            assert!(folder.join("Makefile").is_file());
        }
    }
}

/// Any SANITIZE or HARDEN but 1 turns its flags off and names a program
/// file of its own, so the flagged build is never taken for it.
#[test]
fn sanitize_and_harden_values_other_than_1_build_a_file_of_their_own() {
    let export = export::prepare(&changed_example("hello_world", "main", "c++20"), 4, "0.0.0-test").unwrap();
    let make = export
        .files
        .iter()
        .find(|file| file.path == "Makefile")
        .map(|file| file.contents.clone())
        .unwrap();
    assert!(
        make.contains("$(if $(filter 1,$(SANITIZE)),,-nosanitize)$(if $(filter 1,$(HARDEN)),,-noharden)"),
        "{make}"
    );
    assert!(make.contains("ifeq ($(SANITIZE),1)") && make.contains("ifeq ($(HARDEN),1)"));
    let Some(make) = self::make() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("export");
    export::write(&folder, &export, ExportMode::New).unwrap();
    for (args, out) in [
        (&["SANITIZE=no"][..], "main-debug-nosanitize"),
        (&["HARDEN=off"][..], "main-debug-noharden"),
        (&["SANITIZE=0", "HARDEN=0"][..], "main-debug-nosanitize-noharden"),
        (&[][..], "main-debug"),
    ] {
        let output = tool(make)
            .arg("-C")
            .arg(&folder)
            .arg("-n")
            .args(args)
            .output()
            .unwrap();
        run(&output, &format!("make -n {args:?}"));
        let commands = String::from_utf8_lossy(&output.stdout);
        let line = commands
            .lines()
            .find(|line| line.contains(" -o "))
            .unwrap_or_else(|| panic!("{commands}"));
        let target = line.split(" -o ").nth(1).unwrap().split(' ').next().unwrap();
        assert_eq!(target, format!("{out}{}", std::env::consts::EXE_SUFFIX), "{line}");
        assert_eq!(line.contains("-fsanitize"), !out.contains("nosanitize"), "{line}");
    }
}

#[test]
fn an_export_is_never_replaced_by_another_projects() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("export");
    let first = exported("hello_world", &folder);
    assert_eq!(
        export::earlier_export_project(&folder),
        Some(Some(first.project_id.clone()))
    );
    let main = std::fs::read_to_string(folder.join("src/main.cpp")).unwrap();
    let bytes = std::fs::read(repo_root().join("examples/greeting.b2c")).unwrap();
    let second = export::prepare(&bytes, 4, "0.0.0-test").unwrap();
    assert_ne!(first.project_id, second.project_id);
    assert!(matches!(
        export::write(&folder, &second, ExportMode::Replace),
        Err(export::ExportError::OtherProject)
    ));
    assert_eq!(
        std::fs::read_to_string(folder.join("src/main.cpp")).unwrap(),
        main
    );
    // The same project replaces its own; a format 1 manifest (no project)
    // counts as this project's.
    export::write(&folder, &first, ExportMode::Replace).unwrap();
    let manifest = std::fs::read_to_string(folder.join(EXPORT_MANIFEST)).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    value["formatVersion"] = serde_json::Value::from(1);
    value.as_object_mut().unwrap().remove("projectId");
    value["files"] = paths_only(&value);
    std::fs::write(folder.join(EXPORT_MANIFEST), value.to_string()).unwrap();
    assert_eq!(export::earlier_export_project(&folder), Some(None));
    export::write(&folder, &second, ExportMode::Replace).unwrap();
    // The folder is named after the project.
    assert_eq!(first.folder_name, "hello-world");
}

#[cfg(unix)]
#[test]
fn a_linked_source_folder_is_never_followed_to_remove_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let folder = dir.path().join("export");
    let export = exported("hello_world", &folder);
    // The earlier export also wrote src/old.cpp, which this one does not.
    let manifest = std::fs::read_to_string(folder.join(EXPORT_MANIFEST)).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    value["files"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({"path": "src/old.cpp", "sha256": sha256("keep")}));
    std::fs::write(folder.join(EXPORT_MANIFEST), value.to_string()).unwrap();
    std::fs::remove_dir_all(folder.join("src")).unwrap();
    std::fs::write(outside.path().join("old.cpp"), "keep").unwrap();
    std::os::unix::fs::symlink(outside.path(), folder.join("src")).unwrap();
    assert!(matches!(
        export::write(&folder, &export, ExportMode::Replace),
        Err(export::ExportError::NotAPlainFile)
    ));
    assert_eq!(
        std::fs::read_to_string(outside.path().join("old.cpp")).unwrap(),
        "keep"
    );
}

#[test]
fn an_export_never_writes_over_what_it_did_not_make() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("export");
    let export = exported("hello_world", &folder);
    assert_eq!(export::folder_state(&folder), FolderState::EarlierExport);
    assert!(folder.join(EXPORT_MANIFEST).is_file());
    // Again as new: refused; as a replacement: only its own files change.
    assert!(matches!(
        export::write(&folder, &export, ExportMode::New),
        Err(export::ExportError::NotEmpty)
    ));
    std::fs::write(folder.join("notes.txt"), "mine").unwrap();
    std::fs::write(folder.join("src").join("mine.cpp"), "// mine").unwrap();
    std::fs::write(folder.join("Makefile"), "changed").unwrap();
    std::fs::remove_file(folder.join("README.md")).unwrap();
    let written = export::write(&folder, &export, ExportMode::Replace).unwrap();
    assert_eq!(written.files.last().map(String::as_str), Some(EXPORT_MANIFEST));
    assert_eq!(std::fs::read_to_string(folder.join("notes.txt")).unwrap(), "mine");
    assert_eq!(
        std::fs::read_to_string(folder.join("src/mine.cpp")).unwrap(),
        "// mine"
    );
    // The Makefile the student changed is kept, and named; the README they
    // deleted is written again.
    assert_eq!(
        std::fs::read_to_string(folder.join("Makefile")).unwrap(),
        "changed"
    );
    assert_eq!(kept(&written), [("Makefile", KeptReason::Changed, false)]);
    assert!(!written.files.contains(&String::from("Makefile")));
    assert!(folder.join("README.md").is_file());
    assert_eq!(
        written.kept[0].to_string(),
        "Makefile was kept because it was changed after the earlier export (this export's version of it was not written)"
    );

    // A folder with other things in it is not an export.
    let other = dir.path().join("other");
    std::fs::create_dir(&other).unwrap();
    std::fs::write(other.join("a.txt"), "a").unwrap();
    assert_eq!(export::folder_state(&other), FolderState::Other);
    assert!(matches!(
        export::write(&other, &export, ExportMode::Replace),
        Err(export::ExportError::NoEarlierExport)
    ));
    assert_eq!(
        export::folder_state(&dir.path().join("missing")),
        FolderState::Empty
    );
}

#[cfg(unix)]
#[test]
fn links_are_never_written_through() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("Makefile"), "outside").unwrap();
    // The folder itself is a link.
    let link = dir.path().join("export");
    std::os::unix::fs::symlink(outside.path(), &link).unwrap();
    let bytes = std::fs::read(repo_root().join("examples/hello_world.b2c")).unwrap();
    let export = export::prepare(&bytes, 4, "0.0.0-test").unwrap();
    assert!(export::write(&link, &export, ExportMode::New).is_err());
    assert!(export::write(&link, &export, ExportMode::Replace).is_err());
    // A file of an earlier export replaced by a link.
    let folder = dir.path().join("real");
    export::write(&folder, &export, ExportMode::New).unwrap();
    std::fs::remove_file(folder.join("Makefile")).unwrap();
    std::os::unix::fs::symlink(outside.path().join("Makefile"), folder.join("Makefile")).unwrap();
    assert!(matches!(
        export::write(&folder, &export, ExportMode::Replace),
        Err(export::ExportError::NotAPlainFile)
    ));
    assert_eq!(
        std::fs::read_to_string(outside.path().join("Makefile")).unwrap(),
        "outside"
    );
}

#[test]
fn a_project_with_errors_is_not_exported() {
    let error = export::prepare(b"{ not a project", 4, "0.0.0-test").unwrap_err();
    assert!(matches!(error, export::ExportError::ProjectErrors(problems) if !problems.is_empty()));
}

#[test]
fn a_replacement_never_writes_over_a_file_the_earlier_export_did_not_list() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("export");
    let export = exported("hello_world", &folder);
    // As if the earlier export had no Makefile and someone wrote their own.
    let manifest = std::fs::read_to_string(folder.join(EXPORT_MANIFEST)).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    value["files"]
        .as_array_mut()
        .unwrap()
        .retain(|file| file["path"] != "Makefile");
    std::fs::write(folder.join(EXPORT_MANIFEST), value.to_string()).unwrap();
    std::fs::write(folder.join("Makefile"), "mine").unwrap();
    assert!(matches!(
        export::write(&folder, &export, ExportMode::Replace),
        Err(export::ExportError::NotWrittenByExport(path)) if path == "Makefile"
    ));
    assert_eq!(std::fs::read_to_string(folder.join("Makefile")).unwrap(), "mine");
}

#[test]
fn a_text_define_the_build_tools_cannot_pass_is_refused() {
    let bytes = std::fs::read(repo_root().join("examples/hello_world.b2c")).unwrap();
    let mut project: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    for (text, exportable) in [
        ("a b \"c\" \\ d", true),
        ("a;b", false),
        ("x#y", false),
        ("q $<1:z>", false),
    ] {
        project["project"]["build"]["defines"] =
            serde_json::json!([{ "name": "GREETING", "value": { "string": text } }]);
        let result = export::prepare(project.to_string().as_bytes(), 4, "0.0.0-test");
        match result {
            Ok(_) => assert!(exportable, "{text}"),
            Err(export::ExportError::DefineNotExportable(problems)) => {
                assert!(!exportable, "{text}");
                assert_eq!(problems.len(), 1);
                assert_eq!(problems[0].code, b2c_ir::DiagCode::new("B2C-T1019"));
            }
            Err(error) => panic!("{text}: {error}"),
        }
    }
}

/// The SHA-256 of `text`, as a manifest records it.
fn sha256(text: &str) -> String {
    use sha2::Digest as _;
    b2c_model::hex(&sha2::Sha256::digest(text.as_bytes()))
}

/// A manifest's entries as format 1 and 2 list them: paths only.
fn paths_only(manifest: &serde_json::Value) -> serde_json::Value {
    manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["path"].clone())
        .collect()
}

/// The kept files of a write: path, reason, and whether the export no
/// longer has the file.
fn kept(written: &export::Written) -> Vec<(&str, KeptReason, bool)> {
    written
        .kept
        .iter()
        .map(|file| (file.path.as_str(), file.reason, file.no_longer_written))
        .collect()
}

/// One generated file of `module`.
fn generated(path: &str, kind: FileKind, module: &str, contents: &str) -> GeneratedFile {
    GeneratedFile {
        path: path.to_owned(),
        kind,
        module: Some(b2c_ir::ModuleId::new(module).unwrap()),
        contents: contents.to_owned(),
    }
}

/// What the hand-built program prints.
const TWO_MODULES_OUTPUT: &str = "twice(21) = 42\n";

/// A two-module project made by hand, as the analyser cannot make one yet
/// (w3-modules.md §2.8): `main` calls `twice` of module `helper`, which its
/// header `<helper>.hpp` declares. `helper` names the module (`utils`, then
/// `tools` after a rename); `None` is the project after the module was
/// deleted, with `main` alone.
fn two_modules(helper: Option<&str>) -> (b2c_model::Document, GeneratedProject) {
    let mut document =
        b2c_model::load(&std::fs::read(repo_root().join("examples/hello_world.b2c")).unwrap()).unwrap();
    let mut files = Vec::new();
    match helper {
        Some(name) => {
            document.modules.push(b2c_model::Module {
                id: b2c_ir::ModuleId::new("mod_helper").unwrap(),
                name: name.to_owned(),
                workspace: b2c_model::Workspace::default(),
            });
            files.push(generated(
                "main.cpp",
                FileKind::Source,
                "mod_main",
                &format!(
                    "#include <iostream>\n\n#include \"{name}.hpp\"\n\nint main() {{\n    std::cout << \"twice(21) = \" << twice(21) << '\\n';\n    return 0;\n}}\n"
                ),
            ));
            files.push(generated(
                &format!("{name}.cpp"),
                FileKind::Source,
                "mod_helper",
                &format!("#include \"{name}.hpp\"\n\nint twice(int value) {{\n    return 2 * value;\n}}\n"),
            ));
            files.push(generated(
                &format!("{name}.hpp"),
                FileKind::Header,
                "mod_helper",
                "#pragma once\n\nint twice(int value);\n",
            ));
        }
        None => files.push(generated(
            "main.cpp",
            FileKind::Source,
            "mod_main",
            "#include <iostream>\n\nint main() {\n    std::cout << \"twice(21) = \" << 2 * 21 << '\\n';\n    return 0;\n}\n",
        )),
    }
    (
        document,
        GeneratedProject {
            files,
            source_map: SourceMap::default(),
        },
    )
}

/// The export of [`two_modules`].
fn two_modules_export(helper: Option<&str>) -> export::Export {
    let (document, generated) = two_modules(helper);
    export::assemble(&document, &generated, "0.0.0-test").unwrap()
}

/// Runs `program` and checks that it prints [`TWO_MODULES_OUTPUT`].
fn check_two_modules(program: &Path) {
    // Production code spawns processes only through b2c-process; tests run
    // the programs they built directly.
    #[allow(clippy::disallowed_methods)]
    let output = std::process::Command::new(program).output().unwrap();
    run(&output, &format!("{}", program.display()));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace("\r\n", "\n"),
        TWO_MODULES_OUTPUT
    );
}

/// An export of several sources with a header: every `.cpp` in CMake's
/// `add_executable` and the Makefile's `SOURCES` in module order, the
/// header in `src/`, and both build files build it.
#[test]
fn a_project_of_two_modules_with_a_header_builds_with_both_tools() {
    let export = two_modules_export(Some("utils"));
    let paths: Vec<&str> = export.files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "src/main.cpp",
            "src/utils.cpp",
            "src/utils.hpp",
            "CMakeLists.txt",
            "Makefile",
            "README.md"
        ]
    );
    let file = |path: &str| {
        export
            .files
            .iter()
            .find(|file| file.path == path)
            .map(|file| file.contents.clone())
            .unwrap()
    };
    assert!(
        file("CMakeLists.txt").contains("add_executable(b2c_program\n  src/main.cpp\n  src/utils.cpp\n)"),
        "{}",
        file("CMakeLists.txt")
    );
    assert!(file("Makefile").contains("SOURCES := src/main.cpp src/utils.cpp\n"));
    assert!(file("Makefile").contains("HEADERS := $(wildcard src/*.hpp)\n"));
    assert_eq!(export.name, "main");
    if common::gxx().is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("export");
    export::write(&folder, &export, ExportMode::New).unwrap();
    if let Some(make) = make() {
        run(
            &tool(make).arg("-C").arg(&folder).output().unwrap(),
            "make of two modules",
        );
        check_two_modules(&program(&folder, "main-debug"));
    }
    #[cfg(unix)]
    if available("cmake") {
        let build = folder.join("build");
        run(
            &tool("cmake")
                .arg("-S")
                .arg(&folder)
                .arg("-B")
                .arg(&build)
                .arg("-DCMAKE_CXX_COMPILER=g++")
                .output()
                .unwrap(),
            "cmake configure of two modules",
        );
        run(
            &tool("cmake").arg("--build").arg(&build).output().unwrap(),
            "cmake build of two modules",
        );
        check_two_modules(&build.join("main"));
    }
}

/// The manifest (format 3) records the SHA-256 of each file written. After
/// a module is renamed, and again after it is deleted, Replace removes the
/// old module's unchanged files and keeps the one the student edited,
/// naming it, and goes on; the kept file stays listed as the earlier export
/// recorded it, so it is kept again until it is back to what was written.
#[test]
fn replace_after_a_rename_and_a_delete_keeps_the_edited_files() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("export");
    let first = two_modules_export(Some("utils"));
    export::write(&folder, &first, ExportMode::New).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join(EXPORT_MANIFEST)).unwrap()).unwrap();
    assert_eq!(manifest["formatVersion"], 3);
    assert_eq!(
        manifest["projectId"],
        serde_json::Value::from(first.project_id.clone())
    );
    for file in &first.files {
        let entry = manifest["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["path"] == file.path.as_str())
            .unwrap();
        assert_eq!(entry["sha256"], serde_json::Value::from(sha256(&file.contents)));
    }
    let utils = std::fs::read_to_string(folder.join("src/utils.cpp")).unwrap();
    let edited = format!("{utils}// my note\n");
    std::fs::write(folder.join("src/utils.cpp"), &edited).unwrap();

    // `utils` renamed to `tools`.
    let renamed = two_modules_export(Some("tools"));
    let written = export::write(&folder, &renamed, ExportMode::Replace).unwrap();
    assert_eq!(kept(&written), [("src/utils.cpp", KeptReason::Changed, true)]);
    assert_eq!(
        written.kept[0].to_string(),
        "src/utils.cpp was kept because it was changed after the earlier export (this export no longer has the file)"
    );
    assert!(!folder.join("src/utils.hpp").exists());
    assert_eq!(
        std::fs::read_to_string(folder.join("src/utils.cpp")).unwrap(),
        edited
    );
    for file in &renamed.files {
        assert_eq!(
            std::fs::read_to_string(folder.join(&file.path)).unwrap(),
            file.contents,
            "{}",
            file.path
        );
    }
    if common::gxx().is_some()
        && let Some(make) = make()
    {
        run(
            &tool(make).arg("-C").arg(&folder).output().unwrap(),
            "make after the rename",
        );
        check_two_modules(&program(&folder, "main-debug"));
    }

    // `tools` deleted; the student edited its header and the README.
    std::fs::write(folder.join("src/tools.hpp"), "#pragma once\n// mine\n").unwrap();
    std::fs::write(folder.join("README.md"), "# My notes\n").unwrap();
    let deleted = two_modules_export(None);
    let written = export::write(&folder, &deleted, ExportMode::Replace).unwrap();
    assert_eq!(
        kept(&written),
        [
            ("src/tools.hpp", KeptReason::Changed, true),
            ("README.md", KeptReason::Changed, false),
            ("src/utils.cpp", KeptReason::Changed, true),
        ]
    );
    assert!(!folder.join("src/tools.cpp").exists());
    assert_eq!(
        std::fs::read_to_string(folder.join("README.md")).unwrap(),
        "# My notes\n"
    );
    assert!(!written.files.contains(&String::from("README.md")));
    assert!(written.files.contains(&String::from("src/main.cpp")));
    if common::gxx().is_some()
        && let Some(make) = make()
    {
        run(
            &tool(make).arg("-C").arg(&folder).output().unwrap(),
            "make after the delete",
        );
        check_two_modules(&program(&folder, "main-debug"));
    }

    // Back to what the first export wrote, `utils.cpp` is deleted; the
    // README the student deleted is written again.
    std::fs::write(folder.join("src/utils.cpp"), &utils).unwrap();
    std::fs::remove_file(folder.join("README.md")).unwrap();
    let written = export::write(&folder, &deleted, ExportMode::Replace).unwrap();
    assert_eq!(kept(&written), [("src/tools.hpp", KeptReason::Changed, true)]);
    assert!(!folder.join("src/utils.cpp").exists());
    assert!(folder.join("README.md").is_file());
}

/// A Replace that fails part of the way through (here the export folder
/// is read-only while `src/` is not, as a full disk or a file another
/// program holds open would make it fail) leaves a folder that the next
/// Replace of a changed project takes as an export: nothing is kept as the
/// student's change and no new module's file is in the way. Unix only, and
/// skipped where the mode bits do not stop a write (as root).
#[cfg(unix)]
#[test]
fn a_replace_after_a_failed_one_keeps_nothing() {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = |path: &Path, mode: u32| {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    };
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("export");
    export::write(&folder, &two_modules_export(Some("utils")), ExportMode::New).unwrap();
    mode(&folder, 0o555);
    if std::fs::write(folder.join("probe"), "").is_ok() {
        mode(&folder, 0o755);
        return;
    }
    // `tools` added; main.cpp and the build files change.
    let (mut document, mut project) = two_modules(Some("utils"));
    document.modules.push(b2c_model::Module {
        id: b2c_ir::ModuleId::new("mod_tools").unwrap(),
        name: String::from("tools"),
        workspace: b2c_model::Workspace::default(),
    });
    project.files[0].contents.push_str("// with tools\n");
    project.files.push(generated(
        "tools.cpp",
        FileKind::Source,
        "mod_tools",
        "int tools_version() {\n    return 1;\n}\n",
    ));
    let second = export::assemble(&document, &project, "0.0.0-test").unwrap();
    let failed = export::write(&folder, &second, ExportMode::Replace);
    mode(&folder, 0o755);
    assert!(
        matches!(failed, Err(export::ExportError::Io { .. })),
        "{failed:?}"
    );
    // Changed once more: `tools.cpp` too.
    project.files[0].contents.push_str("// again\n");
    project.files.last_mut().unwrap().contents.push_str("// again\n");
    let third = export::assemble(&document, &project, "0.0.0-test").unwrap();
    let written = export::write(&folder, &third, ExportMode::Replace).unwrap();
    assert_eq!(kept(&written), []);
    for file in &third.files {
        assert_eq!(
            std::fs::read_to_string(folder.join(&file.path)).unwrap(),
            file.contents,
            "{}",
            file.path
        );
    }
}

/// An older manifest (format 2) records no hashes: Replace keeps every
/// listed file it would delete or change, naming each, and writes the rest.
#[test]
fn an_older_manifest_without_hashes_keeps_every_listed_file() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("export");
    let first = two_modules_export(Some("utils"));
    export::write(&folder, &first, ExportMode::New).unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join(EXPORT_MANIFEST)).unwrap()).unwrap();
    value["formatVersion"] = serde_json::Value::from(2);
    value["files"] = paths_only(&value);
    std::fs::write(folder.join(EXPORT_MANIFEST), value.to_string()).unwrap();
    assert_eq!(export::folder_state(&folder), FolderState::EarlierExport);

    let renamed = two_modules_export(Some("tools"));
    let written = export::write(&folder, &renamed, ExportMode::Replace).unwrap();
    // `main.cpp` and both build files differ from what this export writes;
    // the README is the same, so it is written (and recorded) again.
    assert_eq!(
        kept(&written),
        [
            ("src/main.cpp", KeptReason::NotRecorded, false),
            ("src/utils.cpp", KeptReason::NotRecorded, true),
            ("src/utils.hpp", KeptReason::NotRecorded, true),
            ("CMakeLists.txt", KeptReason::NotRecorded, false),
            ("Makefile", KeptReason::NotRecorded, false),
        ]
    );
    for path in ["src/utils.cpp", "src/utils.hpp"] {
        let earlier = first.files.iter().find(|file| file.path == path).unwrap();
        assert_eq!(
            std::fs::read_to_string(folder.join(path)).unwrap(),
            earlier.contents
        );
    }
    assert!(written.files.contains(&String::from("README.md")));
    assert!(written.files.contains(&String::from("src/tools.cpp")));
    // The kept files stay listed without a hash, and so are kept again.
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(folder.join(EXPORT_MANIFEST)).unwrap()).unwrap();
    assert_eq!(manifest["formatVersion"], 3);
    let utils = manifest["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == "src/utils.cpp")
        .unwrap();
    assert!(utils.get("sha256").is_none(), "{utils}");
    let again = export::write(&folder, &renamed, ExportMode::Replace).unwrap();
    assert_eq!(kept(&again).len(), 5);
}

/// A manifest is untrusted: a format 3 entry without its object shape, a
/// hash that is not 64 lower-case hex digits, an earlier hash without a
/// hash, an unknown key, a path that is
/// not an export's, a path listed twice (also in another case) or an entry
/// without a path make the folder one Blocks2Cpp did not make.
#[test]
fn a_malformed_manifest_is_not_an_export() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("export");
    let export = two_modules_export(Some("utils"));
    export::write(&folder, &export, ExportMode::New).unwrap();
    let original = std::fs::read_to_string(folder.join(EXPORT_MANIFEST)).unwrap();
    let hash = sha256("x");
    for files in [
        serde_json::json!(["src/main.cpp"]),
        serde_json::json!([{"path": "src/main.cpp", "sha256": hash.to_uppercase()}]),
        serde_json::json!([{"path": "src/main.cpp", "sha256": &hash[1..]}]),
        serde_json::json!([{"path": "src/main.cpp", "sha256": hash, "size": 1}]),
        serde_json::json!([{"path": "src/main.cpp", "previous": hash}]),
        serde_json::json!([{"path": "src/main.cpp", "sha256": hash, "previous": &hash[1..]}]),
        serde_json::json!([{"path": "../main.cpp", "sha256": hash}]),
        serde_json::json!([{"path": "src/main.cpp"}, {"path": "src/main.cpp"}]),
        serde_json::json!([{"path": "src/main.cpp"}, {"path": "src/Main.cpp"}]),
        serde_json::json!([{"sha256": hash}]),
    ] {
        let mut value: serde_json::Value = serde_json::from_str(&original).unwrap();
        value["files"] = files.clone();
        std::fs::write(folder.join(EXPORT_MANIFEST), value.to_string()).unwrap();
        assert_eq!(export::folder_state(&folder), FolderState::Other, "{files}");
        assert!(
            matches!(
                export::write(&folder, &export, ExportMode::Replace),
                Err(export::ExportError::NoEarlierExport)
            ),
            "{files}"
        );
    }
    // A format 2 manifest with format 3 entries is refused too.
    let mut value: serde_json::Value = serde_json::from_str(&original).unwrap();
    value["formatVersion"] = serde_json::Value::from(2);
    std::fs::write(folder.join(EXPORT_MANIFEST), value.to_string()).unwrap();
    assert_eq!(export::folder_state(&folder), FolderState::Other);
    // As written, it is an export.
    std::fs::write(folder.join(EXPORT_MANIFEST), &original).unwrap();
    assert_eq!(export::folder_state(&folder), FolderState::EarlierExport);
}

/// `hello_world` with `count` more, empty modules named by `name`.
fn with_modules(count: usize, name: impl Fn(usize) -> String) -> Vec<u8> {
    let mut project: serde_json::Value =
        serde_json::from_slice(&std::fs::read(repo_root().join("examples/hello_world.b2c")).unwrap())
            .unwrap();
    for index in 0..count {
        project["modules"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "id": format!("mod_{index}"),
                "name": name(index),
                "workspace": {"blocks": []}
            }));
    }
    project.to_string().into_bytes()
}

/// The README says the project needs MSYS2's shell or CMake only when the
/// Makefile's one build command passes 8,000 characters, which cmd.exe
/// (8,191) would refuse: about 75 characters a module.
#[test]
fn the_readme_names_the_shell_only_for_a_command_too_long_for_cmd() {
    let note = "longer than\ncmd.exe takes";
    let readme = |bytes: &[u8]| {
        let export = export::prepare(bytes, 4, "0.0.0-test").unwrap();
        let file = |path: &str| {
            export
                .files
                .iter()
                .find(|file| file.path == path)
                .map(|file| file.contents.clone())
                .unwrap()
        };
        let sources = file("Makefile")
            .lines()
            .find_map(|line| line.strip_prefix("SOURCES := ").map(str::len))
            .unwrap();
        (file("README.md"), sources)
    };
    let (short, _) = readme(&std::fs::read(repo_root().join("examples/hello_world.b2c")).unwrap());
    assert!(!short.contains(note), "{short}");
    // 100 modules with short names: about 1,500 characters of sources.
    let (hundred, sources) = readme(&with_modules(100, |index| format!("m{index:03}")));
    assert!(sources > 1_000, "{sources}");
    assert!(!hundred.contains(note), "{hundred}");
    // 110 modules with 60-character names: past 8,000.
    let (long, sources) = readme(&with_modules(110, |index| {
        format!("m{index:03}{}", "x".repeat(56))
    }));
    assert!(sources > 7_000, "{sources}");
    assert!(long.contains(note), "{long}");
    assert!(long.contains("MSYS2 shell,\nor build with CMake."), "{long}");
}
