# Dropping a block on a delete area while a connection is previewed deletes the blocks below the insertion point

**Project:** google/blockly (core)
**Version:** 12.5.1 (later versions not checked)
**Severity:** data loss
**Found by:** Blocks2Cpp (a Blockly 12 editor with the Zelos renderer and `@blockly/continuous-toolbox`)

## Summary

When a block is dropped with the pointer over a delete area (the toolbox, an always-open flyout,
the trash can) while a connection preview is still active, Blockly first *applies* the previewed
connection and then disposes the dragged block. If the preview was an insertion between two
statements, the dragged block is spliced into the stack before it is disposed, and `dispose()`
(with `healStack` false) takes every block that the insertion put below it. The user loses the
dragged block **and** the rest of the target stack. Undo restores them, but the user rarely
notices what happened.

## Steps to reproduce

1. Inject a workspace with a toolbox whose flyout never closes (for example
   `@blockly/continuous-toolbox`), or with a trash can.
2. Put a stack `A → B → C` on the canvas close to the delete area (for example scrolled so that it
   sits right next to, or under, the flyout).
3. Drag a statement block `X` so that its previous connection is within the snap radius of
   `A.nextConnection` (the insertion marker appears between `A` and `B`).
4. Without moving `X`'s connection out of the snap radius, move the pointer over the delete area
   (for example by grabbing `X` near its right end, or with a short flick), and release.

   Variant: release the pointer over the delete area **without any `pointermove` after the
   preview appeared** (a fast flick, a touch release, or an automated drop such as WebDriver
   `performActions`). `Gesture.handleUp` calls `onDragEnd` without a final `onDrag`, so the
   strategy still holds the candidate of the last move.

**Expected:** only `X` is deleted; `A → B → C` is unchanged; no preview is shown while the
pointer is over the delete area.

**Actual:** `X`, `B` and `C` are all deleted; `A` remains.

A Vitest/happy-dom reproduction with real pointer events (a component with the `DELETE_AREA`
and `DRAG_TARGET` capabilities and a fixed `getClientRect` stands in for the flyout) shows `main`'s
list going from `[A, B, C, D]` to `[A]` after the drop.

## Cause

`core/dragging/dragger.ts`, `Dragger.onDragEnd` (lines 104–140 in 12.5.1):

```ts
const root = this.getRoot(this.draggable);           // X, the dragged stack's root
...
const wouldDelete = isDeletable(root) && this.wouldDeleteDraggable(e, root);
...
this.draggable.endDrag(e);                           // applies the candidate: A → X → B → C
if (wouldDelete && isDeletable(root)) {
  ...
  root.dispose();                                    // disposes X and everything below it
}
```

`core/dragging/block_drag_strategy.ts`, `BlockDragStrategy.endDrag` (lines 436–438) applies
`this.connectionCandidate` unconditionally; the strategy never learns that the drop deletes.
`BlockDragStrategy.drag` does not look at drag targets at all, so the preview (and the candidate)
stays while the pointer is over a delete area.

`core/gesture.ts`, `Gesture.handleUp` (lines 525–547) ends the drag with
`dragger.onDragEnd(e, currentDragDeltaXY)` without a final `onDrag`, so the candidate can come
from a position the pointer has already left.

A related detail: for a shadow block, `BlockDragStrategy.drag` (line 251) forwards the move to
the parent as `this.block.getParent()?.drag(Coordinate.sum(newLoc, this.dragOffset))`, dropping
the `PointerEvent`, so a strategy on the parent cannot see where the pointer is.

## Suggested fix

Any one of these prevents the data loss; the first two together also fix the misleading preview:

1. In `BlockDragStrategy.drag(newLoc, e)`, do not search for a connection (hide the preview) while
   `workspace.getDragTarget(e)` is a component with the `DELETE_AREA` capability that would delete
   the block. Note that a search radius of `0` is not enough, because
   `ConnectionDB.searchForClosest` still accepts an exactly coincident connection; the search has to
   be skipped (or use a negative radius).
2. In `Dragger.onDragEnd`, when the drop would delete, either run a final `onDrag(e, totalDelta)`
   first (so the strategy sees the drop position), or tell the strategy not to apply its candidate
   (for example by calling `revertDrag`-like clean-up of the candidate before `endDrag`).
3. Dispose only the dragged stack: capture the dragged blocks before `endDrag` and dispose them
   without the blocks a connection added below them.

Also, the shadow path of `BlockDragStrategy.drag` should pass the event on:
`parent.drag(Coordinate.sum(newLoc, this.dragOffset), e)`.

## Workaround we use

A `BlockDragStrategy` subclass that returns `-1` from `getSearchRadius()` while the pointer of the
latest `drag(newLoc, e)` is over a delete area, installed per drag with `BlockSvg.setDragStrategy`
by a `Dragger` subclass; the `Dragger` subclass runs `this.onDrag(e, totalDelta)` before
`super.onDragEnd(e)` when `wouldDeleteDraggable(e, root)` is true, and drags a shadow's first
non-shadow ancestor instead of the shadow, so the strategy always receives the pointer event.
