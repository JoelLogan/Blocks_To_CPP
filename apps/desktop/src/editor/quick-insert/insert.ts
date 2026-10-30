/**
 * Inserting a block from Quick Insert (docs/spec/03-block-language.md §3.14,
 * docs/plans/m3-contracts/w4-editor-tools.md §2.4): where it goes, and the insertion as one undo
 * step.
 *
 * The place, in order: the connection the keyboard cursor is on, when the block fits it; after the
 * selected statement block, or at the start of the body of a selected block that has nothing after
 * it (*when program starts*, *define*), for a statement; into the selected block's first empty value
 * input it fits, for a value; otherwise loose on the canvas, near the top left of the view. The block is created
 * from its toolbox item and settled where it goes as a block dragged from the toolbox is (free
 * names, its variable, start values: ../toolbox/landing.ts), all in one Blockly event group, so one
 * Undo takes it away. It is then selected.
 */
import * as Blockly from 'blockly/core';

import { noteCreatedFromItem, settleDroppedBlock } from '../toolbox/landing';
import { blockState, type B2cBlockInfo } from '../toolbox/presets';
import type { SymbolSource } from '../toolbox/scope';

/** How far inside the view a loose block goes, in workspace units. */
const VIEW_MARGIN = 40;

/** Where a block was inserted. */
export type InsertedAt = 'cursor' | 'after' | 'body' | 'input' | 'canvas';

/** What an insertion did. */
export interface Inserted {
  readonly block: Blockly.BlockSvg;
  readonly at: InsertedAt;
}

/** Whether `a` may connect to `b` (the workspace's checker, with its type rules). */
function canConnect(
  workspace: Blockly.WorkspaceSvg,
  a: Blockly.Connection | null,
  b: Blockly.Connection | null,
): boolean {
  if (a === null || b === null) {
    return false;
  }
  try {
    return workspace.connectionChecker.canConnect(a, b, false);
  } catch {
    return false;
  }
}

/** Connects `block` at `target` when one of its connections fits it; whether it did. */
function connectAt(
  workspace: Blockly.WorkspaceSvg,
  block: Blockly.BlockSvg,
  target: Blockly.Connection | null,
): boolean {
  if (target === null) {
    return false;
  }
  // Blockly types them as always there; a block of another shape has `null` instead.
  const connections: (Blockly.Connection | null)[] = [
    block.outputConnection,
    block.previousConnection,
    block.nextConnection,
  ];
  const own = connections.find((connection) => canConnect(workspace, connection, target));
  if (own === undefined || own === null) {
    return false;
  }
  // A value input with a block in it is not replaced (a shadow is).
  const held = target.targetBlock();
  if (own === block.outputConnection && held !== null && !held.isShadow()) {
    return false;
  }
  own.connect(target);
  return true;
}

/** Whether a block can have one after it (Blockly types its next connection as always there). */
function hasNext(block: Blockly.Block): boolean {
  return (block as { readonly nextConnection: Blockly.Connection | null }).nextConnection !== null;
}

/** Connects `block` after or into `selected`; where, or `null`. */
function connectToSelected(
  workspace: Blockly.WorkspaceSvg,
  block: Blockly.BlockSvg,
  selected: Blockly.BlockSvg,
): InsertedAt | null {
  // The checker refuses a connection of the wrong kind (a value after a statement).
  if (connectAt(workspace, block, selected.nextConnection)) {
    return 'after';
  }
  // A block with nothing after it (*when program starts*, *define*): at the start of its body.
  if (!hasNext(selected)) {
    for (const input of selected.inputList) {
      if (
        input instanceof Blockly.inputs.StatementInput &&
        connectAt(workspace, block, input.connection)
      ) {
        return 'body';
      }
    }
  }
  for (const input of selected.inputList) {
    if (
      input instanceof Blockly.inputs.ValueInput &&
      connectAt(workspace, block, input.connection)
    ) {
      return 'input';
    }
  }
  return null;
}

/** Puts `block` loose near the top left of the view. */
function placeInView(workspace: Blockly.WorkspaceSvg, block: Blockly.BlockSvg): void {
  const view = workspace.getMetricsManager().getViewMetrics(true);
  block.moveTo(new Blockly.utils.Coordinate(view.left + VIEW_MARGIN, view.top + VIEW_MARGIN));
  block.bumpNeighbours();
}

/** What {@link insertBlock} inserts at. */
export interface InsertTarget {
  /** The connection the keyboard cursor is on, or `null`. */
  readonly connection: Blockly.Connection | null;
  /** The selected block, or `null`. */
  readonly selected: Blockly.BlockSvg | null;
}

/** The place Quick Insert inserts at now: the keyboard cursor's connection, the selection. */
export function currentTarget(workspace: Blockly.WorkspaceSvg): InsertTarget {
  let connection: Blockly.Connection | null = null;
  try {
    const node = Blockly.getFocusManager().getFocusedNode();
    if (
      node instanceof Blockly.RenderedConnection &&
      node.getSourceBlock().workspace === workspace
    ) {
      connection = node;
    }
  } catch {
    connection = null;
  }
  const selected = Blockly.common.getSelected();
  return {
    connection,
    selected:
      selected instanceof Blockly.BlockSvg &&
      selected.workspace === workspace &&
      !selected.isShadow()
        ? selected
        : null,
  };
}

/**
 * Inserts the toolbox block `item` at `target` (see the module comment) as one undo step, selects
 * it and returns it, or `null` when it could not be created.
 */
export function insertBlock(
  workspace: Blockly.WorkspaceSvg,
  item: B2cBlockInfo,
  target: InsertTarget,
  symbols: SymbolSource,
): Inserted | null {
  if (workspace.isReadOnly()) {
    return null;
  }
  const outer = Blockly.Events.getGroup();
  if (outer === '') {
    Blockly.Events.setGroup(true);
  }
  let inserted: Inserted | null;
  try {
    const created = Blockly.serialization.blocks.append(blockState(item), workspace, {
      recordUndo: true,
    });
    if (!(created instanceof Blockly.BlockSvg)) {
      created.dispose(false);
      return null;
    }
    noteCreatedFromItem(created, item);
    let at: InsertedAt = 'canvas';
    if (connectAt(workspace, created, target.connection)) {
      at = 'cursor';
    } else if (target.selected !== null && !target.selected.isDeadOrDying()) {
      at = connectToSelected(workspace, created, target.selected) ?? 'canvas';
    }
    if (at === 'canvas') {
      placeInView(workspace, created);
    }
    settleDroppedBlock(workspace, created, symbols);
    inserted = { block: created, at };
  } catch (error: unknown) {
    console.error('Quick Insert could not insert the block', error);
    inserted = null;
  } finally {
    if (outer === '') {
      Blockly.Events.setGroup(false);
    }
  }
  if (inserted !== null) {
    Blockly.common.setSelected(inserted.block);
    try {
      Blockly.getFocusManager().focusNode(inserted.block);
    } catch {
      // The selection is enough without keyboard focus.
    }
  }
  return inserted;
}
