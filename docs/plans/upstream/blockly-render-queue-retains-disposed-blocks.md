# The render queue keeps every block disposed after it was queued as a top block

**Project:** google/blockly (core)
**Version:** 12.5.1 (later versions not checked)
**Severity:** memory leak: every deleted block that was in a stack, and every hidden statement
insertion marker, stays reachable for the life of the page
**Found by:** Blocks2Cpp (a Blockly 12 editor with the Zelos renderer), while measuring drag
previews

## Summary

`core/render_management.ts` keeps the blocks that wait to be drawn in a module-level
`Set<BlockSvg>` (`rootBlocks`). `queueRender(block)` adds the block's current root. `doRenders`
draws only the roots for which `shouldRenderRootBlock` holds (`!block.isDisposed() &&
!block.getParent()`) and removes only the blocks it drew (`dequeueBlock`). A root that is
disposed before the next `doRenders` is filtered out and never removed, so the Set keeps it (and
with it the block's SVG subtree) for good.

A root that gets a parent before the next `doRenders` is filtered out too, but it does not stay:
`RenderedConnection.connect_` queues the child after connecting it, which queues its new root, and
when `doRenders` draws that root, `dequeueBlock` walks down through `getChildren` and deletes the
re-parented block from `rootBlocks` as well. (Measured as below: a print queued as a root,
connected into a repeat before the frame and disposed with its parent after it, is collected like
a block that was never connected, 1 of 20 alive.) Only a re-parented block that is disposed,
with its new root, before the frame stays.

Disconnecting a block always queues it as a root: `RenderedConnection.disconnectInternal` calls
`child.queueRender()` after the child has become a top block. So every block that is unplugged and
disposed in the same task stays in `rootBlocks`:

* a block deleted while it was in a stack (`BlockSvg.dispose` unplugs it first);
* every statement insertion marker that `InsertionMarkerPreviewer.hideInsertionMarker` hides: it
  unplugs the marker (`marker.unplug(true)`) and disposes it right away.

## Steps to reproduce

With Node's `--expose-gc` (measured under Vitest with happy-dom, with Blocks2Cpp's own statement
blocks, a C-shaped loop and a print; the snippet uses Blockly's sample blocks in their place, and a
heap snapshot in a browser should show the same):

```ts
const parent = Blockly.serialization.blocks.append({type: 'controls_repeat_ext'}, ws);
const refs = [];
for (let i = 0; i < 20; i++) {
  const child = ws.newBlock('text_print');
  child.initSvg();
  child.render();
  parent.getInput('DO').connection.connect(child.previousConnection);
  await Blockly.renderManagement.finishQueuedRenders();
  child.dispose(true);                    // as a user deletes it
  refs.push(new WeakRef(child));
}
await Blockly.renderManagement.finishQueuedRenders();
globalThis.gc();
console.log(refs.filter((r) => r.deref() !== undefined).length);
```

**Expected:** 0 (or close to it): nothing refers to the deleted blocks.

**Actual:** 20. The same blocks deleted without ever having been connected are collected (1 of 20
was still alive). Twenty statement insertion markers shown and hidden by
`InsertionMarkerPreviewer` during a drag: 20 of 20 stay alive.

## Cause

`doRenders` filters disposed roots out of the list it draws, but removes from `rootBlocks` only
what it drew and the blocks below it (a re-parented root goes that way, below its new root):

```ts
const blocks = [...rootBlocks]
  .filter(shouldRenderRootBlock)
  .filter((b) => workspaces.has(b.workspace));
...
for (const block of blocks) {
  dequeueBlock(block);
}
```

## Suggested fix

Remove the disposed roots that `doRenders` skips from `rootBlocks` (`rootBlocks.delete(block)`),
for example after the drawing loop; or let `BlockSvg.dispose` remove the block from the queue.

Calling `dequeueBlock` on every root that fails `shouldRenderRootBlock` before the drawing loop
would be wrong for a re-parented root: it would also clear the dirty marks of that block and the
blocks below it, which its new root's `renderBlock` still needs (`renderBlock` returns early for a
block that is not dirty), so the re-parented block would not be drawn.

## Workaround

Blocks2Cpp's own previewer (`packages/blockly-ext/src/checker/previewer.ts`, `restore`) draws the
canvas (`renderManagement.triggerQueuedRenders(workspace)`) after it unplugs a marker and before it
disposes it, so the marker is drawn and dequeued while it is still a live top block; its markers
are then collected (0 of 20 alive). Blockly's own `InsertionMarkerPreviewer` markers and deleted
blocks are not covered: that needs the fix above.
