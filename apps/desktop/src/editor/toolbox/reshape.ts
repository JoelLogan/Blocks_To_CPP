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
 *
 * The same holds for a block still in the toolbox's flyout, whose type or variable the user chose
 * there: the toolbox keeps its flyout's start values in step too (./plugin.ts), so the block taken
 * out has the start value of the type it shows (`set word to ""`, not `set word to 0`).
 *
 * Every other field change of a block with a catalog signature follows typed slots' rule
 * (ADR-0018, docs/plans/m3-contracts/slots.md §6): each value input whose expected type changed
 * gets the new type's start value while it still holds exactly the old type's, or is left empty
 * when the new type has none (a list, in M3), so an argument's start value follows a changed
 * function. `create` and *set* keep the M2 rules above, which give the same start values.
 */
import type { TypeJson } from '@blocks2cpp/b2c-core-wasm';
import {
  catalogBlock,
  exprShadowState,
  readExprShadow,
  sameType,
  startTokens,
  startValueExpectations,
  tokensEqual,
  type TokenJson,
  type TypeClass,
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

/**
 * An input's connection and what it holds as a start value: its expression shadow's tokens, or
 * none (`[]`) while it is empty. `null` when it holds a block or a draft, or has no connection.
 */
function startValueOf(
  block: Blockly.Block,
  input: string = VALUE_INPUT,
): { connection: Blockly.Connection; tokens: readonly TokenJson[] } | null {
  const connection = block.getInput(input)?.connection ?? null;
  if (connection === null) {
    return null;
  }
  const target = connection.targetBlock();
  if (target === null) {
    return { connection, tokens: [] };
  }
  if (!target.isShadow()) {
    return null;
  }
  const current = readExprShadow(target);
  return current === null || current.draft ? null : { connection, tokens: current.tokens };
}

/** The catalog check of an input part (`ARG2` is part 2 of `ARG`). */
function inputCheck(block: Blockly.Block, input: string): TypeClass {
  const inputs = catalogBlock(block.type)?.inputs ?? [];
  const found =
    inputs.find((candidate) => candidate.name === input) ??
    inputs.find(
      (candidate) =>
        candidate.repeat !== null &&
        input.startsWith(candidate.name) &&
        /^(0|[1-9][0-9]*)$/.test(input.slice(candidate.name.length)),
    );
  return found?.check ?? 'any';
}

/**
 * Puts an expression shadow with `tokens` in input `input` of `block`, or leaves the input empty
 * for no tokens (no events).
 */
function setStartValue(
  block: Blockly.Block,
  connection: Blockly.Connection,
  tokens: readonly TokenJson[],
  input: string = VALUE_INPUT,
): void {
  connection.setShadowState(
    tokens.length === 0 ? null : exprShadowState(tokens, false, inputCheck(block, input), false),
  );
}

/** The event type of {@link StartValueChange}. */
export const START_VALUE_EVENT = 'b2c_start_value';

/**
 * A block's start value was replaced by the start value of its new type (no tokens: the input left
 * empty). Blockly records no event when a shadow is replaced, so without this one undo would
 * restore the type (or the variable) but keep the new value.
 */
export class StartValueChange extends Blockly.Events.BlockBase {
  override type = START_VALUE_EVENT;
  /** The start value before the change. */
  oldTokens: readonly TokenJson[];
  /** The start value after the change. */
  newTokens: readonly TokenJson[];
  /** The value input whose start value changed (slots.md §6): `VALUE` for `create` and *set*. */
  input: string;

  constructor(
    block?: Blockly.Block,
    oldTokens: readonly TokenJson[] = [],
    newTokens: readonly TokenJson[] = [],
    input: string = VALUE_INPUT,
  ) {
    super(block);
    this.oldTokens = oldTokens;
    this.newTokens = newTokens;
    this.input = input;
  }

  override isNull(): boolean {
    return tokensEqual(this.oldTokens, this.newTokens);
  }

  /** Puts the value back (undo) or again (redo). */
  override run(forward: boolean): void {
    const id = this.blockId;
    const block = id === undefined ? null : this.getEventWorkspace_().getBlockById(id);
    const value = block === null ? null : startValueOf(block, this.input);
    if (block === null || value === null) {
      console.warn('The start value to restore is gone');
      return;
    }
    setStartValue(block, value.connection, forward ? this.newTokens : this.oldTokens, this.input);
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
  input: string = VALUE_INPUT,
): StartValueChange | null {
  const value = startValueOf(block, input);
  if (
    value === null ||
    before === null ||
    after === null ||
    before.length === 0 ||
    tokensEqual(before, after) ||
    !tokensEqual(value.tokens, before)
  ) {
    return null;
  }
  setStartValue(block, value.connection, after, input);
  return new StartValueChange(block, before, after, input);
}

/**
 * Typed slots' rule for any other field change (slots.md §6): each value input of `block` whose
 * expected type changed when field `field` went from `oldValue` to its value now gets the new
 * type's start value while it still holds exactly the old type's (none for a type without one:
 * the input is left empty). An unknown type on either side leaves the input alone. Returns the
 * events describing the changes.
 */
export function reshapeExpectedValues(
  block: Blockly.Block,
  field: string,
  oldValue: unknown,
): StartValueChange[] {
  const before = startValueExpectations(block, { name: field, value: oldValue });
  const after = startValueExpectations(block);
  const changes: StartValueChange[] = [];
  for (const [input, now] of after) {
    const was = before.get(input) ?? null;
    if (
      was === null ||
      now === null ||
      was.kind === 'error' ||
      now.kind === 'error' ||
      sameType(was, now)
    ) {
      continue;
    }
    const change = swapStartValue(block, startTokens(was), startTokens(now) ?? [], input);
    if (change !== null) {
      changes.push(change);
    }
  }
  return changes;
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
  oldType: TypeJson | null,
  newType: TypeJson | null,
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

/** Whether an event is the user changing a field of a block (not undo, not loading). */
function isAnyFieldChange(
  event: Blockly.Events.Abstract,
): event is Blockly.Events.BlockChange & { name: string } {
  return (
    event instanceof Blockly.Events.BlockChange &&
    event.recordUndo &&
    event.element === 'field' &&
    typeof event.name === 'string'
  );
}

/**
 * Keeps start values in step with types on `workspace`, the canvas or the toolbox's flyout: a
 * `create` block's with its type, a *set* block's with its variable's type, which `typeOf` gives
 * for a symbol ID (from the latest analysis; `null` when it does not know the symbol). Returns
 * the function that stops it.
 */
export function installStartValueReshaping(
  workspace: Blockly.Workspace,
  typeOf: (symId: string) => TypeJson | null,
): () => void {
  const listener = (event: Blockly.Events.Abstract): void => {
    if (!(event instanceof Blockly.Events.BlockChange) || event.blockId === undefined) {
      return;
    }
    const block = workspace.getBlockById(event.blockId);
    if (block === null) {
      return;
    }
    let changes: StartValueChange[] = [];
    if (block.type === DECLARE_TYPE || block.type === SET_TYPE) {
      // M2's rules (see the module comment).
      let change: StartValueChange | null = null;
      if (block.type === DECLARE_TYPE && isFieldChange(event, TYPE_FIELD)) {
        change = reshapeStartValue(block, event.oldValue, event.newValue);
      } else if (block.type === SET_TYPE && isFieldChange(event, VAR_FIELD)) {
        change = reshapeSetValue(block, typeOf(event.oldValue), typeOf(event.newValue));
      }
      changes = change === null ? [] : [change];
    } else if (isAnyFieldChange(event)) {
      changes = reshapeExpectedValues(block, event.name, event.oldValue);
    }
    if (changes.length === 0) {
      return;
    }
    // One undo step: the change gets a group if it had none (it is already on the undo stack, so
    // the group is set on it there), and the new values join it.
    if (event.group === '') {
      event.group = Blockly.utils.idGenerator.genUid();
    }
    for (const change of changes) {
      change.group = event.group;
      change.recordUndo = true;
      Blockly.Events.fire(change);
    }
  };
  workspace.addChangeListener(listener);
  return () => {
    workspace.removeChangeListener(listener);
  };
}
