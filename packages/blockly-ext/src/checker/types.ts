/**
 * Types shared by the connection checker's modules.
 *
 * `StaticType` is the analyser's static type as the editor receives it (06 §6.6, the JSON of
 * `b2c_ir::Type`). It is structurally identical to `StaticType` of `@blocks2cpp/b2c-core-wasm`;
 * it is declared here because this package does not depend on that one yet, and it is not
 * re-exported from the package entry so that the two never clash.
 */
import type * as Blockly from 'blockly/core';

/** A static type: `string` is `std::string`; `error` is compatible with everything. */
export type StaticType = 'void' | 'bool' | 'char' | 'int' | 'double' | 'string' | 'error';

/** Every static type, in the order of b2c-lang's own tables. */
export const STATIC_TYPES: readonly StaticType[] = [
  'void',
  'bool',
  'char',
  'int',
  'double',
  'string',
  'error',
];

/** Whether a value received from outside (the analysis, the oracle) is a known static type. */
export function isStaticType(value: unknown): value is StaticType {
  return typeof value === 'string' && (STATIC_TYPES as readonly string[]).includes(value);
}

/**
 * How a value of one static type converts to another, by name: `b2c_lang::Conversion::name()`.
 * Only `invalid` is an analyser error.
 */
export type StaticConversion = 'same' | 'widening' | 'narrowing' | 'boolNumber' | 'invalid';

/**
 * Whether a value may go into an input: `ok` (silently), `warning` (the analyser warns, the
 * connection is allowed) or `invalid` (the analyser reports an error, so the connection is
 * refused while dragging).
 */
export type Compatibility = 'ok' | 'warning' | 'invalid';

/**
 * Answers the static type of a reporter from the latest analysis (the WASM preview's
 * `blockTypes` and symbol types). Structurally the same as `TypeOracle` in the editor services,
 * so the editor can pass its oracle unchanged. `null` means "not known".
 */
export interface OutputTypeOracle {
  outputTypeOf(block: Blockly.Block): StaticType | null;
}
