/**
 * How a catalog block is laid out as Blockly inputs, from its Friendly label (docs/spec/03 §3.11.1,
 * M2 decision "Label template grammar"). A pure function of the block definition, so registration
 * and the mutators agree on it and it can be tested without Blockly.
 *
 * Blockly lays a block out as a list of inputs; each input shows a row of fields (labels and
 * editable fields) followed by its connection (a value socket, a statement mouth, or nothing for a
 * dummy input). The label template is walked in order:
 *
 * - text and fields gather until the next value or statement input, which shows them in front of
 *   it; what is left at the end goes on a dummy input;
 * - fields and value inputs the label does not mention follow at the end, then the statement inputs
 *   the label does not mention (`statementsNotInLabel`);
 * - repeated inputs and statements, and statements that exist only while an `extra` flag is set,
 *   belong to the block's mutator. Where such a group starts, an empty dummy input named
 *   {@link REPEAT_ANCHOR} marks the place. The label text in front of a mutator part (for example
 *   `print` before `ITEM0`, or `else if`) is the mutator's to show, as are the label words between
 *   the parts of a group; the group ends at the template's `…`.
 */
import type { BlockDefJson } from '../generated/catalog';
import { REPEAT_ANCHOR } from './mutators';

/** One item in a row: a fixed label or a catalog field. */
export type LayoutItem =
  | { readonly kind: 'label'; readonly text: string }
  | { readonly kind: 'field'; readonly name: string };

/** One Blockly input of a block. */
export interface LayoutInput {
  /** Dummy (a row of fields), value socket, statement mouth, or the mutator anchor. */
  readonly kind: 'dummy' | 'value' | 'statement' | 'anchor';
  /** The input name: the catalog name, `b2c_row<k>` for a dummy, {@link REPEAT_ANCHOR}. */
  readonly name: string;
  /** The labels and fields shown in front of its connection. */
  readonly items: readonly LayoutItem[];
}

/** A block's layout. */
export interface BlockLayout {
  readonly inputs: readonly LayoutInput[];
  /** The inputs and statements the block's mutator creates (catalog base names, such as `ITEM`). */
  readonly mutatorParts: readonly string[];
}

/** The name of the `index`-th dummy input of fields. */
export function rowName(index: number): string {
  return `b2c_row${String(index)}`;
}

/** Computes the layout of a block (see the module comment). */
export function blockLayout(def: BlockDefJson): BlockLayout {
  const fieldNames = new Set(def.fields.map((field) => field.name));
  const valueInputs = new Map(def.inputs.map((input) => [input.name, input]));
  const statements = new Map(def.statements.map((statement) => [statement.name, statement]));
  const mutatorParts = new Set<string>([
    ...def.inputs.filter((input) => input.repeat !== null).map((input) => input.name),
    ...def.statements
      .filter((statement) => statement.repeat !== null || statement.when !== null)
      .map((statement) => statement.name),
  ]);

  const inputs: LayoutInput[] = [];
  const mentioned = new Set<string>();
  let pending: LayoutItem[] = [];
  let rows = 0;
  let inGroup = false;

  const flushRow = (): void => {
    if (pending.length > 0) {
      inputs.push({ kind: 'dummy', name: rowName(rows++), items: pending });
      pending = [];
    }
  };
  /** Starts a group of mutator parts: ends the fixed row before it and places the anchor. */
  const openGroup = (): void => {
    // The label words right before the group are the group's own (shown by the mutator).
    while (pending.length > 0 && pending[pending.length - 1]?.kind === 'label') {
      pending.pop();
    }
    flushRow();
    if (inputs[inputs.length - 1]?.kind !== 'anchor') {
      inputs.push({ kind: 'anchor', name: REPEAT_ANCHOR, items: [] });
    }
  };

  for (const part of def.labelParts) {
    if ('repeat' in part) {
      // The `…` closes the repeated group (opening it first if no part of it came before).
      if (!inGroup) {
        openGroup();
      }
      inGroup = false;
    } else if ('text' in part) {
      if (!inGroup) {
        pending.push({ kind: 'label', text: part.text });
      }
    } else {
      const name = part.arg;
      mentioned.add(name);
      if (mutatorParts.has(name)) {
        if (!inGroup) {
          openGroup();
        }
        inGroup = true;
      } else if (fieldNames.has(name)) {
        pending.push({ kind: 'field', name });
      } else if (valueInputs.has(name)) {
        inputs.push({ kind: 'value', name, items: pending });
        pending = [];
        inGroup = false;
      } else if (statements.has(name)) {
        inputs.push({ kind: 'statement', name, items: pending });
        pending = [];
        inGroup = false;
      }
    }
  }

  for (const field of def.fields) {
    if (!mentioned.has(field.name)) {
      pending.push({ kind: 'field', name: field.name });
    }
  }
  for (const input of def.inputs) {
    if (!mentioned.has(input.name) && !mutatorParts.has(input.name)) {
      inputs.push({ kind: 'value', name: input.name, items: pending });
      pending = [];
    }
  }
  flushRow();
  for (const name of def.statementsNotInLabel) {
    if (!mutatorParts.has(name)) {
      inputs.push({ kind: 'statement', name, items: [] });
    }
  }
  return { inputs, mutatorParts: [...mutatorParts] };
}
