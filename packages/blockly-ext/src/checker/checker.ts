/**
 * The type-aware connection checker (03 §3.3, 03 §3.5.3).
 *
 * It keeps all of Blockly's own checks (safety: no self or cross-workspace connections, matching
 * connection kinds, shadows; drag: distance, insertion markers, occupied connections; and the
 * `check` arrays) and adds two kinds of rule:
 *
 * * **Structure**, always: a hat or definition never goes inside anything; a statement never goes
 *   into a value input; a reporter or predicate never goes into a statement list.
 * * **Types**, while the user drags or moves a block: a value is refused only where the analyser
 *   would report an error (`conversionAllowed` is `invalid`). Unknown and erroneous types always
 *   connect. Connections the program makes (loading a project, undo and redo) never apply the type
 *   rule: Blockly throws when a saved connection is refused, and a project may legitimately hold a
 *   type error that the analyser reports.
 *
 * Register it with `registerB2cConnectionChecker()` (done when this module loads) and inject with
 * `plugins: { connectionChecker: 'b2c_checker' }`.
 */
import * as Blockly from 'blockly/core';

import type { Shape } from '../generated/catalog';
import { blockDef, inputDef } from './catalog';
import { conversionAllowed } from './conversion';
import { checkerTypeOracle } from './oracle';
import { staticOutputType } from './output-type';
import type { OutputTypeOracle, StaticType } from './types';

/** Connection kinds as plain numbers (`Connection.type` is a number). */
const INPUT_VALUE: number = Blockly.ConnectionType.INPUT_VALUE;
const NEXT_STATEMENT: number = Blockly.ConnectionType.NEXT_STATEMENT;

/** The name the checker is registered under (`plugins: { connectionChecker: B2C_CHECKER_NAME }`). */
export const B2C_CHECKER_NAME = 'b2c_checker';

/** The catalog shape of a block, or `null` for a block outside the catalog. */
function shapeOf(block: Blockly.Block): Shape | null {
  return blockDef(block.type)?.shape ?? null;
}

/**
 * Whether the structural rules allow `child` (through its output or previous connection) to go
 * into `parent` (a value input, a statement input or a next connection).
 */
function structureAllows(parent: Blockly.Connection, child: Blockly.Connection): boolean {
  const shape = shapeOf(child.getSourceBlock());
  if (shape === 'hat' || shape === 'definition') {
    return false;
  }
  if (parent.type === INPUT_VALUE) {
    return shape !== 'statement';
  }
  if (parent.type === NEXT_STATEMENT) {
    return shape !== 'reporter' && shape !== 'predicate';
  }
  return true;
}

/** The static type of the value a block gives, from the catalog or the oracle. */
function valueType(block: Blockly.Block, oracle: OutputTypeOracle): StaticType | null {
  const def = blockDef(block.type);
  if (def !== undefined) {
    return staticOutputType(block, def, oracle);
  }
  try {
    return oracle.outputTypeOf(block);
  } catch {
    return null;
  }
}

/** Whether the type rule allows the value of `child` in the value input `parent`. */
function typeAllows(
  parent: Blockly.Connection,
  child: Blockly.Connection,
  oracle: OutputTypeOracle,
): boolean {
  if (parent.type !== INPUT_VALUE) {
    return true;
  }
  const parentDef = blockDef(parent.getSourceBlock().type);
  const inputName = parent.getParentInput()?.name;
  if (parentDef === undefined || inputName === undefined) {
    return true;
  }
  const input = inputDef(parentDef, inputName);
  if (input === undefined) {
    return true;
  }
  return conversionAllowed(valueType(child.getSourceBlock(), oracle), input.check) !== 'invalid';
}

/**
 * Blockly's connection checker plus the structural rules and, while dragging, the type rule of
 * 03 §3.5.3.
 */
export class B2cConnectionChecker extends Blockly.ConnectionChecker {
  override canConnectWithReason(
    a: Blockly.Connection | null,
    b: Blockly.Connection | null,
    isDragging: boolean,
    opt_distance?: number,
  ): number {
    const reason = super.canConnectWithReason(a, b, isDragging, opt_distance);
    if (reason !== Blockly.Connection.CAN_CONNECT || a === null || b === null) {
      return reason;
    }
    const [parent, child] = a.isSuperior() ? [a, b] : [b, a];
    if (!structureAllows(parent, child)) {
      return Blockly.Connection.REASON_CHECKS_FAILED;
    }
    if (isDragging && !typeAllows(parent, child, checkerTypeOracle())) {
      return Blockly.Connection.REASON_CHECKS_FAILED;
    }
    return Blockly.Connection.CAN_CONNECT;
  }
}

/**
 * Registers `B2cConnectionChecker` as Blockly's connection checker named `b2c_checker`. Calling it
 * again (or loading this module again, as hot reloading does) replaces the registration.
 */
export function registerB2cConnectionChecker(): void {
  Blockly.registry.register(
    Blockly.registry.Type.CONNECTION_CHECKER,
    B2C_CHECKER_NAME,
    B2cConnectionChecker,
    true,
  );
}

registerB2cConnectionChecker();
