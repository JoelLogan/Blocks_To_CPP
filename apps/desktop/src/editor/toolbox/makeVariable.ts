/**
 * *Make a variable* (docs/spec/03-block-language.md §3.6, 04 §4.2): asks for a name in the app's
 * accessible dialog (never `window.prompt`), suggesting the first free name, and inserts a
 * `create` block with that name and a fresh symbol ID:
 *
 * - before the selected statement (the statement holding a selected value block), so the selected
 *   block can use it;
 * - at the top of the selected function or `main`;
 * - with nothing usable selected, at the top of `main`, creating `main` when the canvas has none.
 *
 * There are no implicit globals: the variable is always declared inside `main` or a function. The
 * insertion is one undo step.
 */
import { MAX_NAME_CHARS } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import type { DialogService } from '../../app/dialogs';
import { newVariableBlock } from './contents';
import { variableNameProblem } from './identifiers';
import { firstFreeName } from './names';
import { blockState } from './presets';
import {
  BODY_INPUT,
  MAIN_TYPE,
  insertionPoint,
  takenNamesAtInsertion,
  type InsertionPoint,
  type SymbolSource,
} from './scope';

/** The dialog's title. */
export const MAKE_VARIABLE_TITLE = 'Make a variable';
/** The dialog's question, which is also the text field's label. */
export const MAKE_VARIABLE_QUESTION = 'Name of the new variable';

/** Where a new `main` goes, relative to the top left of the visible canvas (or the origin). */
const NEW_MAIN_OFFSET = 40;

/** What *Make a variable* needs. */
export interface MakeVariableContext {
  /** The canvas to insert into. */
  readonly workspace: Blockly.Workspace;
  /** The symbols of the latest analysis (for the suggested name and the name check). */
  readonly symbols: SymbolSource;
  /** The app's dialogs. */
  readonly dialogs: Pick<DialogService, 'prompt' | 'alert'>;
  /** The selected block of the canvas, or `null`. */
  selected(): Blockly.Block | null;
  /** Whether another module of the project has `main` (so this canvas must not get one). */
  mainInOtherModule(): boolean;
}

/** How *Make a variable* ended. */
export type MakeVariableResult =
  | { readonly status: 'created'; readonly block: Blockly.Block }
  | { readonly status: 'cancelled' }
  | { readonly status: 'refused'; readonly reason: 'readOnly' | 'noMain' | 'invalidName' };

/** Whether the blocks an insertion point names are still on the canvas. */
function isCurrent(point: InsertionPoint, workspace: Blockly.Workspace): boolean {
  switch (point.kind) {
    case 'before':
      return workspace.getBlockById(point.block.id) === point.block && !point.block.isDisposed();
    case 'top':
      return (
        workspace.getBlockById(point.container.id) === point.container &&
        !point.container.isDisposed()
      );
    case 'newMain':
      return true;
  }
}

/** Creates `main` near the top left of what the canvas shows. */
function createMain(workspace: Blockly.Workspace): Blockly.Block {
  let x = NEW_MAIN_OFFSET;
  let y = NEW_MAIN_OFFSET;
  if (workspace instanceof Blockly.WorkspaceSvg) {
    const view = workspace.getMetricsManager().getViewMetrics(true);
    x = Math.round(view.left + NEW_MAIN_OFFSET);
    y = Math.round(view.top + NEW_MAIN_OFFSET);
  }
  return Blockly.serialization.blocks.append({ type: MAIN_TYPE, x, y }, workspace, {
    recordUndo: true,
  });
}

/** The connection a new first statement of a body attaches to. */
function bodyConnection(container: Blockly.Block): Blockly.Connection | null {
  return container.getInput(BODY_INPUT)?.connection ?? null;
}

/**
 * Inserts a declaration of `name` at `point`, as one undo step. The statement that was at that
 * place moves down below the new block.
 */
export function insertDeclaration(
  workspace: Blockly.Workspace,
  point: InsertionPoint,
  name: string,
): Blockly.Block {
  const existingGroup = Blockly.Events.getGroup();
  if (existingGroup === '') {
    Blockly.Events.setGroup(true);
  }
  try {
    const container =
      point.kind === 'newMain'
        ? createMain(workspace)
        : point.kind === 'top'
          ? point.container
          : null;
    const target =
      point.kind === 'before'
        ? (point.block.previousConnection?.targetConnection ?? null)
        : container === null
          ? null
          : bodyConnection(container);
    const block = Blockly.serialization.blocks.append(
      blockState(newVariableBlock(name)),
      workspace,
      {
        recordUndo: true,
      },
    );
    if (target !== null && block.previousConnection !== null) {
      block.previousConnection.connect(target);
    }
    return block;
  } finally {
    if (existingGroup === '') {
      Blockly.Events.setGroup(false);
    }
  }
}

/** Scrolls a new block into view once it is drawn (rendered canvases only). */
function revealWhenRendered(block: Blockly.Block): void {
  const workspace = block.workspace;
  if (!(workspace instanceof Blockly.WorkspaceSvg) || !(block instanceof Blockly.BlockSvg)) {
    return;
  }
  void Blockly.renderManagement
    .finishQueuedRenders()
    .then(() => {
      if (!block.isDisposed()) {
        workspace.scrollBoundsIntoView(block.getBoundingRectangle());
      }
    })
    .catch((error: unknown) => {
      console.warn('The new variable could not be scrolled into view', error);
    });
}

/** Asks for a name and creates the variable (see the module comment). */
export async function makeVariable(context: MakeVariableContext): Promise<MakeVariableResult> {
  const { workspace } = context;
  if (workspace.isReadOnly()) {
    return { status: 'refused', reason: 'readOnly' };
  }
  let point = insertionPoint(workspace, context.selected());
  if (point.kind === 'newMain' && context.mainInOtherModule()) {
    await context.dialogs.alert({
      title: MAKE_VARIABLE_TITLE,
      message:
        'This module has no “when program starts” block. Click a block inside a function first, ' +
        'then choose Make a variable again.',
    });
    return { status: 'refused', reason: 'noMain' };
  }
  const taken = takenNamesAtInsertion(point, context.symbols);
  const name = await context.dialogs.prompt({
    title: MAKE_VARIABLE_TITLE,
    message: MAKE_VARIABLE_QUESTION,
    defaultValue: firstFreeName('variable', taken),
    okLabel: 'Create',
    maxLength: MAX_NAME_CHARS,
    validate: (value) => variableNameProblem(value, taken),
  });
  if (name === null) {
    return { status: 'cancelled' };
  }
  // The dialog checks as the user types; this check guards the insertion itself.
  if (variableNameProblem(name, taken) !== null) {
    return { status: 'refused', reason: 'invalidName' };
  }
  if (!isCurrent(point, workspace)) {
    point = insertionPoint(workspace, context.selected());
  }
  if (workspace.isReadOnly()) {
    return { status: 'refused', reason: 'readOnly' };
  }
  const block = insertDeclaration(workspace, point, name);
  revealWhenRendered(block);
  return { status: 'created', block };
}
