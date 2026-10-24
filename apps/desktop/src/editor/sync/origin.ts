/**
 * Which optional keys a loaded block had in its file. A project may leave out a field or an `extra`
 * key that has its catalog default, and may write an empty statement list; the canonical writer
 * keeps whatever the document holds. To save an unchanged project byte for byte (05 §5.2), the
 * sync remembers, per block, which of those keys the file wrote, and writes a key the file left out
 * only once its value differs from the default.
 *
 * The record is kept per workspace by block ID, not by Blockly block object: undoing a delete (or
 * a cut, or a drag to the trash) and redoing it rebuild the block as a new object with the same
 * ID, and that block must still be written as its file had it. Loading a module forgets the
 * records of the canvas it replaces ({@link forgetOrigins}).
 *
 * Blocks the editor creates (from the toolbox, by duplicating, by pasting) get fresh IDs, so they
 * have no origin and are written with every key, as the examples are.
 */
import type { BdmBlock } from '@blocks2cpp/b2c-core-wasm';
import type * as Blockly from 'blockly/core';

/** The optional keys a loaded block had. */
export interface BlockOrigin {
  /** The field names the file wrote. */
  readonly fields: ReadonlySet<string>;
  /** The `extra` keys the file wrote (empty when it wrote no `extra`). */
  readonly extra: ReadonlySet<string>;
  /** The statement inputs the file wrote, including empty lists. */
  readonly statements: ReadonlySet<string>;
}

/** Per workspace: the origins of the blocks loaded into it, by block ID. */
const ORIGINS = new WeakMap<Blockly.Workspace, Map<string, BlockOrigin>>();

/** Remembers which optional keys `node` had, for the block made from it. */
export function rememberOrigin(block: Blockly.Block, node: BdmBlock): void {
  let origins = ORIGINS.get(block.workspace);
  if (origins === undefined) {
    origins = new Map();
    ORIGINS.set(block.workspace, origins);
  }
  origins.set(block.id, {
    fields: new Set(Object.keys(node.fields ?? {})),
    extra: new Set(Object.keys(node.extra ?? {})),
    statements: new Set(Object.keys(node.statements ?? {})),
  });
}

/** Forgets the origin of `block` (it was not kept: a placeholder shows its node instead). */
export function forgetOrigin(block: Blockly.Block): void {
  ORIGINS.get(block.workspace)?.delete(block.id);
}

/** Forgets every origin of `workspace`, whose canvas is being replaced. */
export function forgetOrigins(workspace: Blockly.Workspace): void {
  ORIGINS.delete(workspace);
}

/** The optional keys the block's file had, or `null` for a block the editor created. */
export function originOf(block: Blockly.Block): BlockOrigin | null {
  return ORIGINS.get(block.workspace)?.get(block.id) ?? null;
}
