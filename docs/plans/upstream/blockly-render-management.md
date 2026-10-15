# Render management redraws every statement above an edited one, and getDescendants is quadratic

**Project:** google/blockly (core)
**Version:** 12.5.1 (later versions not checked)
**Severity:** performance: an edit at the end of a long statement list redraws every statement above
it, so each edit costs time linear in the list's length; `Block.getDescendants` takes time quadratic
in a stack's length, with recursion as deep as the stack
**Found by:** Blocks2Cpp (a Blockly 12 editor with the Zelos renderer), while measuring edits at
1,000 blocks
**Patch:** [`blockly-render-management.patch`](blockly-render-management.patch) (against the 12.5.1
sources) and, as measured, [`blockly-render-management.bundle.patch`](blockly-render-management.bundle.patch)
(the same change in the npm package's `blockly_compressed.js`)

## Summary

`core/render_management.ts` queues a block for rendering together with all of its ancestors
(`queueBlock`), and `doRenders` redraws every queued block (`renderBlock` calls
`renderEfficiently()` on each). In a stack of statements each block is the parent of the block
below it (its next block), so a change to the 500th statement of a stack, or a block connected
below it, redraws every statement above it, although none of them changes shape: a block's shape
does not depend on the blocks below its next connection. Only the blocks that contain the changed
block in one of their inputs (a C-block around the stack, the block a value is plugged into) can
change size.

After drawing, `doRenders` updates the locations of the components of every block of each queued
stack (`BlockSvg.updateComponentLocations` walks the whole tree), and each connection's `moveTo`
files it again in the connection database (the `moved` check is commented out, TODO #6922).

`Block.getDescendants` builds its list recursively, copying each child's list into its parent's
(`blocks.push(...child.getDescendants(ordered))`). A stack of n statements nests n levels deep (the
next block is a child), so the copying takes time quadratic in n and the recursion is n calls deep.
Drag starts, the event utilities, `workspace.render()`, `getAllBlocks(true)` and
`getBlockTypeCounts` call it.

## Steps to reproduce

```ts
// 500 statements in one stack (Blockly's sample blocks; any statement block will do).
const blocks = [];
for (let i = 0; i < 500; i++) {
  const block = ws.newBlock('text_print');
  block.initSvg();
  if (i > 0) blocks[i - 1].nextConnection.connect(block.previousConnection);
  blocks.push(block);
}
await Blockly.renderManagement.finishQueuedRenders();

const renders = sinon.spy(Blockly.BlockSvg.prototype, 'renderEfficiently');
const added = ws.newBlock('text_print');
added.initSvg();
blocks[499].nextConnection.connect(added.previousConnection);
Blockly.renderManagement.triggerQueuedRenders();
console.log(renders.callCount);
```

**Expected:** 2 (the new block, and the block it was connected to, whose bottom a renderer may draw
differently once a block is below it).

**Actual:** 501: every block of the stack is drawn again (we measured the same with our own blocks,
below).

Measured with Blocks2Cpp's own blocks (a `when program starts` C-block holding the statements,
each a print with a text shadow in a value input; Vitest with happy-dom, Zelos):

| Edit                                         | Blocks drawn, 12.5.1 | Blocks drawn, patched |
| -------------------------------------------- | -------------------- | --------------------- |
| Append a print to a stack of 500 prints      | 503                  | 4                     |
| Delete that print again                      | 501                  | 3                     |

With the patch, the four are the new print and its shadow, the print above it, and the C-block,
whose mouth grew. Both builds still update the locations of all 1,003 blocks and move all 2,005
connections, because the C-block is drawn again (see below).

`getDescendants(false)` on the first block of a chain of statements (headless workspace, the same
environment):

| Statements | 12.5.1                         | Patched |
| ---------- | ------------------------------ | ------- |
| 1,000      | 4.0 ms                         | 0.21 ms |
| 5,000      | 188 ms                         | 0.36 ms |
| 10,000     | 681 ms                         | 0.90 ms |
| 20,000     | `RangeError` (call stack size) | 1.55 ms |

(`Block.dispose` is recursive too, and already overflows the stack at 5,000 statements in the same
environment; the patch does not change it.)

In the app (Tauri 2 with WebKitGTK on Linux under xvfb; a 1,000-block program whose `when program
starts` holds 379 statements; a print appended at its end and deleted again, 8 times; medians),
with timing probes around the phases of `doRenders`:

| Per edit                                    | 12.5.1     | Patched  |
| ------------------------------------------- | ---------- | -------- |
| Blocks drawn, appending / deleting          | 382 / 380  | 4 / 3    |
| Drawing (`renderBlock`), appending          | 439.5 ms   | 20 ms    |
| Drawing, deleting                           | 442.5 ms   | 3 ms     |
| `resizeContents`, appending                 | 216 ms     | 204 ms   |
| Component locations, appending              | 11 ms      | 12.5 ms  |
| `bumpNeighbours`, appending                 | 31.5 ms    | 38.5 ms  |
| The page's longest task, appending          | 1,110.5 ms | 729.5 ms |

In alternating runs of our edit benchmark (the 95th percentile of 10 edits per sample, 10
samples per build, from the edit until the page has finished what it started), the time fell from
1,627.5 ms to 1,071 ms (−34 %). Selecting a block, which draws nothing, was unchanged.

## Cause

`core/render_management.ts`, `queueBlock` and `renderBlock`:

```ts
function queueBlock(block: BlockSvg) {
  dirtyBlocks.add(block);
  eventGroups.set(block, eventUtils.getGroup());
  const parent = block.getParent();
  if (parent) {
    queueBlock(parent);
  } else {
    rootBlocks.add(block);
  }
}

function renderBlock(block: BlockSvg) {
  if (!dirtyBlocks.has(block)) return;
  if (!block.initialized) return;
  for (const child of block.getChildren(false)) {
    renderBlock(child);
  }
  block.renderEfficiently();
}
```

Every dirty block is drawn, and every ancestor of a queued block is dirty.

## Suggested fix

The patch keeps the dirty set (the blocks `renderBlock` walks through) and adds a second set, the
blocks that are drawn again:

- `queueBlock` walks up iteratively. It marks for drawing the queued block, its parent (a renderer
  may draw a block differently once a block is connected below it: the common renderer squares its
  bottom-left corner, `BottomRow.hasLeftSquareCorner`), and each block further up that the walk
  reaches through one of its inputs, or that has an output connection (the common, Geras and
  Thrasos `RenderInfo` count the width of the blocks below such a block in its own). A block
  reached through its next connection is only walked through.
- `renderBlock` still walks every dirty block, and calls `renderEfficiently()` only on the marked
  ones.
- `doRenders` updates component locations from each drawn block down (as before, its whole
  subtree), and walks through the dirty blocks that were not drawn: they kept their shape and the
  places of their children, so only their dirty children can have moved.
- `Block.getDescendants` walks the tree with a stack of its own, in the same order.

The rest of a queued stack below a drawn block is still updated in full, which an edit in the
middle of a long stack needs when the block changed height; when the edited stack is inside a
C-block (as a program's main stack is), that C-block is drawn again (its mouth changed) and its
whole subtree is updated. Updating only the children whose place changed would need
`BlockSvg`'s private location methods, so the patch leaves it.

We checked the patch with a property test (in our editor's Vitest suite, not committed): 40 random
edits each with Zelos and with Geras (append, insert after a statement, insert into a C-block's
mouth, delete with healing, move a statement into another mouth, collapse and expand); after each
edit's render, every block's place, size and outline, every connection's place and every entry of
the connection databases equals what `workspace.render()` (which redraws everything) then gives. The
order of `getDescendants` equals the recursive one for ordered and unordered walks.

## Workaround we use

None. Blocks2Cpp ships 12.5.1 unchanged; the patch was measured in a local build only. It may be
applied as a reviewed `pnpm` patch in a later milestone, linked to this report, until a release
of Blockly carries a fix.
