/**
 * When the canvas records where its drag targets are: the toolbox, its always-open flyout and the
 * trash can, the places where a drop deletes (docs/spec/03-block-language.md §3.3 "Placing
 * blocks").
 *
 * Blockly checks every move of a drag, and its drop, against the rectangles the canvas recorded
 * last (`WorkspaceSvg.recordDragTargets`). A record reads each target's place on the screen
 * (`getBoundingClientRect`), and the first such read after any change to the page makes the
 * browser lay the whole page out at once, the canvas and the flyout's blocks included. Blockly
 * 12.5.1 records on a resize, a zoom, the first gesture after the page scrolled and when a flyout
 * is shown or hidden, but `Flyout.show` records before it displays the flyout again, when the
 * toolbox seems to end at the window's left edge (docs/plans/upstream/
 * blockly-flyout-setvisible-records-hidden.md); and the editor's flyout keeps its place and width
 * across a rebuild, so Blockly's own reflow does not record again. Without a later record, a drop
 * on the toolbox deleted nothing after a rebuild of the flyout.
 *
 * The editor's flyout therefore marks the record stale after each of its layouts, and whenever it
 * moves or changes size ({@link markDragTargetsStale}, `toolbox/continuous.ts`), instead of
 * recording there and then: a record there would lay the page out once more in every rebuild of
 * the toolbox, that is after nearly every edit. A stale record is made again once, by whichever
 * comes first:
 *
 * - the next animation frame, after the renders Blockly has queued by then: the browser lays the
 *   page out for that frame anyway, so the reads cost no layout of their own unless something
 *   changes the page again before the frame is drawn;
 * - the start of a drag (the editor's block dragger calls {@link recordDragTargetsIfStale}), for a
 *   drag that starts before that frame.
 *
 * A drag that starts once the record is current reads nothing.
 */
import * as Blockly from 'blockly/core';

/** The workspaces whose record of their drag targets is stale. */
const stale = new WeakSet<Blockly.WorkspaceSvg>();

/** The workspaces for which a record in an animation frame is on its way. */
const scheduled = new WeakSet<Blockly.WorkspaceSvg>();

/**
 * Marks `workspace`'s record of its drag targets stale: one of them moved, changed size or was
 * shown again since it was recorded. It is recorded again in the next animation frame, after the
 * renders Blockly has queued by then, or at the start of a drag that comes first. Marking reads
 * nothing; any number of marks before the record make one record.
 */
export function markDragTargetsStale(workspace: Blockly.WorkspaceSvg): void {
  stale.add(workspace);
  if (scheduled.has(workspace)) {
    return;
  }
  scheduled.add(workspace);
  requestAnimationFrame(() => {
    // Blockly renders the blocks it queued in an animation frame of its own: wait for those
    // renders (at once when none is queued), so that the reads find the page as it is drawn.
    void Blockly.renderManagement.finishQueuedRenders().then(() => {
      scheduled.delete(workspace);
      recordDragTargetsIfStale(workspace);
    });
  });
}

/**
 * Records `workspace`'s drag targets now if they were marked stale since they were last recorded
 * here; returns whether it recorded. A workspace disposed meanwhile is left alone.
 */
export function recordDragTargetsIfStale(workspace: Blockly.WorkspaceSvg): boolean {
  if (!stale.delete(workspace) || Blockly.Workspace.getById(workspace.id) !== workspace) {
    return false;
  }
  workspace.recordDragTargets();
  return true;
}
