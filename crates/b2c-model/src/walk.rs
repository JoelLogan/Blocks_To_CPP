//! Visiting every block of a block tree without recursion.
//!
//! A document that was built in memory (not loaded) has no depth limit, and
//! statement lists and stacks can hold up to [`crate::limits::MAX_BLOCKS`]
//! blocks, so these walks keep their own stack on the heap instead of
//! recursing.
//!
//! Order: parents before children (pre-order), the given blocks in order;
//! inside a block, its value inputs (by input name), then its statement
//! lists (by input name, each in order), then its `stack` (in order).

use crate::document::{Block, Document, Input};

/// Calls `visit` for each block in `blocks` and every block nested in them.
pub(crate) fn blocks<'a>(blocks: &'a [Block], mut visit: impl FnMut(&'a Block)) {
    let mut pending: Vec<&'a Block> = blocks.iter().rev().collect();
    while let Some(block) = pending.pop() {
        visit(block);
        for child in block.stack.iter().rev() {
            pending.push(child);
        }
        for list in block.statements.values().rev() {
            pending.extend(list.iter().rev());
        }
        for input in block.inputs.values().rev() {
            if let Input::Block(nested) = input {
                pending.push(&nested.block);
            }
        }
    }
}

/// Like [`blocks`], with mutable access. `visit` runs before the block's
/// children are visited, so it may change them.
pub(crate) fn blocks_mut(blocks: &mut [Block], mut visit: impl FnMut(&mut Block)) {
    let mut pending: Vec<&mut Block> = blocks.iter_mut().rev().collect();
    while let Some(block) = pending.pop() {
        visit(&mut *block);
        let Block {
            inputs,
            statements,
            stack,
            ..
        } = block;
        for child in stack.iter_mut().rev() {
            pending.push(child);
        }
        for list in statements.values_mut().rev() {
            pending.extend(list.iter_mut().rev());
        }
        for input in inputs.values_mut().rev() {
            if let Input::Block(nested) = input {
                pending.push(&mut nested.block);
            }
        }
    }
}

/// Calls `visit` for every block of a document in canonical order: modules
/// in file order, each module's top-level blocks sorted by ID (as saved),
/// and their children as in [`blocks`].
pub(crate) fn document<'a>(document: &'a Document, mut visit: impl FnMut(&'a Block)) {
    for module in &document.modules {
        let mut top_level: Vec<&Block> = module.workspace.blocks.iter().collect();
        top_level.sort_by(|a, b| a.id.cmp(&b.id));
        for block in top_level {
            blocks(std::slice::from_ref(block), &mut visit);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use b2c_ir::BlockId;

    use super::*;
    use crate::document::BlockInput;

    fn block(id: &str) -> Block {
        Block {
            id: BlockId::new(id).unwrap(),
            block_type: "t".into(),
            v: 1,
            x: None,
            y: None,
            collapsed: false,
            disabled: false,
            comment: None,
            extra: BTreeMap::new(),
            fields: BTreeMap::new(),
            inputs: BTreeMap::new(),
            statements: BTreeMap::new(),
            stack: Vec::new(),
        }
    }

    /// `a` with input `b` (with input `c`), statements `S1: [d, e]` and
    /// `S0: [f]`, and stack `[g]`; then `h`.
    fn tree() -> Vec<Block> {
        let mut b = block("b");
        b.inputs.insert(
            "IN".into(),
            Input::Block(BlockInput {
                block: Box::new(block("c")),
            }),
        );
        let mut a = block("a");
        a.inputs
            .insert("IN".into(), Input::Block(BlockInput { block: Box::new(b) }));
        a.statements.insert("S1".into(), vec![block("d"), block("e")]);
        a.statements.insert("S0".into(), vec![block("f")]);
        a.stack.push(block("g"));
        vec![a, block("h")]
    }

    #[test]
    fn pre_order_inputs_statements_stack() {
        let tree = tree();
        let mut seen = Vec::new();
        blocks(&tree, |b| seen.push(b.id.as_str().to_owned()));
        assert_eq!(seen, ["a", "b", "c", "f", "d", "e", "g", "h"]);
        let mut tree = tree;
        let mut seen_mut = Vec::new();
        blocks_mut(&mut tree, |b| {
            seen_mut.push(b.id.as_str().to_owned());
            b.v = 2;
        });
        assert_eq!(seen_mut, seen);
        let mut all_changed = true;
        blocks(&tree, |b| all_changed &= b.v == 2);
        assert!(all_changed);
    }

    #[test]
    fn long_and_deep_trees_need_no_recursion() {
        // 200,000 levels of nesting would overflow any recursive walk.
        let mut deep = block("leaf");
        for _ in 0..200_000 {
            let mut parent = block("p");
            parent.statements.insert("BODY".into(), vec![deep]);
            deep = parent;
        }
        let mut count = 0usize;
        blocks(std::slice::from_ref(&deep), |_| count += 1);
        assert_eq!(count, 200_001);
        // Dropping such a tree recurses in the compiler's drop glue, so
        // take it apart level by level.
        let mut next = Some(deep);
        while let Some(mut node) = next {
            next = node.statements.remove("BODY").and_then(|mut list| list.pop());
        }
    }
}
