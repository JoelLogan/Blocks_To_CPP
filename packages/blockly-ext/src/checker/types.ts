/**
 * Types shared by the connection checker's modules.
 *
 * `StaticType` is the analyser's static type as an object (`b2c_ir::Type`, ../types/static-type.ts):
 * the analysis sends it as its term (`TypeJson`, docs/plans/m3-contracts/types.md §3.1), and the
 * editor reads it with `parseType`.
 */
import type * as Blockly from 'blockly/core';

import type { StaticConversion, StaticType } from '../types/static-type';

export type { StaticConversion, StaticType };

/**
 * Whether a value received from outside (an oracle) is a static type object. The oracles build
 * their answers with `parseType`, so only the outer shape is checked here.
 */
export function isStaticType(value: unknown): value is StaticType {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const kind: unknown = (value as { kind?: unknown }).kind;
  return kind === 'prim' || kind === 'lib' || kind === 'named' || kind === 'error';
}

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
