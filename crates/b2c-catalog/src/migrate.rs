//! Block migrations (spec §3.11.3): when a block definition gets a breaking
//! change, its `version` goes up and the catalog ships a pure function that
//! upgrades a block of the previous version (BDM → BDM). The resolve stage
//! runs the chain for every block saved with an older version, so later
//! stages only ever see current blocks.
//!
//! Every block of the core catalog is still at version 1, so the shipped
//! chain is empty; the tests below exercise the machinery with a synthetic
//! chain.

use b2c_model::Block;

/// One step: upgrades blocks of type `block` from version `from` to
/// `from + 1`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BlockMigration {
    /// The block type this step applies to.
    pub(crate) block: &'static str,
    /// The version this step reads; it produces `from + 1`.
    pub(crate) from: u32,
    /// The upgrade. It may change anything except `v`, which the chain sets.
    /// An error is a plain-English reason.
    pub(crate) apply: fn(&mut Block) -> Result<(), String>,
}

/// The block migrations shipped with this catalog.
pub(crate) const BLOCK_MIGRATIONS: &[BlockMigration] = &[];

/// Why a block could not be upgraded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MigrationError {
    /// No step upgrades this version.
    NoPath {
        /// The version that cannot be upgraded.
        from: u32,
    },
    /// A step failed.
    Failed {
        /// The version the failing step reads.
        from: u32,
        /// Why it failed.
        reason: String,
    },
}

/// Upgrades `block` to version `to` with `chain`. The block is changed only
/// when every step succeeds.
///
/// # Errors
/// Fails when a step is missing or reports an error; downgrades
/// (`block.v > to`) do not exist.
pub(crate) fn upgrade(block: &mut Block, to: u32, chain: &[BlockMigration]) -> Result<(), MigrationError> {
    if block.v > to {
        return Err(MigrationError::NoPath { from: block.v });
    }
    let mut upgraded = block.clone();
    while upgraded.v < to {
        let from = upgraded.v;
        let step = chain
            .iter()
            .find(|step| step.block == upgraded.block_type && step.from == from)
            .ok_or(MigrationError::NoPath { from })?;
        (step.apply)(&mut upgraded).map_err(|reason| MigrationError::Failed { from, reason })?;
        // The chain owns the version and the type, so a step cannot loop or
        // turn the block into something else.
        upgraded.v = from + 1;
        upgraded.block_type.clone_from(&block.block_type);
    }
    *block = upgraded;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use b2c_ir::BlockId;
    use b2c_model::FieldValue;

    use super::*;

    /// A synthetic v1 → v2 step: the field `OLD` was renamed to `NEW`.
    fn rename_field(block: &mut Block) -> Result<(), String> {
        let value = block.fields.remove("OLD").ok_or("the block has no OLD field")?;
        block.fields.insert("NEW".into(), value);
        Ok(())
    }

    /// A synthetic v2 → v3 step that sets a flag.
    #[allow(clippy::unnecessary_wraps, reason = "the signature every migration has")]
    fn add_flag(block: &mut Block) -> Result<(), String> {
        block.fields.insert("FLAG".into(), FieldValue::Bool(true));
        Ok(())
    }

    /// A step that tries to change the version and the type itself.
    #[allow(clippy::unnecessary_wraps, reason = "the signature every migration has")]
    fn meddle(block: &mut Block) -> Result<(), String> {
        block.v = 1;
        block.block_type = "other.block".into();
        Ok(())
    }

    const CHAIN: &[BlockMigration] = &[
        BlockMigration {
            block: "test.block",
            from: 2,
            apply: add_flag,
        },
        BlockMigration {
            block: "test.block",
            from: 1,
            apply: rename_field,
        },
        BlockMigration {
            block: "other.block",
            from: 1,
            apply: add_flag,
        },
    ];

    fn block(v: u32) -> Block {
        Block {
            id: BlockId::new("b1").unwrap(),
            block_type: "test.block".into(),
            v,
            x: None,
            y: None,
            collapsed: false,
            disabled: false,
            comment: None,
            extra: BTreeMap::new(),
            fields: [("OLD".to_owned(), FieldValue::Text("x".into()))].into(),
            inputs: BTreeMap::new(),
            statements: BTreeMap::new(),
            stack: Vec::new(),
        }
    }

    #[test]
    fn chain_upgrades_step_by_step() {
        let mut upgraded = block(1);
        upgrade(&mut upgraded, 3, CHAIN).unwrap();
        assert_eq!(upgraded.v, 3);
        assert_eq!(upgraded.fields["NEW"], FieldValue::Text("x".into()));
        assert_eq!(upgraded.fields["FLAG"], FieldValue::Bool(true));
        assert!(!upgraded.fields.contains_key("OLD"));

        let mut current = block(3);
        upgrade(&mut current, 3, CHAIN).unwrap();
        assert_eq!(current, block(3));
    }

    #[test]
    fn failures_leave_the_block_unchanged() {
        let mut newer = block(4);
        assert_eq!(
            upgrade(&mut newer, 3, CHAIN),
            Err(MigrationError::NoPath { from: 4 })
        );
        let mut ancient = block(0);
        assert_eq!(
            upgrade(&mut ancient, 3, CHAIN),
            Err(MigrationError::NoPath { from: 0 })
        );
        let mut broken = block(1);
        broken.fields.clear();
        let before = broken.clone();
        assert_eq!(
            upgrade(&mut broken, 3, CHAIN),
            Err(MigrationError::Failed {
                from: 1,
                reason: "the block has no OLD field".into()
            })
        );
        assert_eq!(broken, before);
        // Steps of other block types never apply.
        let mut other = block(1);
        other.block_type = "third.block".into();
        assert_eq!(
            upgrade(&mut other, 2, CHAIN),
            Err(MigrationError::NoPath { from: 1 })
        );
    }

    #[test]
    fn steps_cannot_change_the_version_or_type() {
        let chain = [BlockMigration {
            block: "test.block",
            from: 1,
            apply: meddle,
        }];
        let mut upgraded = block(1);
        upgrade(&mut upgraded, 2, &chain).unwrap();
        assert_eq!(upgraded.v, 2);
        assert_eq!(upgraded.block_type, "test.block");
    }

    #[test]
    fn shipped_chain_is_consistent() {
        let catalog = crate::core_catalog();
        for (index, step) in BLOCK_MIGRATIONS.iter().enumerate() {
            let def = &catalog.blocks[step.block];
            assert!(step.from >= 1 && step.from < def.version, "{}", step.block);
            assert!(
                BLOCK_MIGRATIONS
                    .iter()
                    .skip(index + 1)
                    .all(|s| (s.block, s.from) != (step.block, step.from))
            );
        }
    }
}
