//! The diagnostic DTO is byte-identical in JSON to `b2c_ir::Diagnostic`, which is
//! also the CLI's `--format json` shape, so the app and the CLI report the same
//! JSON (`docs/spec/06-compiler-pipeline.md` §6.12).

// Test code: unwrap/expect/panic are fine here.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use b2c_ipc::diag::Diagnostic;
use b2c_ir::{BlockId, DiagSource, Location, ModuleId, Part};

fn assert_identical(shared: &b2c_ir::Diagnostic) {
    let dto = Diagnostic::from(shared);
    let shared_json = serde_json::to_string(shared).unwrap();
    let dto_json = serde_json::to_string(&dto).unwrap();
    assert_eq!(dto_json, shared_json);
    assert_eq!(
        serde_json::to_string_pretty(&dto).unwrap(),
        serde_json::to_string_pretty(shared).unwrap()
    );
    // And back: the DTO reads its own JSON and the shared type's.
    assert_eq!(serde_json::from_str::<Diagnostic>(&shared_json).unwrap(), dto);
    let shared_again: b2c_ir::Diagnostic = serde_json::from_str(&dto_json).unwrap();
    assert_eq!(&shared_again, shared);
}

/// The loader diagnostics of every file in the malicious-project suite, which the
/// CLI reports with `b2c check --format json`.
#[test]
fn loader_diagnostics_of_the_security_suite() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/security/projects");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "b2c"))
        .collect();
    files.sort();
    assert!(files.len() > 50, "the suite has its files");
    let mut compared = 0;
    for file in files {
        let bytes = std::fs::read(&file).unwrap();
        if let Err(error) = b2c_model::load(&bytes) {
            for diagnostic in &error.diagnostics {
                assert_identical(diagnostic);
                compared += 1;
            }
        }
    }
    assert!(compared > 40, "compared {compared} diagnostics");
}

/// Every shape: each part kind, related locations, raw compiler text, every
/// severity and source.
#[test]
fn every_shape() {
    let module = ModuleId::new("mod_main").unwrap();
    let block = BlockId::new("b042").unwrap();
    let parts = [
        Part::Whole,
        Part::Field { name: "NAME".into() },
        Part::Input { name: "COND0".into() },
        Part::Tokens {
            input: "EXPR".into(),
            start: 2,
            end: 5,
        },
    ];
    let sources = [
        DiagSource::Loader,
        DiagSource::Catalog,
        DiagSource::Analyser,
        DiagSource::Generator,
        DiagSource::Toolchain,
        DiagSource::Compiler,
        DiagSource::Linker,
        DiagSource::Runtime,
    ];
    for (index, part) in parts.into_iter().enumerate() {
        for source in sources {
            let location = Location::block(Some(module.clone()), block.clone()).with_part(part.clone());
            let mut diagnostic = match index % 3 {
                0 => b2c_ir::Diagnostic::error("B2C-E0201", source, location, "“Quoted” <b>text</b>\n"),
                1 => b2c_ir::Diagnostic::warning("B2C-W0501", source, location, "a warning"),
                _ => b2c_ir::Diagnostic::info("B2C-I0513", source, location, "info"),
            };
            assert_identical(&diagnostic);
            diagnostic = diagnostic
                .with_related(Location::project(), "the project")
                .with_related(Location::block(None, block.clone()), "this block");
            diagnostic.raw = Some("main.cpp:3:5: error: expected ';'\n".into());
            assert_identical(&diagnostic);
        }
    }
    assert_eq!(
        serde_json::to_value(Diagnostic::from(&b2c_ir::Diagnostic::warning(
            "B2C-W0502",
            DiagSource::Analyser,
            Location::project(),
            "m"
        )))
        .unwrap()["severity"],
        "warning"
    );
}
