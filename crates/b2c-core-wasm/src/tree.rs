//! Walking block trees without recursion.
//!
//! A loaded document nests at most about 40 blocks deep, but statement
//! lists and stacks can hold up to [`b2c_model::limits::MAX_BLOCKS`]
//! blocks and documents built in memory have no depth limit, so these walks
//! keep their own stack on the heap.
//!
//! Order: parents before children (pre-order), the given blocks in order;
//! inside a block, its value inputs (by input name), then its statement
//! lists (by input name, each in order), then its loose `stack` (in order).
//! This is the order of `b2c_model`'s own walks.

use b2c_model::{Block, Input};

/// The direct children of a block, in walk order.
fn children(block: &Block) -> impl DoubleEndedIterator<Item = &Block> {
    let inputs = block.inputs.values().filter_map(|input| match input {
        Input::Block(nested) => Some(&*nested.block),
        Input::Expr(_) => None,
    });
    let statements = block.statements.values().flatten();
    inputs.chain(statements).chain(block.stack.iter())
}

/// Calls `visit` for each block in `roots` and every block nested in them.
pub(crate) fn walk<'a>(roots: impl IntoIterator<Item = &'a Block>, mut visit: impl FnMut(&'a Block)) {
    let mut pending: Vec<&'a Block> = roots.into_iter().collect();
    pending.reverse();
    while let Some(block) = pending.pop() {
        visit(block);
        pending.extend(children(block).rev());
    }
}

/// Like [`walk`], but `visit` also returns whether to look inside the block;
/// `false` skips its children (and their children).
pub(crate) fn walk_pruned<'a>(
    roots: impl IntoIterator<Item = &'a Block>,
    mut visit: impl FnMut(&'a Block) -> bool,
) {
    let mut pending: Vec<&'a Block> = roots.into_iter().collect();
    pending.reverse();
    while let Some(block) = pending.pop() {
        if visit(block) {
            pending.extend(children(block).rev());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Vec<Block> {
        serde_json::from_value(serde_json::json!([
            {"id": "a", "type": "t", "v": 1,
             "inputs": {"Z": {"block": {"id": "a_z", "type": "t", "v": 1}},
                        "A": {"block": {"id": "a_a", "type": "t", "v": 1}},
                        "E": {"expr": []}},
             "statements": {"BODY": [{"id": "a_b1", "type": "t", "v": 1,
                                      "statements": {"DO": [{"id": "a_b1_d", "type": "t", "v": 1}]}},
                                     {"id": "a_b2", "type": "t", "v": 1}],
                            "ALT": [{"id": "a_alt", "type": "t", "v": 1}]},
             "stack": [{"id": "a_s", "type": "t", "v": 1}]},
            {"id": "b", "type": "t", "v": 1}
        ]))
        .unwrap()
    }

    #[test]
    fn pre_order_inputs_then_lists_then_stack() {
        let blocks = tree();
        let mut seen = Vec::new();
        walk(&blocks, |block| seen.push(block.id.as_str().to_owned()));
        assert_eq!(
            seen,
            ["a", "a_a", "a_z", "a_alt", "a_b1", "a_b1_d", "a_b2", "a_s", "b"]
        );
    }

    #[test]
    fn pruning_skips_the_subtree() {
        let blocks = tree();
        let mut seen = Vec::new();
        walk_pruned(&blocks, |block| {
            seen.push(block.id.as_str().to_owned());
            block.id.as_str() != "a_b1"
        });
        assert_eq!(seen, ["a", "a_a", "a_z", "a_alt", "a_b1", "a_b2", "a_s", "b"]);
    }

    #[test]
    fn deep_trees_do_not_recurse() {
        let mut block: Block =
            serde_json::from_value(serde_json::json!({"id": "leaf", "type": "t", "v": 1})).unwrap();
        for _ in 0..100_000 {
            let mut parent: Block =
                serde_json::from_value(serde_json::json!({"id": "n", "type": "t", "v": 1})).unwrap();
            parent.statements.insert(String::from("BODY"), vec![block]);
            block = parent;
        }
        let mut count = 0;
        walk(std::slice::from_ref(&block), |_| count += 1);
        assert_eq!(count, 100_001);
        // Take the tree apart level by level: dropping it whole would recurse.
        let mut pending = vec![block];
        while let Some(mut next) = pending.pop() {
            for (_, list) in std::mem::take(&mut next.statements) {
                pending.extend(list);
            }
        }
    }
}
