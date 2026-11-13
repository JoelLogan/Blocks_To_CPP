/**
 * The *namespace* block's mouth (docs/plans/m3-contracts/w3-modules.md §2.12, §6 W3M-D2).
 *
 * A namespace is a definition with a mouth, `DEFINITIONS`, that holds the definitions marked
 * `namespaceMember` in the catalog: *define*, *struct*, *enum*, *global* and *namespace*. Those
 * blocks have a previous and a next connection, which only a mouth and the definitions in one use:
 * the connection checker (../checker/checker.ts) refuses every other place, so a definition never
 * stacks under a top-level definition and never goes into a statement list. Namespaces nest at
 * most {@link MAX_NAMESPACE_DEPTH} deep; the checker refuses a drop deeper than that (the analyser
 * reports a hand-edited file with `B2C-E0219`).
 *
 * On the canvas a member keeps the hat look of every definition: the renderer draws it as if it
 * had neither connection while it sits at the top level (its previous connection unconnected), and
 * with the notches of a statement inside a mouth. The renderer is Zelos with that one change,
 * registered under Zelos's own name, so every workspace, flyout and test that asks for `zelos`
 * draws the same; blocks without the two connections draw exactly as Zelos draws them.
 */
import * as Blockly from 'blockly/core';

import { blockDef } from '../checker/catalog';

/**
 * How deep namespaces nest (`MAX_NAMESPACE_DEPTH` in crates/b2c-model/src/limits.rs; the analyser
 * reports a deeper one with `B2C-E0219`).
 */
export const MAX_NAMESPACE_DEPTH = 14;

/** The block type of a namespace. */
export const NAMESPACE_TYPE = 'program.namespace';

/** Connection kinds as plain numbers (`Connection.type` is a number). */
const INPUT_VALUE: number = Blockly.ConnectionType.INPUT_VALUE;
const NEXT_STATEMENT: number = Blockly.ConnectionType.NEXT_STATEMENT;

/** Whether a block type is a definition that also fits a namespace's mouth. */
export function isNamespaceMember(type: string): boolean {
  const def = blockDef(type);
  return def?.shape === 'definition' && def.namespaceMember === true;
}

/** Whether the statement input `name` of a block type is a namespace's mouth. */
export function isMouth(type: string, name: string): boolean {
  return blockDef(type)?.statements.some((s) => s.name === name && s.definitions === true) ?? false;
}

/**
 * The statement input of a mouth that `block` sits in, directly or below other definitions of it,
 * or `null` when the block is not in a mouth.
 */
function enclosingMouth(block: Blockly.Block): Blockly.Connection | null {
  let current: Blockly.Block = block;
  for (;;) {
    const target = current.previousConnection?.targetConnection ?? null;
    if (target === null) {
      return null;
    }
    const owner = target.getSourceBlock();
    if (target.type === INPUT_VALUE) {
      return null;
    }
    if (owner.nextConnection === target) {
      // Below another block of the same list.
      current = owner;
      continue;
    }
    const input = target.getParentInput();
    return input !== null && isMouth(owner.type, input.name) ? target : null;
  }
}

/** Whether a block sits in a namespace's mouth (directly or below other definitions there). */
export function isInMouth(block: Blockly.Block): boolean {
  return enclosingMouth(block) !== null;
}

/**
 * How many namespaces a definition list is nested in: for a mouth, its namespace and the
 * namespaces around it; for a next connection, those of the mouth its block is in (0 when the
 * connection's block is not in a mouth).
 */
export function namespaceDepthAt(connection: Blockly.Connection): number {
  let mouth: Blockly.Connection | null =
    connection.type === NEXT_STATEMENT && connection.getSourceBlock().nextConnection === connection
      ? enclosingMouth(connection.getSourceBlock())
      : connection;
  let depth = 0;
  while (mouth !== null) {
    depth += 1;
    mouth = enclosingMouth(mouth.getSourceBlock());
  }
  return depth;
}

/**
 * How many levels of namespaces a block and the blocks below it bring: 0 for definitions without
 * a namespace, 1 for a namespace holding none, and so on.
 */
export function namespaceHeight(block: Blockly.Block | null): number {
  let height = 0;
  // A work list: no recursion along long chains.
  const pending: { readonly block: Blockly.Block; readonly level: number }[] = [];
  for (let b = block; b !== null; b = b.getNextBlock()) {
    pending.push({ block: b, level: 0 });
  }
  while (pending.length > 0) {
    const item = pending.pop();
    if (item?.block.type !== NAMESPACE_TYPE) {
      continue;
    }
    const level = item.level + 1;
    height = Math.max(height, level);
    for (const input of item.block.inputList) {
      if (input.type !== Blockly.inputs.inputTypes.STATEMENT) {
        continue;
      }
      for (let b = input.connection?.targetBlock() ?? null; b !== null; b = b.getNextBlock()) {
        pending.push({ block: b, level });
      }
    }
  }
  return height;
}

/**
 * Runs `draw` with a top-level namespace member's previous and next connections hidden from the
 * renderer, so that it draws the hat of a definition and a flat bottom.
 */
function asTopLevelDefinition(block: Blockly.BlockSvg, draw: () => void): void {
  // Blockly types the connection as always there; a block without one has null.
  const previous = block.previousConnection as Blockly.RenderedConnection | null;
  const hide =
    isNamespaceMember(block.type) &&
    !block.isInsertionMarker() &&
    previous?.isConnected() === false;
  if (!hide) {
    draw();
    return;
  }
  const writable = block as unknown as {
    previousConnection: Blockly.RenderedConnection | null;
    nextConnection: Blockly.RenderedConnection | null;
  };
  const { previousConnection, nextConnection } = block;
  writable.previousConnection = null;
  writable.nextConnection = null;
  try {
    draw();
  } finally {
    writable.previousConnection = previousConnection;
    writable.nextConnection = nextConnection;
  }
}

/** Zelos's measuring, with a top-level namespace member measured as a definition. */
class B2cRenderInfo extends Blockly.zelos.RenderInfo {
  override measure(): void {
    asTopLevelDefinition(this.block_, () => {
      super.measure();
    });
  }
}

/** Zelos with {@link B2cRenderInfo}. */
class B2cZelosRenderer extends Blockly.zelos.Renderer {
  protected override makeRenderInfo_(block: Blockly.BlockSvg): Blockly.zelos.RenderInfo {
    return new B2cRenderInfo(this, block);
  }
}

/**
 * Registers the renderer under Zelos's name (`renderer: 'zelos'`), replacing Blockly's. Called when
 * this module loads; calling it again replaces the registration again.
 */
export function registerB2cRenderer(): void {
  Blockly.registry.register(Blockly.registry.Type.RENDERER, 'zelos', B2cZelosRenderer, true);
}

registerB2cRenderer();
