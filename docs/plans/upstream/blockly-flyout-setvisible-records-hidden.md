# Flyout.setVisible records drag targets before updateDisplay

**Project:** google/blockly (core)
**Version:** 12.5.1 (later versions not checked)
**Severity:** drops on an always-open flyout are missed (a delete area that does not delete)
**Found by:** Blocks2Cpp (a Blockly 12 editor with the Zelos renderer and `@blockly/continuous-toolbox`)

## Summary

`Flyout.setVisible(true)` asks the target workspace to record its drag targets **before** it
displays the flyout again. At that moment the flyout's SVG still has `display: none`, so
`VerticalFlyout.getClientRect()` measures an empty box and records a delete area whose right edge
is 0 (the window's left edge). A toolbox whose delete area is its never-closing flyout
(`@blockly/continuous-toolbox`'s `ContinuousToolbox.getClientRect` returns the flyout's rectangle)
is recorded the same way.

In stock Blockly the wrong record is hidden, because `Flyout.show` sets `width_` to 0 just before
`reflow()`, so `VerticalFlyout.reflowInternal_` always finds a "new" width and records again. A
subclass whose reflow keeps the width when nothing changed (to avoid placing the flyout and
resizing the workspace on every showing) is left with the wrong record: every drop over the flyout
or the category column misses the delete area, nothing is shown faded, and connections are still
previewed over the flyout, until something else records again (a workspace resize, a zoom, the
first gesture after a page scroll). Only the trash can and the window's left edge still delete.

## Steps to reproduce

1. Subclass `VerticalFlyout` so that `reflowInternal_` places the flyout and calls
   `targetWorkspace.recordDragTargets()` only when its computed width differs from the width it
   last applied (rather than from `width_`, which `show` resets).
2. Inject a workspace with that flyout in a toolbox whose flyout never closes
   (`@blockly/continuous-toolbox`, or a flyout with `autoClose` false) on the left, and a block on
   the workspace.
3. Make one gesture on the workspace, for example drag its background a little. Blockly records the
   drag targets at the first gesture on a workspace (`Gesture.doStart` calls
   `updateScreenCalculationsIfScrolled`, whose last recorded page scroll starts as `null`), so this
   gesture has to come before the next step: a first gesture made after it records the right
   rectangles again and hides the defect.
4. Show the flyout twice with the same contents (for example a dynamic category built again), so
   the width does not change.
5. Drag the block and release it over the middle of the flyout.

**Expected:** the block is deleted; while the pointer is over the flyout the block is shown with
the delete style and the toolbox has `blocklyToolboxDelete`.

**Actual:** the block is dropped on the workspace under the flyout (or connected to a previewed
connection); `workspace.getDragTarget(e)` returns `null` over the whole flyout, and
`workspace.dragTargetAreas` holds the toolbox and the flyout with a right edge of 0, although
`flyout.getClientRect()` called at that moment returns the right rectangle.

## Cause

`core/flyout_base.ts`, `Flyout.setVisible` (lines 497–509 in 12.5.1):

```ts
this.visible = visible;
if (visibilityChanged) {
  if (!this.autoClose) {
    this.targetWorkspace.recordDragTargets();   // the SVG is still display: none here
  }
  this.updateDisplay();                         // only now display: block
}
```

`Flyout.show` (lines 624–651) calls `hide()` (which records with the flyout invisible, so it is
left out) and then `setVisible(true)` (which records with the SVG not displayed), before it lays
out and reflows the contents. `VerticalFlyout.getClientRect` (`core/flyout_vertical.ts`,
lines 272–294) builds the area from `svgGroup_.getBoundingClientRect()`, which is empty while the
SVG is not displayed. The order is the base class's, so a `HorizontalFlyout` is recorded the
same way.

## Suggested fix

In `Flyout.setVisible`, update the display first and record afterwards:

```ts
if (visibilityChanged) {
  this.updateDisplay();
  if (!this.autoClose) {
    this.targetWorkspace.recordDragTargets();
  }
}
```

Recording again at the end of `Flyout.show` (after `reflow()`) would also cover subclasses whose
reflow does not record. `Flyout.setContainerVisible` changes the display without recording at
all; it may want the same record when the flyout becomes displayed.

## Workaround we use

The flyout subclass records the drag targets after every reflow (and whenever its `position()`
moves or resizes it), and the editor's `Dragger` subclass calls
`this.workspace.recordDragTargets()` in `onDragStart` before `super.onDragStart(e)`, so every drag
checks the drop against the current rectangles.
