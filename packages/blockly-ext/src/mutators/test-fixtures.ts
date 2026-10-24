/**
 * Test support for the mutator and connection-checker tests (not exported by the package).
 *
 * `defineTestBlocks()` defines every catalog block the way blockly-ext's block registration lays
 * it out: the label walked in order, words and fields gathered in front of the next value or
 * statement input or on a dummy row `b2c_row<k>`, the words just before a repeated group left to
 * the mutator, an empty `b2c_repeat` dummy input where the group goes, then the mutator applied.
 * Blockly's built-in fields stand in for blockly-ext's fields, and `ValueField` holds the object
 * values of declarations and references. Nothing here depends on block registration itself.
 */
import * as Blockly from 'blockly/core';

import { BLOCK_DEFS, type BlockDefJson, type FieldDefJson } from '../generated/catalog';
import { REGISTRATION_ANCHOR } from './parts';
import {
  B2C_MUTATOR_CALL_ARGS,
  B2C_MUTATOR_IF,
  B2C_MUTATOR_ITEMS,
  B2C_MUTATOR_PARAMS,
  type B2cMutatorName,
} from './types';

/** The mutator of each catalog block with variable parts (as block registration maps them). */
export const TEST_MUTATOR_FOR_BLOCK: Readonly<Record<string, B2cMutatorName>> = {
  'io.print': B2C_MUTATOR_ITEMS,
  'text.join': B2C_MUTATOR_ITEMS,
  'logic.operation': B2C_MUTATOR_ITEMS,
  'control.if': B2C_MUTATOR_IF,
  'func.call': B2C_MUTATOR_CALL_ARGS,
  'func.call_stmt': B2C_MUTATOR_CALL_ARGS,
  'func.define': B2C_MUTATOR_PARAMS,
};

/** A field holding any value, such as `{ref}` or `{sym, name}`. */
export class ValueField extends Blockly.Field<unknown> {
  override SERIALIZABLE = true;

  protected override getText_(): string {
    return JSON.stringify(this.getValue());
  }
}

type Item = { readonly label: string } | { readonly field: FieldDefJson };

interface TestInput {
  readonly kind: 'dummy' | 'value' | 'statement' | 'anchor';
  readonly name: string;
  readonly items: readonly Item[];
}

/** The layout of block registration (see the module comment). */
function layout(def: BlockDefJson, withAnchor: boolean): TestInput[] {
  const fields = new Map(def.fields.map((field) => [field.name, field]));
  const values = new Set(def.inputs.map((input) => input.name));
  const statements = new Set(def.statements.map((statement) => statement.name));
  const variable = new Set([
    ...def.inputs.filter((input) => input.repeat !== null).map((input) => input.name),
    ...def.statements
      .filter((statement) => statement.repeat !== null || statement.when !== null)
      .map((statement) => statement.name),
  ]);
  const inputs: TestInput[] = [];
  const mentioned = new Set<string>();
  let pending: Item[] = [];
  let rows = 0;
  let inGroup = false;
  const flush = (): void => {
    if (pending.length > 0) {
      inputs.push({ kind: 'dummy', name: `b2c_row${String(rows)}`, items: pending });
      rows += 1;
      pending = [];
    }
  };
  /** Starts a group: the words just before it are the mutator's; the anchor marks the place. */
  const startGroup = (): void => {
    while (pending.length > 0 && 'label' in (pending[pending.length - 1] ?? {})) {
      pending.pop();
    }
    flush();
    if (withAnchor && !inputs.some((input) => input.kind === 'anchor')) {
      inputs.push({ kind: 'anchor', name: REGISTRATION_ANCHOR, items: [] });
    }
  };
  for (const part of def.labelParts) {
    if ('repeat' in part) {
      if (!inGroup) {
        startGroup();
      }
      inGroup = false;
    } else if ('text' in part) {
      if (!inGroup) {
        pending.push({ label: part.text });
      }
    } else {
      mentioned.add(part.arg);
      const field = fields.get(part.arg);
      if (variable.has(part.arg)) {
        if (!inGroup) {
          startGroup();
          inGroup = true;
        }
      } else if (field !== undefined) {
        pending.push({ field });
      } else if (values.has(part.arg) || statements.has(part.arg)) {
        inputs.push({
          kind: values.has(part.arg) ? 'value' : 'statement',
          name: part.arg,
          items: pending,
        });
        pending = [];
        inGroup = false;
      }
    }
  }
  for (const field of def.fields) {
    if (!mentioned.has(field.name)) {
      pending.push({ field });
    }
  }
  flush();
  for (const name of def.statementsNotInLabel) {
    if (!variable.has(name)) {
      inputs.push({ kind: 'statement', name, items: [] });
    }
  }
  return inputs;
}

/** A built-in field standing in for a blockly-ext field of the given catalog kind. */
function testField(def: FieldDefJson): Blockly.Field {
  const text = typeof def.default === 'string' ? def.default : null;
  const dropdown = (options: [string, string][]): Blockly.Field => {
    const field = new Blockly.FieldDropdown(options);
    if (text !== null) {
      field.setValue(text);
    }
    return field;
  };
  switch (def.kind) {
    case 'dropdown':
      return dropdown(def.options.map(([label, value]) => [label, value]));
    case 'type':
      return dropdown(def.types.map((type) => [type, type]));
    case 'checkbox':
      return new Blockly.FieldCheckbox(def.default === true);
    case 'text':
    case 'number':
      return new Blockly.FieldTextInput(text ?? '');
    case 'symbol_decl':
    case 'symbol_ref':
      return new ValueField(null);
  }
}

/** Options of `defineTestBlocks`. */
export interface TestBlockOptions {
  /** Place the `b2c_repeat` anchor (default true); without it the mutators use the label. */
  readonly anchor?: boolean;
}

/** Defines every catalog block for tests (see the module comment). Call `registerB2cMutators` first. */
export function defineTestBlocks(options: TestBlockOptions = {}): void {
  const withAnchor = options.anchor ?? true;
  for (const def of BLOCK_DEFS) {
    const inputs = layout(def, withAnchor);
    const mutator = TEST_MUTATOR_FOR_BLOCK[def.id];
    Blockly.Blocks[def.id] = {
      init(this: Blockly.Block): void {
        for (const spec of inputs) {
          const input =
            spec.kind === 'value'
              ? this.appendValueInput(spec.name)
              : spec.kind === 'statement'
                ? this.appendStatementInput(spec.name)
                : this.appendDummyInput(spec.name);
          for (const item of spec.items) {
            if ('label' in item) {
              input.appendField(new Blockly.FieldLabel(item.label));
            } else {
              input.appendField(testField(item.field), item.field.name);
            }
          }
        }
        if (def.shape === 'statement') {
          this.setPreviousStatement(true);
          this.setNextStatement(true);
        } else if (def.shape === 'reporter' || def.shape === 'predicate') {
          this.setOutput(true);
        }
        this.setInputsInline(true);
        if (mutator !== undefined) {
          Blockly.Extensions.apply(mutator, this, true);
        }
      },
    };
  }
}

/** A block type outside the catalog: a reporter with a text field, used as a shadow. */
export const TEST_SHADOW = 'test_shadow';

/** A block type outside the catalog: a plain reporter of unknown type. */
export const TEST_UNKNOWN_REPORTER = 'test_unknown_reporter';

/** Defines `TEST_SHADOW` and `TEST_UNKNOWN_REPORTER`. */
export function defineHelperBlocks(): void {
  Blockly.Blocks[TEST_SHADOW] = {
    init(this: Blockly.Block): void {
      this.appendDummyInput().appendField(new Blockly.FieldTextInput(''), 'TEXT');
      this.setOutput(true);
    },
  };
  Blockly.Blocks[TEST_UNKNOWN_REPORTER] = {
    init(this: Blockly.Block): void {
      this.appendDummyInput().appendField(new Blockly.FieldLabel('?'));
      this.setOutput(true);
    },
  };
}

/** Waits until Blockly has fired the queued events (after an animation frame and a timeout). */
export function flushEvents(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      setTimeout(() => {
        setTimeout(resolve, 0);
      }, 0);
    });
  });
}

/** The names of a block's inputs, in order. */
export function inputNames(block: Blockly.Block): string[] {
  return block.inputList.map((input) => input.name);
}

/** The texts of a block's label fields, in order (empty labels left out). */
export function labelTexts(block: Blockly.Block, inputName?: string): string[] {
  const inputs =
    inputName === undefined
      ? block.inputList
      : block.inputList.filter((input) => input.name === inputName);
  return inputs.flatMap((input) =>
    input.fieldRow
      .filter((field) => field instanceof Blockly.FieldLabel)
      .map((field) => field.getText())
      .filter((text) => text !== ''),
  );
}
