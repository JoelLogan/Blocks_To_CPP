//! Access to a top-level block's loose statement stack (spec §5.4,
//! ADR-0011): the statement blocks attached below a block that sits directly
//! on the canvas, saved in the block's `stack` key.
//!
//! Only a statement heads a stack. The definitions in a namespace's mouth
//! follow one another in the mouth's statement list (05 §5.4,
//! w3-modules.md §2.12), never in a `stack`: a definition's connection below
//! it exists only inside a mouth, so a block stacked below a top-level
//! definition is `B2C-E0604`, as before namespaces.

use b2c_model::Block;

/// Takes the loose stack out of a top-level block, so it can be resolved
/// next to its head; [`restore`] puts it back.
pub(crate) fn take(block: &mut Block) -> Vec<Block> {
    std::mem::take(&mut block.stack)
}

/// Puts a stack taken with [`take`] back into its head block.
pub(crate) fn restore(block: &mut Block, stack: Vec<Block>) {
    block.stack = stack;
}
