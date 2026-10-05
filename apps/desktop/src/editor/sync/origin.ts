/**
 * Which optional keys a loaded block had in its file. A project may leave out a field or an `extra`
 * key that has its catalog default, and may write an empty statement list; the canonical writer
 * keeps whatever the document holds. To save an unchanged project byte for byte (05 §5.2), the
 * sync remembers, per Blockly block, which of those keys the file wrote, and writes a key the file
 * left out only once its value differs from the default.
 *
 * Blocks the editor creates (from the toolbox, by duplicating, by pasting) have no origin and are
 * written with every key, as the examples are.
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

const ORIGINS = new WeakMap<Blockly.Block, BlockOrigin>();

/** Remembers which optional keys `node` had, for the block made from it. */
export function rememberOrigin(block: Blockly.Block, node: BdmBlock): void {
  ORIGINS.set(block, {
    fields: new Set(Object.keys(node.fields ?? {})),
    extra: new Set(Object.keys(node.extra ?? {})),
    statements: new Set(Object.keys(node.statements ?? {})),
  });
}

/** The optional keys the block's file had, or `null` for a block the editor created. */
export function originOf(block: Blockly.Block): BlockOrigin | null {
  return ORIGINS.get(block) ?? null;
}
