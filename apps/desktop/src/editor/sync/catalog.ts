/**
 * What the sync needs to know about catalog blocks (docs/spec/03-block-language.md §3.11): which
 * Blockly inputs are project-file inputs, and the defaults that let the sync leave unchanged keys
 * out of a file that left them out.
 *
 * Milestone M3's wave W2 (docs/plans/m3-contracts/w2.md §2.1) adds repeated fields, whose parts
 * (`ERROR0`, `ERROR1`, …) are fields of the project file like any other, and the row extras
 * `fields` and `values` beside `params`.
 */
import type { JsonValue } from '@blocks2cpp/b2c-core-wasm';
import type * as Blockly from 'blockly/core';
import {
  type BlockDefJson,
  catalogBlock,
  isRowKind,
  type ExtraDefJson,
  type FieldDefJson,
  type InputDefJson,
  type Shape,
  type StatementDefJson,
} from '@blocks2cpp/blockly-ext';

/** A repeated part's index (`ITEM12` → 12), as project files write it: no leading zeros. */
const PART_INDEX = /^(?:0|[1-9][0-9]{0,5})$/;

/** Whether `name` is part `base<n>` of a repeated input, statement or field. */
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

/** The definition of the field `name` (a repeated one by its numbered name), or `null`. */
export function fieldDef(def: BlockDefJson, name: string): FieldDefJson | null {
  for (const field of def.fields) {
    if (field.repeat === null ? field.name === name : isPartOf(name, field.name)) {
      return field;
    }
  }
  return null;
}

/**
 * The catalog fields a block has, in block order, with their names and definitions: each field
 * shown once, and each part of a repeated field the block has now (`NAME0`, `NAME1`, …).
 */
export function catalogFieldsOf(
  block: Blockly.Block,
  def: BlockDefJson,
): { readonly name: string; readonly def: FieldDefJson; readonly field: Blockly.Field }[] {
  const found: { name: string; def: FieldDefJson; field: Blockly.Field }[] = [];
  const repeated = def.fields.some((field) => field.repeat !== null);
  if (!repeated) {
    // The common case, in catalog order as before W2.
    for (const definition of def.fields) {
      const field = block.getField(definition.name);
      if (field !== null) {
        found.push({ name: definition.name, def: definition, field });
      }
    }
    return found;
  }
  for (const input of block.inputList) {
    for (const field of input.fieldRow) {
      const name = field.name;
      const definition = name === undefined ? null : fieldDef(def, name);
      if (name !== undefined && definition !== null) {
        found.push({ name, def: definition, field });
      }
    }
  }
  return found;
}

/** The definition of the `extra` key `name`, or `null`. */
export function extraDef(def: BlockDefJson, name: string): ExtraDefJson | null {
  return def.extra.find((extra) => extra.name === name) ?? null;
}

/**
 * The value an `extra` key has when a file leaves it out: a count's or a flag's catalog default, and
 * no rows for a row extra (`params`, `fields`, `values`).
 */
export function extraDefault(def: ExtraDefJson): JsonValue {
  if (isRowKind(def.kind)) {
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
