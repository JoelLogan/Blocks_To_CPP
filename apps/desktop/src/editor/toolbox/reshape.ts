/**
 * A new variable's start value follows its type (docs/spec/04-user-interface.md §4.2, M2 decision
 * "Smart defaults for inputs without a catalog default"): when the user changes the type of a
 * `create` block whose value is still the start value of the old type (`0` for `int`), the value
 * becomes the start value of the new one (`0.0` for `double`, `true`, `'a'`, `""`). A value the user
 * typed, a block in the slot, and `auto` are left alone.
 *
 * The new value is recorded as an event of its own ({@link StartValueChange}) in the type change's
 * group, so undo and redo restore the type and the value together.
 */
import {
  catalogBlock,
  exprShadowState,
  readExprShadow,
  tokensEqual,
  type TokenJson,
} from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import { DECLARE_TYPE } from './scope';
import { startTokensForDeclaredType } from './values';

/** The type field of `var.declare`. */
const TYPE_FIELD = 'TYPE';
/** The value input of `var.declare`. */
const VALUE_INPUT = 'VALUE';

/** The value input's connection and its expression shadow's tokens, if it holds one. */
function startValueOf(
  block: Blockly.Block,
): { connection: Blockly.Connection; tokens: readonly TokenJson[] } | null {
  const connection = block.getInput(VALUE_INPUT)?.connection ?? null;
  const shadow = connection?.targetBlock() ?? null;
  if (connection === null || shadow?.isShadow() !== true) {
    return null;
  }
  const current = readExprShadow(shadow);
  return current === null || current.draft ? null : { connection, tokens: current.tokens };
}

/** Puts an expression shadow with `tokens` in the block's value input (no events). */
function setStartValue(connection: Blockly.Connection, tokens: readonly TokenJson[]): void {
  const check =
    catalogBlock(DECLARE_TYPE)?.inputs.find((input) => input.name === VALUE_INPUT)?.check ?? 'any';
  connection.setShadowState(exprShadowState(tokens, false, check, false));
}

/** The event type of {@link StartValueChange}. */
export const START_VALUE_EVENT = 'b2c_start_value';

/**
 * A `create` block's start value was replaced by the start value of its new type. Blockly records no
 * event when a shadow is replaced, so without this one undo would restore the type but keep the
 * new type's value.
 */
export class StartValueChange extends Blockly.Events.BlockBase {
  override type = START_VALUE_EVENT;
  /** The start value before the change. */
  oldTokens: readonly TokenJson[];
  /** The start value after the change. */
  newTokens: readonly TokenJson[];

  constructor(
    block?: Blockly.Block,
    oldTokens: readonly TokenJson[] = [],
    newTokens: readonly TokenJson[] = [],
  ) {
    super(block);
    this.oldTokens = oldTokens;
    this.newTokens = newTokens;
  }

  override isNull(): boolean {
    return tokensEqual(this.oldTokens, this.newTokens);
  }

  /** Puts the value back (undo) or again (redo). */
  override run(forward: boolean): void {
    const id = this.blockId;
    const block = id === undefined ? null : this.getEventWorkspace_().getBlockById(id);
    const value = block === null ? null : startValueOf(block);
    if (value === null) {
      console.warn('The start value to restore is gone');
      return;
    }
    setStartValue(value.connection, forward ? this.newTokens : this.oldTokens);
  }
}

/**
 * Replaces the start value of a `var.declare` block after its type changed from `oldType` to
 * `newType`, if the value is still the old type's start value. Returns the event describing the
 * change, or `null` when nothing changed.
 */
export function reshapeStartValue(
  block: Blockly.Block,
  oldType: string,
  newType: string,
): StartValueChange | null {
  const value = startValueOf(block);
  const before = startTokensForDeclaredType(oldType);
  const after = startTokensForDeclaredType(newType);
  if (
    value === null ||
    before === null ||
    after === null ||
    tokensEqual(before, after) ||
    !tokensEqual(value.tokens, before)
  ) {
    return null;
  }
  setStartValue(value.connection, after);
  return new StartValueChange(block, before, after);
}

/** Whether an event is the user changing a `var.declare` block's type (not undo, not loading). */
function isTypeChange(event: Blockly.Events.Abstract): event is Blockly.Events.BlockChange {
  return (
    event instanceof Blockly.Events.BlockChange &&
    event.recordUndo &&
    event.element === 'field' &&
    event.name === TYPE_FIELD
  );
}

/**
 * Keeps start values in step with types on `workspace`. Returns the function that stops it.
 */
export function installStartValueReshaping(workspace: Blockly.Workspace): () => void {
  const listener = (event: Blockly.Events.Abstract): void => {
    if (!isTypeChange(event) || event.blockId === undefined) {
      return;
    }
    const block = workspace.getBlockById(event.blockId);
    if (block?.type !== DECLARE_TYPE || block.isInFlyout) {
      return;
    }
    const oldType = typeof event.oldValue === 'string' ? event.oldValue : '';
    const newType = typeof event.newValue === 'string' ? event.newValue : '';
    const change = reshapeStartValue(block, oldType, newType);
    if (change === null) {
      return;
    }
    // One undo step: the type change gets a group if it had none (it is already on the undo stack,
    // so the group is set on it there), and the new value joins it.
    if (event.group === '') {
      event.group = Blockly.utils.idGenerator.genUid();
    }
    change.group = event.group;
    change.recordUndo = true;
    Blockly.Events.fire(change);
  };
  workspace.addChangeListener(listener);
  return () => {
    workspace.removeChangeListener(listener);
  };
}
