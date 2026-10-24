//! Fuzzes the clipboard loader (spec §5.12) with arbitrary bytes: it must
//! never panic, overflow the stack or allocate without bound, and every
//! rejection must carry at least one diagnostic. Whatever it accepts must
//! save to canonical text that loads back to the same payload and saves to
//! the same bytes, and must survive a paste: fresh IDs from `remap_ids`
//! keep the payload valid and leave no old block ID behind.

#![no_main]

use std::collections::BTreeSet;

use b2c_model::{Block, Clipboard, SeededIds, load_clipboard, remap_ids, to_canonical_clipboard_json};
use libfuzzer_sys::fuzz_target;

/// Every block ID in a tree, without recursion.
fn block_ids(blocks: &[Block]) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut pending: Vec<&Block> = blocks.iter().collect();
    while let Some(block) = pending.pop() {
        ids.insert(block.id.as_str().to_owned());
        pending.extend(&block.stack);
        pending.extend(block.statements.values().flatten());
        for input in block.inputs.values() {
            if let b2c_model::Input::Block(nested) = input {
                pending.push(&nested.block);
            }
        }
    }
    ids
}

fuzz_target!(|data: &[u8]| {
    let clipboard = match load_clipboard(data) {
        Ok(clipboard) => clipboard,
        Err(error) => {
            assert!(!error.diagnostics.is_empty());
            return;
        }
    };
    let saved = to_canonical_clipboard_json(&clipboard);
    let reloaded = match load_clipboard(saved.as_bytes()) {
        Ok(reloaded) => reloaded,
        Err(error) => panic!("the canonical form does not load: {:?}", error.diagnostics),
    };
    assert_eq!(reloaded, clipboard);
    assert_eq!(to_canonical_clipboard_json(&reloaded), saved);

    // Paste: the old IDs are taken, as they would be in the source document.
    let old = block_ids(&clipboard.blocks);
    let mut blocks = clipboard.blocks.clone();
    let seed: [u8; 32] = std::array::from_fn(|i| data.get(i).copied().unwrap_or(0));
    if let Err(error) = remap_ids(&mut blocks, &old, &mut SeededIds::new(seed)) {
        panic!("a valid payload cannot be remapped: {error}");
    }
    let new = block_ids(&blocks);
    assert_eq!(new.len(), old.len());
    assert!(new.is_disjoint(&old));
    let pasted = Clipboard { blocks, ..clipboard };
    if let Err(error) = load_clipboard(to_canonical_clipboard_json(&pasted).as_bytes()) {
        panic!("the remapped payload does not load: {:?}", error.diagnostics);
    }
});
