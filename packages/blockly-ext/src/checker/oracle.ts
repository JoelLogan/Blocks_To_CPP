/**
 * Where the connection checker gets the types of reporters it cannot work out by itself
 * (`var.get`, `func.call`, expression shadows): the editor's type oracle, which answers from the
 * latest analysis.
 *
 * Blockly creates the checker itself (from the `plugins` inject option), so the oracle cannot be
 * passed to its constructor; the editor sets it here once, when it sets up its services. Until
 * then every such type is "not known", which always connects.
 */
import type { OutputTypeOracle } from './types';

/** An oracle that knows nothing: every type is `null` ("not known"). */
const UNKNOWN: OutputTypeOracle = {
  outputTypeOf: () => null,
};

let current: OutputTypeOracle = UNKNOWN;

/**
 * Sets the oracle the connection checker asks for the types of `symbol` and `any` outputs and of
 * blocks outside the catalog; `null` goes back to knowing nothing.
 */
export function setCheckerTypeOracle(oracle: OutputTypeOracle | null): void {
  current = oracle ?? UNKNOWN;
}

/** The oracle currently in use. */
export function checkerTypeOracle(): OutputTypeOracle {
  return current;
}
