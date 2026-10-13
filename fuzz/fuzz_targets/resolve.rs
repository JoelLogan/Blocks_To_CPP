//! Fuzzes block upgrades and the resolve stage with every document the
//! loader accepts: neither may panic; an upgrade keeps the security hash and
//! summary, gives a document that loads, and is idempotent; and resolving
//! its own output must change nothing more.
//!
//! The shipped upgrade tables hold one replacement (`var.update` by
//! `var.change`) and no migration, so the upgrade also runs with a fuzz-only
//! table: the retired type `fuzz.old` at version 1 is replaced by
//! `io.print`, its `WHAT` input becoming `ITEM0` (and the replacement fails
//! when the block has a `FAIL` field), and `io.print` blocks of version 0
//! are moved to version 1. That exercises the walk, the stand-ins for nested
//! blocks and every check of the result.

#![no_main]

use b2c_catalog::{BlockMigration, BlockReplacement, BlockUpgrades};
use b2c_model::{Block, Document};
use libfuzzer_sys::fuzz_target;

/// `fuzz.old` → `io.print`: what to print moves to the first item.
fn old_to_print(block: &mut Block) -> Result<(), String> {
    if let Some(what) = block.inputs.remove("WHAT") {
        block.inputs.insert(String::from("ITEM0"), what);
    }
    if block.fields.contains_key("FAIL") {
        return Err(String::from("asked to fail"));
    }
    Ok(())
}

/// `io.print` v0 → v1: no change.
#[allow(clippy::unnecessary_wraps)]
fn keep(_: &mut Block) -> Result<(), String> {
    Ok(())
}

const FUZZ_UPGRADES: BlockUpgrades<'static> = BlockUpgrades {
    migrations: &[BlockMigration {
        block: "io.print",
        from: 0,
        apply: keep,
    }],
    replacements: &[BlockReplacement {
        from: "fuzz.old",
        version: 1,
        to: "io.print",
        to_version: 1,
        apply: old_to_print,
    }],
    retired: &["fuzz.old"],
};

/// Upgrades a copy of `document` and checks what every upgrade promises.
fn check_upgrade(document: &Document, upgrades: &BlockUpgrades<'_>) -> Document {
    let catalog = b2c_catalog::core_catalog();
    let mut upgraded = document.clone();
    let report = b2c_catalog::upgrade_document_with(&mut upgraded, catalog, upgrades);
    assert_eq!(
        b2c_model::security_hash(&upgraded),
        b2c_model::security_hash(document)
    );
    assert_eq!(
        b2c_model::security_summary(&upgraded),
        b2c_model::security_summary(document)
    );
    if report.changed() {
        let text = b2c_model::to_canonical_json(&upgraded);
        assert!(b2c_model::load(text.as_bytes()).is_ok());
    } else {
        assert_eq!(&upgraded, document);
    }
    let mut again = upgraded.clone();
    let second = b2c_catalog::upgrade_document_with(&mut again, catalog, upgrades);
    assert!(!second.changed());
    assert_eq!(again, upgraded);
    upgraded
}

fuzz_target!(|data: &[u8]| {
    if let Ok(document) = b2c_model::load(data) {
        let catalog = b2c_catalog::core_catalog();
        check_upgrade(&document, &BlockUpgrades::SHIPPED);
        let upgraded = check_upgrade(&document, &FUZZ_UPGRADES);
        for document in [&document, &upgraded] {
            let (completed, _) = b2c_catalog::resolve(document, catalog);
            let (again, _) = b2c_catalog::resolve(&completed, catalog);
            assert_eq!(again, completed);
        }
    }
});
