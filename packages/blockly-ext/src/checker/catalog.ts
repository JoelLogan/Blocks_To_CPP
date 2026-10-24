/**
 * Lookups into the generated catalog (packages/blockly-ext/src/generated/catalog.ts) for the
 * connection checker and the mutators.
 */
import { BLOCK_DEFS, type BlockDefJson, type InputDefJson } from '../generated/catalog';

const DEFS_BY_ID: ReadonlyMap<string, BlockDefJson> = new Map(
  BLOCK_DEFS.map((def) => [def.id, def]),
);

/** The catalog definition of a block type, or `undefined` for a type outside the catalog. */
export function blockDef(type: string): BlockDefJson | undefined {
  return DEFS_BY_ID.get(type);
}

/**
 * The index in a repeated part's name: `ITEM3` is part 3 of `ITEM`. Numbers are written without
 * leading zeros, as in project files (05 §5.4), so `ITEM03` is not a part of `ITEM`.
 */
export function partIndex(name: string, base: string): number | null {
  if (!name.startsWith(base)) {
    return null;
  }
  const digits = name.slice(base.length);
  if (!/^(0|[1-9][0-9]{0,5})$/.test(digits)) {
    return null;
  }
  return Number(digits);
}

/**
 * The definition of the value input `name` of a block: a plain input by its name, a repeated one
 * by its numbered name (`ITEM0`, `ITEM1`, …).
 */
export function inputDef(def: BlockDefJson, name: string): InputDefJson | undefined {
  return def.inputs.find((input) =>
    input.repeat === null ? input.name === name : partIndex(name, input.name) !== null,
  );
}
