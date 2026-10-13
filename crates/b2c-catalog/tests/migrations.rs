//! Golden-file tests of block upgrades (spec 03 §3.11.3, 05 §5.7,
//! ADR-0015): every migration and every replacement has one.
//!
//! Each `tests/migrations/<case>.b2c` is an old project. The test upgrades it
//! with `upgrade_document_with` and compares the canonical result with
//! `<case>.upgraded.b2c`; run with `B2C_UPDATE_GOLDEN=1` to write the expected
//! files, and review the diff. It also checks what every upgrade promises:
//! the result loads, keeps the security hash and summary, upgrading it again
//! changes nothing, and the catalog has no old or unknown block left in it.
//!
//! The tables are the shipped ones joined with test-only entries, so the
//! mechanism is covered before the first real replacement ships: the retired
//! type `test.say` (replaced by `io.print`, with an older version of its
//! own) and `test.counter`, a copy of `var.change` at version 3 that exists
//! only in this test's catalog. A shipped migration or replacement gets its
//! own case here, upgraded with the same tables.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test helpers fail the test by panicking"
)]

use std::path::{Path, PathBuf};

use b2c_catalog::{
    BLOCK_MIGRATIONS, BLOCK_REPLACEMENTS, BlockMigration, BlockReplacement, BlockUpgrades, Catalog,
    RETIRED_BLOCKS,
};
use b2c_model::{Block, FieldValue};

/// `test.say` v0 → v1: the `LOUD` checkbox went away.
#[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
fn say_without_loud(block: &mut Block) -> Result<(), String> {
    block.fields.remove("LOUD");
    Ok(())
}

/// `test.say` v1 → `io.print` v1: what to say is the first item.
fn say_to_print(block: &mut Block) -> Result<(), String> {
    let what = block.inputs.remove("WHAT").ok_or("the block has no WHAT input")?;
    block.inputs.insert(String::from("ITEM0"), what);
    Ok(())
}

/// `test.counter` v1 → v2: the input `STEP` is now `BY`.
#[allow(clippy::unnecessary_wraps, reason = "the signature every step has")]
fn step_to_by(block: &mut Block) -> Result<(), String> {
    if let Some(step) = block.inputs.remove("STEP") {
        block.inputs.insert(String::from("BY"), step);
    }
    Ok(())
}

/// `test.counter` v2 → v3: the field `VARIABLE` is now `VAR`.
fn variable_to_var(block: &mut Block) -> Result<(), String> {
    match block.fields.remove("VARIABLE") {
        Some(variable @ FieldValue::Ref(_)) => {
            block.fields.insert(String::from("VAR"), variable);
            Ok(())
        }
        Some(_) => Err(String::from("its VARIABLE does not refer to a variable")),
        None => Ok(()),
    }
}

/// The test-only migrations.
const TEST_MIGRATIONS: [BlockMigration; 3] = [
    BlockMigration {
        block: "test.say",
        from: 0,
        apply: say_without_loud,
    },
    BlockMigration {
        block: "test.counter",
        from: 1,
        apply: step_to_by,
    },
    BlockMigration {
        block: "test.counter",
        from: 2,
        apply: variable_to_var,
    },
];

/// The test-only replacements.
const TEST_REPLACEMENTS: [BlockReplacement; 1] = [BlockReplacement {
    from: "test.say",
    version: 1,
    to: "io.print",
    to_version: 1,
    apply: say_to_print,
}];

/// The test-only retired types.
const TEST_RETIRED: [&str; 1] = ["test.say"];

/// The shipped tables and the test-only ones.
struct Tables {
    migrations: Vec<BlockMigration>,
    replacements: Vec<BlockReplacement>,
    retired: Vec<&'static str>,
}

impl Tables {
    fn new() -> Self {
        Self {
            migrations: [BLOCK_MIGRATIONS, &TEST_MIGRATIONS].concat(),
            replacements: [BLOCK_REPLACEMENTS, &TEST_REPLACEMENTS].concat(),
            retired: [RETIRED_BLOCKS, &TEST_RETIRED].concat(),
        }
    }

    fn upgrades(&self) -> BlockUpgrades<'_> {
        BlockUpgrades {
            migrations: &self.migrations,
            replacements: &self.replacements,
            retired: &self.retired,
        }
    }
}

/// The core catalog with the test-only `test.counter` (a copy of
/// `var.change` at version 3).
fn catalog() -> Catalog {
    let mut catalog = b2c_catalog::core_catalog().clone();
    let mut counter = catalog.blocks["var.change"].clone();
    counter.id = String::from("test.counter");
    counter.version = 3;
    catalog.blocks.insert(counter.id.clone(), counter);
    catalog
}

fn cases() -> Vec<PathBuf> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/migrations");
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| {
            path.extension().is_some_and(|e| e == "b2c") && !path.to_string_lossy().ends_with(".upgraded.b2c")
        })
        .collect();
    cases.sort();
    cases
}

#[test]
fn golden_migrations() {
    let update = std::env::var_os("B2C_UPDATE_GOLDEN").is_some();
    let catalog = catalog();
    let tables = Tables::new();
    let cases = cases();
    assert!(cases.len() >= 2, "the cases are missing");
    for case in cases {
        let name = case.file_name().unwrap().to_string_lossy().into_owned();
        let old = b2c_model::load(&std::fs::read(&case).unwrap())
            .unwrap_or_else(|error| panic!("{name} does not load: {:#?}", error.diagnostics));
        let mut upgraded = old.clone();
        let report = b2c_catalog::upgrade_document_with(&mut upgraded, &catalog, &tables.upgrades());
        assert!(report.diagnostics.is_empty(), "{name}: {:#?}", report.diagnostics);
        assert!(report.changed(), "{name}: nothing to upgrade");

        let text = b2c_model::to_canonical_json(&upgraded);
        let expected_path = case.with_extension("upgraded.b2c");
        if update {
            std::fs::write(&expected_path, &text).unwrap();
        } else {
            let expected = std::fs::read_to_string(&expected_path).unwrap_or_else(|error| {
                panic!(
                    "{name}: cannot read {} ({error}); run with B2C_UPDATE_GOLDEN=1 to create it",
                    expected_path.display()
                )
            });
            assert!(
                text == expected,
                "{name}: the upgraded project differs from {} (run with B2C_UPDATE_GOLDEN=1 and review the diff)",
                expected_path.display()
            );
        }

        // It loads, trust sees the same content, and nothing is left to do.
        let reloaded = b2c_model::load(text.as_bytes()).unwrap();
        assert_eq!(b2c_model::to_canonical_json(&reloaded), text, "{name}");
        assert_eq!(
            b2c_model::security_hash(&upgraded),
            b2c_model::security_hash(&old),
            "{name}"
        );
        assert_eq!(
            b2c_model::security_summary(&upgraded),
            b2c_model::security_summary(&old),
            "{name}"
        );
        let mut again = upgraded.clone();
        let report = b2c_catalog::upgrade_document_with(&mut again, &catalog, &tables.upgrades());
        assert!(!report.changed() && report.diagnostics.is_empty(), "{name}");
        assert_eq!(again, upgraded, "{name}: upgrading is idempotent");
        // The catalog has nothing old or unknown left to report.
        let (_, diagnostics) = b2c_catalog::resolve(&upgraded, &catalog);
        let codes: Vec<&str> = diagnostics.iter().map(|d| d.code.0.as_str()).collect();
        assert!(
            !codes
                .iter()
                .any(|code| ["B2C-E0601", "B2C-E0602", "B2C-E0603"].contains(code)),
            "{name}: {codes:?}"
        );
    }
}

#[test]
fn every_test_table_entry_has_a_case() {
    let all: String = cases()
        .iter()
        .map(|case| std::fs::read_to_string(case).unwrap())
        .collect();
    for block in TEST_MIGRATIONS
        .iter()
        .map(|step| step.block)
        .chain(TEST_REPLACEMENTS.iter().map(|r| r.from))
        .chain(BLOCK_MIGRATIONS.iter().map(|step| step.block))
        .chain(BLOCK_REPLACEMENTS.iter().map(|r| r.from))
    {
        assert!(
            all.contains(&format!("\"type\": \"{block}\"")),
            "no golden case uses {block}"
        );
    }
}
