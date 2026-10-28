//! Export as a C++ project (`docs/spec/06-compiler-pipeline.md` §6.11):
//! exported examples build with `make` and with CMake, without Blocks2Cpp,
//! and the programs do what their golden run data says (the same check as
//! `golden.rs`). The builds need g++ (required with `B2C_REQUIRE_GXX`), and
//! `make` or `cmake` on `PATH`; a tool that is missing skips its part. CMake
//! is used on Linux only (on Windows it would choose MSVC).

// Test helpers fail the test by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::path::{Path, PathBuf};

use b2c_build::export::{self, EXPORT_MANIFEST, ExportMode, FolderState};
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

fn exported(name: &str, folder: &Path) -> export::Export {
    let bytes = std::fs::read(repo_root().join("examples").join(format!("{name}.b2c"))).unwrap();
    let export = export::prepare(&bytes, 4, "0.0.0-test").unwrap();
    export::write(folder, &export, ExportMode::New).unwrap();
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
    if common::gxx().is_none() || !available("make") {
        return;
    }
    for name in EXAMPLES {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("export");
        let export = exported(name, &folder);
        // Each configuration builds a program file of its own: `make` and
        // then `make CONFIG=release` compiles again.
        for config in ["debug", "release"] {
            let output = tool("make")
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
        if available("make") {
            run(
                &tool("make").arg("-C").arg(&folder).output().unwrap(),
                &format!("make {module} {standard}"),
            );
            let built = folder.join(format!("{module}-debug"));
            check("hello_world", &built);
            run(
                &tool("make").arg("-C").arg(&folder).arg("clean").output().unwrap(),
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
    if !available("make") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("export");
    export::write(&folder, &export, ExportMode::New).unwrap();
    for (args, out) in [
        (&["SANITIZE=no"][..], "main-debug-nosanitize"),
        (&["HARDEN=off"][..], "main-debug-noharden"),
        (&["SANITIZE=0", "HARDEN=0"][..], "main-debug-nosanitize-noharden"),
        (&[][..], "main-debug"),
    ] {
        let output = tool("make")
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
        .push(serde_json::Value::from("src/old.cpp"));
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
    let written = export::write(&folder, &export, ExportMode::Replace).unwrap();
    assert_eq!(written.last().map(String::as_str), Some(EXPORT_MANIFEST));
    assert_eq!(std::fs::read_to_string(folder.join("notes.txt")).unwrap(), "mine");
    assert_eq!(
        std::fs::read_to_string(folder.join("src/mine.cpp")).unwrap(),
        "// mine"
    );
    assert_ne!(
        std::fs::read_to_string(folder.join("Makefile")).unwrap(),
        "changed"
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
        .retain(|file| file != "Makefile");
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
