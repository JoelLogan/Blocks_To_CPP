//! Access to a top-level block's loose statement stack (spec §5.4,
//! ADR-0011): the statement blocks attached below a block that sits directly
//! on the canvas, saved in the block's `stack` key.

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
