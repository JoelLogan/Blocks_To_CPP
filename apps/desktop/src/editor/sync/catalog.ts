/**
 * What the sync needs to know about catalog blocks (docs/spec/03-block-language.md §3.11): which
 * Blockly inputs are project-file inputs, and the defaults that let the sync leave unchanged keys
 * out of a file that left them out.
 */
import type { JsonValue } from '@blocks2cpp/b2c-core-wasm';
import {
  type BlockDefJson,
  catalogBlock,
  type ExtraDefJson,
  type FieldDefJson,
  type InputDefJson,
  type Shape,
  type StatementDefJson,
} from '@blocks2cpp/blockly-ext';

/** A repeated part's index (`ITEM12` → 12), as project files write it: no leading zeros. */
const PART_INDEX = /^(?:0|[1-9][0-9]{0,5})$/;

/** Whether `name` is part `base<n>` of a repeated input or statement. */
function isPartOf(name: string, base: string): boolean {
  return (
    name.length > base.length && name.startsWith(base) && PART_INDEX.test(name.slice(base.length))
  );
}

/** The catalog definition of a block type, or `null` for a type outside the catalog. */
export function blockDefOf(type: string): BlockDefJson | null {
  return catalogBlock(type) ?? null;
}

/** The definition of the value input `name` (a repeated one by its numbered name), or `null`. */
export function valueInputDef(def: BlockDefJson, name: string): InputDefJson | null {
  for (const input of def.inputs) {
    if (input.repeat === null ? input.name === name : isPartOf(name, input.name)) {
      return input;
    }
  }
  return null;
}

/** The definition of the statement input `name` (a repeated one by its numbered name), or `null`. */
export function statementDef(def: BlockDefJson, name: string): StatementDefJson | null {
  for (const statement of def.statements) {
    if (statement.repeat === null ? statement.name === name : isPartOf(name, statement.name)) {
      return statement;
    }
  }
  return null;
}

/** The definition of the field `name`, or `null`. */
export function fieldDef(def: BlockDefJson, name: string): FieldDefJson | null {
  return def.fields.find((field) => field.name === name) ?? null;
}

/** The definition of the `extra` key `name`, or `null`. */
export function extraDef(def: BlockDefJson, name: string): ExtraDefJson | null {
  return def.extra.find((extra) => extra.name === name) ?? null;
}

/**
 * The value an `extra` key has when a file leaves it out: a count's or a flag's catalog default, and
 * no rows for `params`.
 */
export function extraDefault(def: ExtraDefJson): JsonValue {
  if (def.kind === 'params') {
    return [];
  }
  return def.default;
}

/** The value a field has when a file leaves it out, or `null` for a field without a default. */
export function fieldDefault(def: FieldDefJson): JsonValue {
  return def.default;
}

/** Where a block may sit, by its catalog shape. */
export type Slot = 'top' | 'value' | 'statement';

/** Whether a block of `shape` fits in `slot` (everything may sit on the canvas). */
export function shapeFits(shape: Shape, slot: Slot): boolean {
  switch (slot) {
    case 'top':
      return true;
    case 'value':
      return shape === 'reporter' || shape === 'predicate';
    case 'statement':
      return shape === 'statement';
  }
}
