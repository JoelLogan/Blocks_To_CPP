/**
 * The type-aware connection checker (03 §3.3, 03 §3.5.3).
 *
 * It keeps all of Blockly's own checks (safety: no self or cross-workspace connections, matching
 * connection kinds, shadows; drag: distance, insertion markers, occupied connections; and the
 * `check` arrays) and adds four kinds of rule:
 *
 * * **Structure**, always: a hat or definition never goes inside anything, but for the
 *   definitions that are namespace members, which go only into a namespace's mouth (w3-modules.md
 *   §2.12); a mouth takes nothing else; a statement never goes into a value input; a reporter or
 *   predicate never goes into a statement list.
 * * **Depth**, while the user drags or moves a block: namespaces nest at most 14 deep.
 * * **Types**, while the user drags or moves a block: the parent is typed as if the value were in
 *   the input (typed slots, ADR-0018: `candidateFit`, ./slots.ts), and the value is refused only
 *   where the analyser would report an error: a conversion or class error between known types
 *   (`isFitError`, asked through `candidateRefused`, which asks about scopes only to confirm a
 *   refusal). The input's expected type comes from the parent's signature: `create`'s type,
 *   `set`'s variable, a call's parameter, a `return`'s function; an input without a pattern checks
 *   its class as in M2. Unknown and erroneous types always connect. Connections the program makes
 *   (loading a project, undo and redo, paste) never apply the type rule: Blockly throws when a
 *   saved connection is refused, and a project may legitimately hold a type error that the
 *   analyser reports.
 * * **Wrapping**, while the user drags an empty statement C-block with the pointer: dropped on a
 *   statement of a list, it takes the statement's place and wraps it and the statements below it
 *   (./wrap.ts), where Blockly would insert it.
 *
 * Register it with `registerB2cConnectionChecker()` (done when this module loads) and inject with
 * `plugins: { connectionChecker: 'b2c_checker' }`.
 */
import * as Blockly from 'blockly/core';

import {
  isInMouth,
  isMouth,
  isNamespaceMember,
  MAX_NAMESPACE_DEPTH,
  namespaceDepthAt,
  namespaceHeight,
} from '../blocks/namespace';
import type { Shape } from '../generated/catalog';
import type { TypeOracle } from '../services';
import { blockDef } from './catalog';
import { checkerTypeOracle } from './oracle';
import { candidateRefused, currentSlotCache } from './slots';
import { wrapPlace } from './wrap';

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
 * Whether `connection` (a statement input or a next connection) holds definitions: it is a
 * namespace's mouth, or the next connection of a block in one (w3-modules.md §2.12).
 */
function holdsDefinitions(connection: Blockly.Connection): boolean {
  const block = connection.getSourceBlock();
  if (block.nextConnection === connection) {
    return isInMouth(block);
  }
  const input = connection.getParentInput();
  return input !== null && isMouth(block.type, input.name);
}

/**
 * Whether the structural rules allow `child` (through its output or previous connection) to go
 * into `parent` (a value input, a statement input or a next connection). A namespace's mouth, and
 * the next connection of a block in one, take only the definitions that are namespace members
 * (and blocks outside the catalog, such as the placeholder of a statement a file put there); those
 * definitions go nowhere else, so nothing ever stacks under a top-level definition.
 */
function structureAllows(parent: Blockly.Connection, child: Blockly.Connection): boolean {
  const childBlock = child.getSourceBlock();
  const shape = shapeOf(childBlock);
  if (parent.type === NEXT_STATEMENT) {
    const definitions = holdsDefinitions(parent);
    if (shape === 'definition' && isNamespaceMember(childBlock.type)) {
      return definitions;
    }
    const below = parent.getSourceBlock().nextConnection === parent;
    if (definitions ? shape !== null : below && shapeOf(parent.getSourceBlock()) === 'definition') {
      // A mouth holds only definitions (and the placeholders of what a file put there), and a
      // top-level definition takes nothing below it.
      return false;
    }
  }
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

/**
 * Whether a drag keeps namespaces within {@link MAX_NAMESPACE_DEPTH} levels: the namespaces
 * around the place and those the dragged blocks bring (w3-modules.md §2.12). Only drags are
 * refused; a hand-edited file loads as it is and the analyser reports it (`B2C-E0219`).
 */
function depthAllows(parent: Blockly.Connection, child: Blockly.Connection): boolean {
  if (parent.type !== NEXT_STATEMENT) {
    return true;
  }
  const height = namespaceHeight(child.getSourceBlock());
  return height === 0 || namespaceDepthAt(parent) + height <= MAX_NAMESPACE_DEPTH;
}

/** Whether the type rule allows the value of `child` in the value input `parent`. */
function typeAllows(
  parent: Blockly.Connection,
  child: Blockly.Connection,
  oracle: TypeOracle,
): boolean {
  if (parent.type !== INPUT_VALUE) {
    return true;
  }
  const parentBlock = parent.getSourceBlock();
  const inputName = parent.getParentInput()?.name;
  if (blockDef(parentBlock.type) === undefined || inputName === undefined) {
    return true;
  }
  try {
    return !candidateRefused(
      parentBlock,
      inputName,
      child.getSourceBlock(),
      oracle,
      currentSlotCache(),
    );
  } catch (error: unknown) {
    // Typing never throws by design; a failure must never break dragging.
    console.warn('A connection could not be typed', error);
    return true;
  }
}

/**
 * Blockly's connection checker plus the structural rules and, while dragging, the type rule of
 * 03 §3.5.3 and the wrapping of statements in a C-block (03 §3.3, ./wrap.ts).
 */
export class B2cConnectionChecker extends Blockly.ConnectionChecker {
  /**
   * Blockly's drag checks, except for a wrap ({@link wrapPlace}): an empty statement C-block
   * dragged with the pointer may take the place of a statement in a list, which then goes into its
   * first mouth with the statements below it. Blockly would insert the C-block there instead (or
   * refuse a C-block with no next connection); a wrap is allowed within `distance` when the
   * statements can go into the mouth. An empty C-block is so never inserted between two
   * statements by the pointer.
   */
  override doDragChecks(
    a: Blockly.RenderedConnection,
    b: Blockly.RenderedConnection,
    distance: number,
  ): boolean {
    const wrap = wrapPlace(a, b);
    if (wrap === null) {
      return super.doDragChecks(a, b, distance);
    }
    const statements = wrap.wrapped.previousConnection;
    return (
      a.distanceFrom(b) <= distance &&
      statements !== null &&
      this.canConnect(wrap.mouth, statements, false)
    );
  }

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
    if (isDragging && !depthAllows(parent, child)) {
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
