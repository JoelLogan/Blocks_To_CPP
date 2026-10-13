/**
 * Start values follow types (docs/spec/04-user-interface.md §4.2, M2 decision "Smart defaults for
 * inputs without a catalog default"):
 *
 * - when the user changes the type of a `create` block whose value is still the start value of the
 *   old type (`0` for `int`), the value becomes the start value of the new one (`0.0` for
 *   `double`, `true`, `'a'`, `""`);
 * - when the variable of a *set* block changes (chosen in its menu, or given by the toolbox where
 *   the block was dropped, ./landing.ts) and its value is still the start value of the old
 *   variable's type, the value becomes the start value of the new variable's type.
 *
 * A value the user typed, a block in the slot, `auto`, and a type the analysis does not know are
 * left alone. The new value is recorded as an event of its own ({@link StartValueChange}) in the
 * group of the change that caused it, so undo and redo restore both together.
 */
import type { StaticType } from '@blocks2cpp/b2c-core-wasm';
import {
  catalogBlock,
  exprShadowState,
  readExprShadow,
  tokensEqual,
  type TokenJson,
} from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import { DECLARE_TYPE } from './scope';
import { startTokensForDeclaredType, startTokensForStaticType } from './values';

/** The type field of `var.declare`. */
const TYPE_FIELD = 'TYPE';
/** The block that gives a variable a value. */
const SET_TYPE = 'var.set';
/** The variable field of `var.set`. */
const VAR_FIELD = 'VAR';
/** The value input of `var.declare` and `var.set`. */
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

/** Puts an expression shadow with `tokens` in the value input of `block` (no events). */
function setStartValue(
  block: Blockly.Block,
  connection: Blockly.Connection,
  tokens: readonly TokenJson[],
): void {
  const check =
    catalogBlock(block.type)?.inputs.find((input) => input.name === VALUE_INPUT)?.check ?? 'any';
  connection.setShadowState(exprShadowState(tokens, false, check, false));
}

/** The event type of {@link StartValueChange}. */
export const START_VALUE_EVENT = 'b2c_start_value';

/**
 * A block's start value was replaced by the start value of its new type. Blockly records no event
 * when a shadow is replaced, so without this one undo would restore the type (or the variable) but
 * keep the new value.
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
    if (block === null || value === null) {
      console.warn('The start value to restore is gone');
      return;
    }
    setStartValue(block, value.connection, forward ? this.newTokens : this.oldTokens);
  }
}

/**
 * Replaces `block`'s start value `before` with `after` when its value is still `before`. Returns
 * the event describing the change, or `null` when nothing changed.
 */
function swapStartValue(
  block: Blockly.Block,
  before: readonly TokenJson[] | null,
  after: readonly TokenJson[] | null,
): StartValueChange | null {
  const value = startValueOf(block);
  if (
    value === null ||
    before === null ||
    after === null ||
    tokensEqual(before, after) ||
    !tokensEqual(value.tokens, before)
  ) {
    return null;
  }
  setStartValue(block, value.connection, after);
  return new StartValueChange(block, before, after);
}

/**
 * Replaces the start value of a `var.declare` block after its type changed from `oldType` to
 * `newType` (TYPE values), if the value is still the old type's start value. Returns the event
 * describing the change, or `null` when nothing changed.
 */
export function reshapeStartValue(
  block: Blockly.Block,
  oldType: string,
  newType: string,
): StartValueChange | null {
  return swapStartValue(
    block,
    startTokensForDeclaredType(oldType),
    startTokensForDeclaredType(newType),
  );
}

/**
 * Replaces the value of a `var.set` block after its variable changed from one of `oldType` to one
 * of `newType` (`null`: a type the analysis does not know), if the value is still the old type's
 * start value. Returns the event describing the change, or `null` when nothing changed (also for
 * any other block).
 */
export function reshapeSetValue(
  block: Blockly.Block,
  oldType: StaticType | null,
  newType: StaticType | null,
): StartValueChange | null {
  if (block.type !== SET_TYPE || oldType === null || newType === null) {
    return null;
  }
  return swapStartValue(
    block,
    startTokensForStaticType(oldType),
    startTokensForStaticType(newType),
  );
}

/**
 * Whether an event is the user changing field `field` of a block (not undo, not loading), with
 * text values before and after.
 */
function isFieldChange(
  event: Blockly.Events.Abstract,
  field: string,
): event is Blockly.Events.BlockChange & { oldValue: string; newValue: string } {
  return (
    event instanceof Blockly.Events.BlockChange &&
    event.recordUndo &&
    event.element === 'field' &&
    event.name === field &&
    typeof event.oldValue === 'string' &&
    typeof event.newValue === 'string'
  );
}

/**
 * Keeps start values in step with types on `workspace`: a `create` block's with its type, a *set*
 * block's with its variable's type, which `typeOf` gives for a symbol ID (from the latest
 * analysis; `null` when it does not know the symbol). Returns the function that stops it.
 */
export function installStartValueReshaping(
  workspace: Blockly.Workspace,
  typeOf: (symId: string) => StaticType | null,
): () => void {
  const listener = (event: Blockly.Events.Abstract): void => {
    if (!(event instanceof Blockly.Events.BlockChange) || event.blockId === undefined) {
      return;
    }
    const block = workspace.getBlockById(event.blockId);
    if (block === null || block.isInFlyout) {
      return;
    }
    let change: StartValueChange | null = null;
    if (block.type === DECLARE_TYPE && isFieldChange(event, TYPE_FIELD)) {
      change = reshapeStartValue(block, event.oldValue, event.newValue);
    } else if (block.type === SET_TYPE && isFieldChange(event, VAR_FIELD)) {
      change = reshapeSetValue(block, typeOf(event.oldValue), typeOf(event.newValue));
    }
    if (change === null) {
      return;
    }
    // One undo step: the change gets a group if it had none (it is already on the undo stack, so
    // the group is set on it there), and the new value joins it.
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
