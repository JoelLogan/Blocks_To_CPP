//! Block migrations (spec §3.11.3): when a block definition gets a breaking
//! change, its `version` goes up and the catalog ships a pure function that
//! upgrades a block of the previous version (BDM → BDM).
//!
//! The upgrade walk ([`crate::upgrade`]) runs the chain for every block
//! saved with an older version, whenever a document, recovery snapshot or
//! clipboard payload is loaded and again in the resolve stage, so the editor
//! and the later stages only ever see current blocks (05 §5.7, ADR-0015).
//!
//! Every block of the core catalog is still at version 1, so the shipped
//! chain is empty; the upgrade module's tests exercise the machinery with
//! synthetic chains.

use b2c_model::Block;

/// One step: upgrades blocks of type `block` from version `from` to
/// `from + 1`.
///
/// The step may rewrite the block's own fields, value inputs (including
/// renaming one, with the block in it), statement inputs and `extra`
/// settings. The upgrade walk owns everything else: it sets the type and the
/// version, keeps the block's ID, position, comment and flags, keeps its
/// loose stack and nested blocks, and refuses a step that adds or removes a
/// nested block, adds expression tokens, changes the IDs of the symbols the
/// block declares or breaks a limit of 05 §5.6 (`B2C-E0603`, the block is
/// left as it was).
#[derive(Debug, Clone, Copy)]
pub struct BlockMigration {
    /// The block type this step applies to.
    pub block: &'static str,
    /// The version this step reads; it produces `from + 1`.
    pub from: u32,
    /// The upgrade. An error is a plain-English reason, shown after "…
    /// failed: ".
    pub apply: fn(&mut Block) -> Result<(), String>,
}

/// The block migrations shipped with this catalog. Each one has a golden-file
/// test in `crates/b2c-catalog/tests/migrations/`.
pub const BLOCK_MIGRATIONS: &[BlockMigration] = &[];
