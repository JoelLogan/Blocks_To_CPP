//! Access to a top-level block's loose statement stack (spec §5.4,
//! ADR-0011): the statement blocks attached below a block that sits directly
//! on the canvas, saved in the block's `stack` key.
//!
//! SEAM (M2 amendment A1): the `stack` key is parsed, limited, saved and
//! hashed by `b2c-model` (package w1-model-security-clipboard, built in
//! parallel), which adds `pub stack: Vec<Block>` to [`b2c_model::Block`].
//! Until that field exists, a block has no stack, and these two functions are
//! the only code that touches it. Once it is merged they become
//! `std::mem::take(&mut block.stack)` and `block.stack = stack`; the resolve
//! stage around them already checks stacks (see `Resolver::top_level`).

use b2c_model::Block;

/// Takes the loose stack out of a top-level block, so it can be resolved
/// next to its head; [`restore`] puts it back.
pub(crate) fn take(block: &mut Block) -> Vec<Block> {
    let _ = block;
    Vec::new()
}

/// Puts a stack taken with [`take`] back into its head block.
pub(crate) fn restore(block: &mut Block, stack: Vec<Block>) {
    let _ = (block, stack);
}
