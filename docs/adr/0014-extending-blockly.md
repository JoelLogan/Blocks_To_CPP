# ADR-0014: Extending Blockly: allowed seams and patched dependencies

* Status: Accepted (owner decision D6 of 2026-10-10)
* Date: 2026-10-10
* Partly supersedes [ADR-0002](0002-block-editor-blockly.md): its clause
  "extended through supported plugin APIs only". The choice of Blockly 12
  with the Zelos renderer stays in force.

## Context

[ADR-0002](0002-block-editor-blockly.md) chose Blockly with the Zelos
renderer, "extended through supported plugin APIs only (custom fields,
`ConnectionChecker`, mutators, renderer constants/theme, toolbox)". Blockly
does not define which of its APIs count as plugin APIs, and the editor
already uses a little more than that list:

* `B2cBlockDragger` subclasses Blockly's dragger
  (`Blockly.dragging.Dragger`) to report block drags to the editor;
* the toolbox, its flyout, its metrics manager and the toolbox category are
  replaced through `Blockly.registry` with classes of
  `@blockly/continuous-toolbox` or subclasses of them and of Blockly's (the
  flyout overrides the plugin's protected `getRecyclableInflater` and sets
  its protected `scrollAnimationFraction`).

The M2 polish wave, planned after the owner's first trial, needs more:

* **A drag strategy** (W1-0a). A block dropped on the toolbox while a
  connection is previewed is first connected and then deleted, together with
  every block below the insertion point: a data-loss bug. The fix overrides
  `BlockDragStrategy.drag` and `getSearchRadius` and installs the strategy
  with `BlockSvg.setDragStrategy`.
* **The flyout's protected methods** (W3-1). A fixed block scale and a width
  the user sets need `Flyout.getFlyoutScale` and `reflowInternal_`.
* **A replaced connection previewer** (W5-2), for Scratch-style wrapping of
  statements by an empty C-block.
* **A possible patch of Blockly's render management** (P-3): an edit at the
  end of a 1,000-block list costs about 0.85 s of rendering.

Separately, the owner was asked whether the block renderer should change,
because small connection targets made blocks hard to place (U8).

## Options considered

**How Blockly may be extended:**

1. **Supported plugin APIs only (ADR-0002 as it was).** The data-loss fix,
   the flyout width and wrapping could not be done, or only by reaching into
   private members.
2. **Public and protected members of documented classes, registry
   replacements and drag strategies; patches only with evidence
   (chosen).** Protected methods are part of the classes' published types
   and reference, so a subclass is checked by the TypeScript compiler on
   every upgrade.
3. **Replacing methods on Blockly's prototypes at runtime.** Invisible to the
   type checker and easy to miss in review, and it breaks silently when
   Blockly changes. Rejected.
4. **A fork of Blockly.** Full control, but every upstream fix and security
   update would have to be merged by hand, the reason ADR-0002 rejected
   scratch-blocks. Rejected.

**The renderer:**

1. **Keep Zelos (chosen)** and make the connection targets easier to hit
   with a snap radius that follows the zoom.
2. **Geras or Thrasos.** Smaller blocks, which are harder to hit.
3. **A custom Zelos variant with taller empty C-block mouths.** It changes
   only the look, not the hit test.

## Decision

* **Allowed seams.** The editor may use the public and protected methods of
  documented Blockly and plugin classes (for example
  `BlockDragStrategy.drag` and `getSearchRadius`, `Dragger.onDrag`,
  `onDragEnd` and `wouldDeleteDraggable`, `Flyout.getFlyoutScale` and
  `reflowInternal_`, `ConnectionChecker.doDragChecks`), components replaced
  through `Blockly.registry` (connection checker, connection previewer,
  flyouts, toolbox) and `BlockSvg.setDragStrategy`. Private members are not
  used.
* **A list of seams.** Each subclass of a Blockly or plugin class is listed
  in the developer guide with the upstream version it was checked against.
* **Patched dependencies.** A patched dependency (pnpm
  `patchedDependencies`) is allowed only with benchmark or defect evidence,
  a linked upstream issue or pull request, the smallest diff, and a test that
  fails without it. Each patch is reviewed as code, re-applied and
  re-reviewed on every update of the package, and removed once upstream
  ships the fix. Cargo `[patch]` is not used
  ([08 §8.9](../spec/08-security.md#89-supply-chain)).
* **The renderer stays Zelos.** Hard-to-hit targets are fixed by the snap
  radius, not by another renderer.

## Consequences

* The drop-on-the-toolbox fix, the flyout's fixed scale and width, and
  wrapping can be built on documented seams, each in a small subclass.
* Every upgrade of Blockly or its plugins re-checks the listed seams (M3
  spike for Blockly 13); the E2E and visual tests cover them.
* Defects found in Blockly or its plugins are reported upstream, so that the
  subclasses and any patch can be removed again.
* The render-management patch (P-3) is measured first as a spike, and lands
  (in M3) only if the measured gain justifies it and its review passes.
* The developer guide carries the list of seams
  ([Blockly seams](../developer-guide/desktop-architecture.md#blockly-seams-adr-0014)).
  Its first entries were the classes named in the Context; the polish wave
  added the drag strategy's preview preference and the connection previewer
  of wrapping (W5), and the list also names the workarounds of upstream
  defects, such as the drop on a delete area and the render queue that keeps
  disposed blocks, with their reports.
