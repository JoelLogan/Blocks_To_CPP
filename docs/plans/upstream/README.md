# Upstream reports to file

Defects found in our dependencies while working on Blocks2Cpp, written up as
reports for their projects. The agents of the M2 polish wave may not open
issues on GitHub, so the owner files each report and records its link in the
table and in the code that works around the defect. Filing is deferred for
now (owner, 2026-10-12; plan F-8). Once upstream ships a
fix, the workaround is removed (ADR-0014).

| Report | Project | Worked around in | Filed |
| ------ | ------- | ---------------- | ----- |
| [Dropping a block on a delete area deletes the blocks below the insertion point](blockly-drop-on-delete-area-deletes-stack.md) | google/blockly | `apps/desktop/src/editor/sync/drag.ts` (W1-0a) | not yet |
| [A chosen category is not selected after the scroll](continuous-toolbox-category-selection.md) (two defects) | google/blockly-samples, `@blockly/continuous-toolbox` | `apps/desktop/src/editor/toolbox/continuous.ts` (W1-7) | not yet |
| [Flyout.setVisible records drag targets before updateDisplay](blockly-flyout-setvisible-records-hidden.md) | google/blockly | `apps/desktop/src/editor/sync/drag.ts` (`B2cBlockDragger.onDragStart`) and `apps/desktop/src/editor/toolbox/continuous.ts` (`B2cContinuousFlyout`) | not yet |
| [Render management redraws every statement above an edited one; getDescendants is quadratic](blockly-render-management.md), with [the patch](blockly-render-management.patch) and [the spike's tests and tools](p3-spike/README.md) | google/blockly | nothing yet: measured in the P-3 spike only; a pnpm patch is M3-2 | not yet |
| [The render queue keeps every block disposed after it was queued as a top block](blockly-render-queue-retains-disposed-blocks.md) | google/blockly | `packages/blockly-ext/src/checker/previewer.ts` (`restore`, W5-2) | not yet |
