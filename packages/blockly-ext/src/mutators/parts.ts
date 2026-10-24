/**
 * Adding, removing and placing the inputs a mutator owns.
 *
 * Removing a part never deletes a user's block: a block connected to the part is unplugged (with
 * the blocks below it, for a statement list) and, on a rendered workspace, bumped a little away,
 * so it stays on the canvas. Those moves are ordinary Blockly events and belong to the undo step
 * of the change. Shadow blocks (the default values shown in an empty input) are not user blocks:
 * they are removed with the input, silently, and their state is kept per block so that the same
 * part comes back with the same value when it is added again or the change is undone.
 */
import * as Blockly from 'blockly/core';

/**
 * The name of the empty dummy input block registration places where a block's repeated group
 * starts (`REPEAT_ANCHOR` in blockly-ext's block registration).
 */
export const REGISTRATION_ANCHOR = 'b2c_repeat';

/** Runs `change` with Blockly events disabled, so it records nothing and notifies no one. */
export function silently<T>(change: () => T): T {
  Blockly.Events.disable();
  try {
    return change();
  } finally {
    Blockly.Events.enable();
  }
}

/** Moves a block that was just unplugged a little away from where it was, as Blockly bumps. */
function bump(block: Blockly.Block): void {
  if (!(block instanceof Blockly.BlockSvg) || block.isInFlyout || block.isDeadOrDying()) {
    return;
  }
  const offset = Blockly.config.snapRadius;
  block.moveBy(block.RTL ? -offset : offset, offset, ['bump']);
}

/**
 * Removes the input `name` (if present): unplugs and bumps the user's block in it, keeps its
 * shadow's state in `shadows`, and removes the input silently.
 */
export function removePart(
  block: Blockly.Block,
  name: string,
  shadows: Map<string, Blockly.serialization.blocks.State>,
): void {
  const input = block.getInput(name);
  if (input === null) {
    return;
  }
  const connection = input.connection;
  if (connection !== null) {
    const target = connection.targetBlock();
    if (target !== null && !target.isShadow()) {
      // healStack false: a statement list leaves together with the blocks below its first one.
      target.unplug(false);
      bump(target);
    }
    const shadow = connection.getShadowState(true);
    if (shadow === null) {
      shadows.delete(name);
    } else {
      shadows.set(name, shadow);
    }
  }
  silently(() => block.removeInput(name, true));
}

/**
 * Gives a new value input its shadow: the kept one, or else `fallback()` (the editor's factory).
 * A factory that fails, or a shadow Blockly cannot create, leaves the input empty: the part still
 * works, and the analyser reports a missing value if there is one.
 */
export function restoreShadow(
  input: Blockly.Input,
  shadows: Map<string, Blockly.serialization.blocks.State>,
  fallback: () => Blockly.serialization.blocks.State | null,
): void {
  const connection = input.connection;
  if (connection?.targetBlock() !== null) {
    return;
  }
  const kept = shadows.get(input.name);
  shadows.delete(input.name);
  let state: Blockly.serialization.blocks.State | null = kept ?? null;
  if (state === null) {
    try {
      state = fallback();
    } catch {
      state = null;
    }
  }
  if (state === null) {
    return;
  }
  try {
    connection.setShadowState(state);
  } catch {
    // Forget the state, or Blockly would try to create the same shadow again whenever the input
    // becomes empty.
    connection.setShadowState(null);
  }
}

/** Whether `input` holds the field or is the input named `arg`. */
function holds(input: Blockly.Input, arg: string): boolean {
  return input.name === arg || input.fieldRow.some((field) => field.name === arg);
}

/**
 * The input the owned inputs go before: registration's anchor when the block has one (as
 * `block.moveInputBefore(part, REPEAT_ANCHOR)` would place them); otherwise the first input that
 * holds one of `anchorArgs` (in that order). `null` means the end of the block.
 */
function referenceInput(
  block: Blockly.Block,
  isOwned: (input: Blockly.Input) => boolean,
  anchorArgs: readonly string[],
): Blockly.Input | null {
  const anchor = block.getInput(REGISTRATION_ANCHOR);
  if (anchor !== null) {
    return anchor;
  }
  for (const arg of anchorArgs) {
    const input = block.inputList.find((candidate) => !isOwned(candidate) && holds(candidate, arg));
    if (input !== undefined) {
      return input;
    }
  }
  return null;
}

/**
 * Puts the owned inputs `order` together, in that order, at the mutator's place on the block (see
 * `referenceInput`). Inputs no longer on the block are skipped.
 */
export function placeParts(
  block: Blockly.Block,
  order: readonly Blockly.Input[],
  isOwned: (input: Blockly.Input) => boolean,
  anchorArgs: readonly string[],
): void {
  const reference = referenceInput(block, isOwned, anchorArgs);
  for (const input of order) {
    const from = block.inputList.indexOf(input);
    if (from === -1) {
      continue;
    }
    const to = reference === null ? block.inputList.length : block.inputList.indexOf(reference);
    if (from !== to - 1) {
      block.moveNumberedInputBefore(from, to);
    }
  }
}
