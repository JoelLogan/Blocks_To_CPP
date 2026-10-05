/**
 * Per-block mutator state. Each block with a b2c mutator has one controller, kept here rather than
 * on the block object, so the only members the mutators add to blocks are Blockly's
 * `saveExtraState`/`loadExtraState` and `b2cGetExtra`/`b2cSetExtra`.
 */
import * as Blockly from 'blockly/core';

import { MutatorButton } from './buttons';

/** What every mutator does for its block. */
export interface MutatorController {
  /** The block's `extra`, exactly as a project file stores it. */
  extra(): Record<string, unknown>;
  /** Shapes the block for `extra`; throws `MutatorStateError` without changing anything. */
  apply(extra: unknown): void;
  /** Updates text that comes from the latest analysis (argument labels) or from other fields. */
  refresh(): void;
}

const CONTROLLERS = new WeakMap<Blockly.Block, MutatorController>();

/** The controller of a block, if it has a b2c mutator. */
export function controllerOf(block: Blockly.Block): MutatorController | undefined {
  return CONTROLLERS.get(block);
}

/** Records the controller of a block (when its mutator is applied). */
export function attachController(block: Blockly.Block, controller: MutatorController): void {
  CONTROLLERS.set(block, controller);
}

/** A button wanted in a mutator's button row: a stable key, what it does, and its text. */
export interface ButtonPlan {
  readonly key: string;
  readonly action: 'add' | 'remove';
  readonly label: string;
  readonly onActivate: () => void;
}

/** One item of a button row: a word shown before buttons, or a button. */
export type RowItem = { readonly text: string } | ButtonPlan;

const BUTTON_KEYS = new WeakMap<MutatorButton, string>();

/** The key of a button created by `fillRow`. */
export function buttonKey(button: MutatorButton): string | undefined {
  return BUTTON_KEYS.get(button);
}

/** A row's items as one comparable string: what decides whether the row must be rebuilt. */
function signature(items: readonly RowItem[]): string {
  return JSON.stringify(items.map((item) => ('key' in item ? `b:${item.key}` : `t:${item.text}`)));
}

const ROW_SIGNATURES = new WeakMap<Blockly.Input, string>();

/** A new button for `plan`. */
export function createButton(plan: ButtonPlan): MutatorButton {
  const button = new MutatorButton(plan.action, plan.label, () => {
    plan.onActivate();
  });
  BUTTON_KEYS.set(button, plan.key);
  return button;
}

/** Adds the words and buttons of a row to `input`. */
function appendItems(input: Blockly.Input, items: readonly RowItem[]): void {
  for (const item of items) {
    input.appendField('key' in item ? createButton(item) : new Blockly.FieldLabel(item.text));
  }
}

/**
 * The dummy input `name` of `block`, holding `items`. An existing row whose items have not changed
 * is kept as it is (so a button that stays keeps its focus); otherwise the row is replaced. Row
 * inputs hold no connections, so replacing one records nothing. The caller places the row.
 */
export function buttonRow(
  block: Blockly.Block,
  name: string,
  items: readonly RowItem[],
): Blockly.Input {
  const wanted = signature(items);
  const existing = block.getInput(name);
  if (existing !== null && ROW_SIGNATURES.get(existing) === wanted) {
    return existing;
  }
  block.removeInput(name, true);
  const input = block.appendDummyInput(name);
  appendItems(input, items);
  ROW_SIGNATURES.set(input, wanted);
  return input;
}

/** The button with `key` on `block`, if any. */
function findButton(block: Blockly.Block, key: string): MutatorButton | null {
  for (const input of block.inputList) {
    for (const field of input.fieldRow) {
      if (field instanceof MutatorButton && BUTTON_KEYS.get(field) === key) {
        return field;
      }
    }
  }
  return null;
}

/** Any button on `block`, preferring the one with `key`. */
function replacementButton(block: Blockly.Block, key: string): MutatorButton | null {
  const same = findButton(block, key);
  if (same !== null) {
    return same;
  }
  for (const input of block.inputList) {
    const button = input.fieldRow.find((field) => field instanceof MutatorButton);
    if (button instanceof MutatorButton) {
      return button;
    }
  }
  return null;
}

/**
 * Runs `change`, keeping keyboard focus usable: when the button with `key` had focus and is
 * replaced, focus moves to its replacement, another button of the block, or the block.
 */
export function keepingFocus(block: Blockly.Block, key: string, change: () => void): void {
  const manager = Blockly.getFocusManager();
  const focused = block instanceof Blockly.BlockSvg ? manager.getFocusedNode() : null;
  const hadFocus = focused instanceof MutatorButton && BUTTON_KEYS.get(focused) === key;
  change();
  if (!hadFocus || !(block instanceof Blockly.BlockSvg) || block.isDeadOrDying()) {
    return;
  }
  const next = replacementButton(block, key);
  if (next === focused) {
    return;
  }
  manager.focusNode(next?.canBeFocused() === true ? next : block);
}
