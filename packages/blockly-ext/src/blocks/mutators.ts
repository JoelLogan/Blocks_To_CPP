/**
 * The contract between block registration and the ⊕/⊖ mutators (docs/spec/03-block-language.md
 * §3.2, M2 decision "Label template grammar"). Registration builds the fixed parts of a block; the
 * mutator named here adds the repeated parts (`ITEM0`, `ITEM1`, …) and the parts that exist only
 * while a flag is set (`ELSE`), and keeps the project's `extra` state.
 *
 * The mutators themselves are registered by `registerB2cMutators()` (src/mutators/).
 */
import type * as Blockly from 'blockly/core';

/** The mutator extension names. */
export const MUTATOR_NAME = Object.freeze({
  /** Variadic items: io.print, text.join, logic.operation (`itemCount`). */
  items: 'b2c_mutator_items',
  /** if / else if / else: control.if (`elseIfCount`, `hasElse`). */
  if: 'b2c_mutator_if',
  /** Call arguments: func.call, func.call_stmt (`argCount`). */
  callArgs: 'b2c_mutator_call_args',
  /** Parameter rows: func.define (`params`). */
  params: 'b2c_mutator_params',
} as const);

/** A mutator extension name. */
export type MutatorName = (typeof MUTATOR_NAME)[keyof typeof MUTATOR_NAME];

/** The mutator of each catalog block that has repeated or optional parts. */
export const MUTATOR_FOR_BLOCK: Readonly<Record<string, MutatorName>> = Object.freeze({
  'io.print': MUTATOR_NAME.items,
  'text.join': MUTATOR_NAME.items,
  'logic.operation': MUTATOR_NAME.items,
  'control.if': MUTATOR_NAME.if,
  'func.call': MUTATOR_NAME.callArgs,
  'func.call_stmt': MUTATOR_NAME.callArgs,
  'func.define': MUTATOR_NAME.params,
});

/**
 * The name of the empty dummy input that registration places where a block's repeated group
 * starts in its label. A mutator inserts its parts relative to it (for example with
 * `block.moveInputBefore(name, REPEAT_ANCHOR)`); the inputs registration created keep their
 * catalog names, and its rows of fields are dummy inputs named `b2c_row0`, `b2c_row1`, ….
 */
export const REPEAT_ANCHOR = 'b2c_repeat';

/** The `extra` state of a block, exactly as a project file stores it (05 §5.4). */
export type BlockExtra = Record<string, unknown>;

/**
 * What every Blocks2Cpp mutator mixes into its blocks, besides Blockly's `saveExtraState` and
 * `loadExtraState`: the project-file `extra` values (counts as numbers, flags as booleans, `params`
 * as an array of rows), with parts named `NAME0` … `NAME{n-1}`.
 */
export interface B2cMutatorMixin {
  /** The block's `extra`, as a project file stores it. */
  b2cGetExtra(): BlockExtra;
  /** Rebuilds the parts for an `extra` from a project file. */
  b2cSetExtra(extra: BlockExtra): void;
}

/** Whether a block has a Blocks2Cpp mutator. */
export function hasB2cMutator(block: Blockly.Block): block is Blockly.Block & B2cMutatorMixin {
  const candidate = block as Partial<B2cMutatorMixin>;
  return typeof candidate.b2cGetExtra === 'function' && typeof candidate.b2cSetExtra === 'function';
}
