# @blockly/continuous-toolbox: a chosen category is not selected after the scroll at most zoom levels

Repository: `google/blockly-samples`, package `plugins/continuous-toolbox`.
Version: `@blockly/continuous-toolbox` 7.0.9 with `blockly` 12.5.1 (also present on the
`master` source of `ContinuousFlyout.ts` as of 2026-10).

Two defects in `ContinuousFlyout`, both in the code that selects a category from the scroll
position after a category is clicked.

## 1. Rounding the scroll position selects the category before the clicked one

### What happens

Click a category in the toolbox. The flyout scrolls to the category's heading and, when the
scroll ends, the toolbox selection jumps back to the category **before** the clicked one. It
happens at most zoom levels, for some categories and not others.

### Cause

`recordScrollPositions()` records each heading at
`Math.max(0, label.getPosition().y - this.GAP_Y / 2)`, in workspace units. These positions are
often fractional (for example 312.4).

`scrollTo(position)` scrolls to `position * scale` pixels. When the scroll ends, the
`VIEWPORT_CHANGE` listener calls `selectCategoryByScrollPosition(-scrollY)`, which does:

```ts
const scaledPosition = Math.round(position / this.getWorkspace().scale);
for (const [name, position] of [...this.scrollPositions.entries()].reverse()) {
  if (scaledPosition >= position) {
    this.getParentToolbox()?.selectCategoryByName(name);
    return;
  }
}
```

The rounded value is compared with the fractional heading position. For a heading at 312.4,
the scroll ends at 312.4 × scale pixels, `Math.round(312.4)` is 312, and 312 >= 312.4 is false,
so the loop selects the previous category. Whether a category is affected depends on the
fractional part of its position, which changes with the content above it and the scale.

### Steps to reproduce

1. Inject a workspace with the continuous toolbox (`registerContinuousToolbox()`), several
   categories with blocks of different heights, and zoom controls.
2. Zoom in or out once or twice.
3. Click each category in turn and watch the toolbox selection once the flyout stops.

Expected: the clicked category stays selected. Actual: for some categories, the one above it is
selected.

### Suggested fix

Compare like with like: drop the `Math.round`, or round the recorded positions, or scroll to a
target a fraction past the heading. A workaround in a subclass that scrolls to `position + 0.5`
selected the right category in all 192 clicks we tried at eight zoom levels:

```ts
class FixedFlyout extends ContinuousFlyout {
  override scrollTo(position: number) {
    super.scrollTo(position + 0.5);
  }
}
```

## 2. A scroll target of 0 counts as "not scrolling"

`selectCategoryByScrollPosition` and `wheel_` test `if (this.scrollTarget)`. The first category's
position is 0 (`Math.max(0, …)`), so while the flyout animates up to the first category,
`scrollTarget` is 0, the test is false, and every `VIEWPORT_CHANGE` on the way selects the
category under the current position: the selection flickers through the categories above, and the
wheel is not ignored during that animation.

Suggested fix: test `this.scrollTarget !== undefined`.

## Environment

Linux (WebKitGTK 2.4x, Tauri 2) and Windows (WebView2); the defect is in the plugin's arithmetic
and does not depend on the browser.
