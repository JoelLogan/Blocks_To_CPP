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
  /** Rows: func.define (`params`), type.struct (`fields`), type.enum (`values`). */
  params: 'b2c_mutator_params',
} as const);

/** A mutator extension name. */
export type MutatorName = (typeof MUTATOR_NAME)[keyof typeof MUTATOR_NAME];

/**
 * The mutator of each catalog block that has repeated or optional parts.
 *
 * The blocks of milestone M3's wave W2 are listed before the catalog has them
 * (docs/plans/m3-contracts/w2.md §2.13), so the packages that add them never edit this table; an
 * entry for a block the catalog does not have is never read. *if*'s mutator also draws *switch*'s
 * cases, *try*'s catch parts (with their repeated fields) and the file blocks' flag-only *if it
 * can't be opened*; the call-arguments mutator draws *new … with*'s parts, named by its struct;
 * the parameter mutator draws a struct's `fields` rows and an enum's `values` rows.
 */
export const MUTATOR_FOR_BLOCK: Readonly<Record<string, MutatorName>> = Object.freeze({
  'io.print': MUTATOR_NAME.items,
  'text.join': MUTATOR_NAME.items,
  'logic.operation': MUTATOR_NAME.items,
  'control.if': MUTATOR_NAME.if,
  'func.call': MUTATOR_NAME.callArgs,
  'func.call_stmt': MUTATOR_NAME.callArgs,
  'func.define': MUTATOR_NAME.params,
  // W2 (w2.md §2.13).
  'coll.list_of': MUTATOR_NAME.items,
  'file.write': MUTATOR_NAME.items,
  'control.switch': MUTATOR_NAME.if,
  'error.try': MUTATOR_NAME.if,
  'file.for_each_line': MUTATOR_NAME.if,
  'file.open': MUTATOR_NAME.if,
  'type.new': MUTATOR_NAME.callArgs,
  'type.struct': MUTATOR_NAME.params,
  'type.enum': MUTATOR_NAME.params,
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
