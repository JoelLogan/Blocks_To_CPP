/**
 * Public types of the variadic mutators (03 §3.2 ⊕/⊖, 05 §5.4 `extra`).
 */
import type * as Blockly from 'blockly/core';

/** Repeated value inputs: `io.print`, `text.join` and `logic.operation` (`itemCount`). */
export const B2C_MUTATOR_ITEMS = 'b2c_mutator_items';
/** The *else if* and *else* parts of `control.if` (`elseIfCount`, `hasElse`). */
export const B2C_MUTATOR_IF = 'b2c_mutator_if';
/** The arguments of `func.call` and `func.call_stmt` (`argCount`). */
export const B2C_MUTATOR_CALL_ARGS = 'b2c_mutator_call_args';
/** The parameter rows of `func.define` (`params`). */
export const B2C_MUTATOR_PARAMS = 'b2c_mutator_params';

/** The name of one of the four mutators, as Blockly extensions. */
export type B2cMutatorName =
  | typeof B2C_MUTATOR_ITEMS
  | typeof B2C_MUTATOR_IF
  | typeof B2C_MUTATOR_CALL_ARGS
  | typeof B2C_MUTATOR_PARAMS;

/** Every mutator name, in the order they are registered. */
export const B2C_MUTATOR_NAMES: readonly B2cMutatorName[] = [
  B2C_MUTATOR_ITEMS,
  B2C_MUTATOR_IF,
  B2C_MUTATOR_CALL_ARGS,
  B2C_MUTATOR_PARAMS,
];

/**
 * The prefix of each kind of row's fields (`B2C_PARAM0_TYPE`, `B2C_FIELD1_NAME`, `B2C_VALUE0_NAME`),
 * which the rows mutator names its rows by.
 */
export const ROW_FIELD_PREFIXES = Object.freeze({
  params: 'B2C_PARAM',
  fields: 'B2C_FIELD',
  values: 'B2C_VALUE',
} as const);

/** The name of the type field of row `index` of a row extra of `kind` (`B2C_PARAM0_TYPE`). */
export function rowTypeFieldName(kind: keyof typeof ROW_FIELD_PREFIXES, index: number): string {
  return `${ROW_FIELD_PREFIXES[kind]}${String(index)}_TYPE`;
}

/** How a parameter is passed (03 §3.7.7): by value, by `&`, or by `const&`. */
export type ParamMode = 'copy' | 'editable' | 'read_only';

/** Every parameter mode, in menu order. */
export const PARAM_MODES: readonly ParamMode[] = ['copy', 'editable', 'read_only'];

/**
 * One row of `func.define`'s `params` extra, exactly as a project file stores it: the parameter's
 * symbol ID, its name, its type (one of the catalog's parameter types, such as `std::string`) and
 * how it is passed.
 */
export interface ParamRow {
  sym: string;
  name: string;
  type: string;
  mode: ParamMode;
}

/**
 * One row of any row extra (docs/plans/m3-contracts/w2.md §2.1), as the rows mutator keeps it: a
 * `params` row has a type and a mode, a `fields` row a type (`mode` `null`), a `values` row neither.
 * A type is kept by its key, as type fields keep one.
 */
export interface RowState {
  sym: string;
  name: string;
  type: string | null;
  mode: ParamMode | null;
}

/**
 * A block with one of the b2c mutators. Its mutation state is exactly the BDM `extra` of the block
 * (05 §5.4): counts as numbers, flags as booleans, `params` as an array of rows.
 */
export interface B2cMutatorBlock extends Blockly.Block {
  /** The block's `extra`, with every key of its catalog schema. */
  b2cGetExtra(): Record<string, unknown>;
  /**
   * Shapes the block for `extra` (absent keys take their catalog defaults). It fires no mutation
   * event, so it is not an undo step of its own; a user block in a part it removes is unplugged and
   * kept, with Blockly's ordinary move events. Throws `MutatorStateError`, without changing the
   * block, for a value the block cannot show.
   */
  b2cSetExtra(extra: Record<string, unknown>): void;
}
