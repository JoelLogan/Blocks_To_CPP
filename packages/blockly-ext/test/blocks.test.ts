/** Block registration from the catalog, layout, placeholders and block state (src/blocks/). */
import type { BdmBlock } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';
import { beforeEach, describe, expect, it } from 'vitest';

import {
  BLOCK_DEFS,
  BlockRegistrationError,
  CATALOG_FIELD_OPTIONS,
  EXPR_SHADOW_TYPE,
  MUTATOR_FOR_BLOCK,
  MUTATOR_NAME,
  PLACEHOLDER_TYPE,
  REPEAT_ANCHOR,
  SHARED_FIELD,
  ZELOS_OUTPUT_SHAPE,
  B2cSymbolRefField,
  B2cTextField,
  blockDefinition,
  blockLayout,
  blockStyleFor,
  catalogBlock,
  hasB2cMutator,
  isManuallyDisabled,
  isPlaceholder,
  missingPackOf,
  placeholderState,
  readPlaceholder,
  registerB2cBlocks,
  setManuallyDisabled,
  type BlockDefJson,
  type LayoutInput,
} from '../src';
import { w2Def } from '../src/mutators/w2-fixtures';
import { headlessWorkspace, renderedBlock, renderedWorkspace, setUpBlocks } from './helpers';

beforeEach(() => {
  setUpBlocks();
});

/** The layout as `kind:name[items]` strings, for readable expectations. */
function describeLayout(def: BlockDefJson): string[] {
  return blockLayout(def).inputs.map((input: LayoutInput) => {
    const items = input.items.map((item) => {
      const text = item.kind === 'label' ? `"${item.text}"` : item.name;
      return item.hidden === true ? `(${text})` : text;
    });
    return `${input.kind}:${input.name}[${items.join(' ')}]`;
  });
}

function def(id: string): BlockDefJson {
  const found = catalogBlock(id);
  if (found === undefined) {
    throw new Error(`no catalog block ${id}`);
  }
  return found;
}

describe('registerB2cBlocks', () => {
  it('defines every catalog block, the expression shadows and the placeholder, idempotently', () => {
    registerB2cBlocks();
    for (const block of BLOCK_DEFS) {
      expect(Object.hasOwn(Blockly.Blocks, block.id)).toBe(true);
    }
    for (const type of Object.values(EXPR_SHADOW_TYPE)) {
      expect(Object.hasOwn(Blockly.Blocks, type)).toBe(true);
    }
    expect(Object.hasOwn(Blockly.Blocks, PLACEHOLDER_TYPE)).toBe(true);
  });

  it('instantiates every catalog block with the connections of its shape (headless)', () => {
    const workspace = headlessWorkspace();
    for (const blockDef of BLOCK_DEFS) {
      const block = workspace.newBlock(blockDef.id);
      const statement = blockDef.shape === 'statement';
      const value = blockDef.shape === 'reporter' || blockDef.shape === 'predicate';
      expect(block.previousConnection !== null, `${blockDef.id} previous`).toBe(statement);
      expect(block.nextConnection !== null, `${blockDef.id} next`).toBe(statement);
      expect(block.outputConnection !== null, `${blockDef.id} output`).toBe(value);
      const expectedShape =
        blockDef.shape === 'reporter'
          ? ZELOS_OUTPUT_SHAPE.round
          : blockDef.shape === 'predicate'
            ? ZELOS_OUTPUT_SHAPE.hexagonal
            : null;
      expect(block.getOutputShape(), `${blockDef.id} output shape`).toBe(expectedShape);
      expect(block.getStyleName()).toBe(blockStyleFor(blockDef.category, blockDef.shape));
      expect(block.getInputsInline()).toBe(true);
      // Every catalog field exists, and no connection carries Blockly type checks. A repeated
      // field (w2.md §2.1) is the mutator's to draw in each copy of its group (blocks/layout.ts),
      // so the stub mutators here leave it out.
      for (const field of blockDef.fields.filter((candidate) => candidate.repeat === null)) {
        expect(block.getField(field.name), `${blockDef.id}.${field.name}`).not.toBeNull();
      }
      for (const input of block.inputList) {
        expect(input.connection?.getCheck() ?? null).toBeNull();
      }
    }
  });

  it('gives hats and definitions a statement mouth and no other connection', () => {
    const workspace = headlessWorkspace();
    for (const id of ['program.main', 'func.define']) {
      const block = workspace.newBlock(id);
      expect(block.getInput('BODY')?.type).toBe(Blockly.inputs.inputTypes.STATEMENT);
      expect(block.previousConnection).toBeNull();
      expect(block.nextConnection).toBeNull();
      expect(block.outputConnection).toBeNull();
    }
  });

  it('renders every catalog block with Zelos, its tooltip as plain text, and labels as SVG text', () => {
    const workspace = renderedWorkspace();
    for (const blockDef of BLOCK_DEFS) {
      const block = renderedBlock(workspace, blockDef.id);
      expect(block.getTooltip()).toBe(blockDef.help);
      expect(block.getSvgRoot().querySelector('foreignObject')).toBeNull();
    }
    // A label from the catalog is an SVG <text> with a plain text node (Blockly keeps spaces as
    // no-break spaces).
    const main = workspace.getBlocksByType('program.main', false)[0] as
      Blockly.BlockSvg | undefined;
    const texts = [...(main?.getSvgRoot().querySelectorAll('text') ?? [])].map(
      (text) => text.textContent,
    );
    expect(texts.join(' ').replaceAll('\u00a0', ' ')).toContain('when program starts');
  });

  it('uses the Zelos output shape numbers', () => {
    const constants = new Blockly.zelos.ConstantProvider();
    expect(constants.SHAPES.HEXAGONAL).toBe(ZELOS_OUTPUT_SHAPE.hexagonal);
    expect(constants.SHAPES.ROUND).toBe(ZELOS_OUTPUT_SHAPE.round);
  });

  it('sets the symbol-reference kinds and the single-character rule per block', () => {
    const workspace = headlessWorkspace();
    const kindsOf = (type: string, field: string) => {
      const found = workspace.newBlock(type).getField(field);
      return found instanceof B2cSymbolRefField ? found.kinds : null;
    };
    expect(kindsOf('var.get', 'VAR')).toBe('variables');
    expect(kindsOf('var.set', 'VAR')).toBe('assignable');
    expect(kindsOf('io.ask', 'VAR')).toBe('assignable');
    // change counts with its operators: text and true/false variables cannot take part.
    expect(kindsOf('var.change', 'VAR')).toBe('numeric');
    expect(kindsOf('func.call', 'FUNC')).toBe('functions');
    const char = workspace.newBlock('text.char').getField('VALUE');
    expect(char instanceof B2cTextField && char.singleChar).toBe(true);
    const text = workspace.newBlock('text.literal').getField('VALUE');
    expect(text instanceof B2cTextField && text.singleChar).toBe(false);
  });
});

describe('the catalog tables of this package', () => {
  // Until W2's integration step restores exact equality (docs/plans/m3-contracts/w2.md §2.13,
  // §5.3), the table may name blocks the catalog does not have yet: every catalog block that needs
  // a mutator has one, and every catalog block named needs one.
  it('name a mutator for every catalog block with repeated or optional parts, and no other', () => {
    const needsMutator = new Set(
      BLOCK_DEFS.filter(
        (blockDef) =>
          blockDef.extra.length > 0 ||
          blockDef.inputs.some((input) => input.repeat !== null) ||
          blockDef.fields.some((field) => field.repeat !== null) ||
          blockDef.statements.some(
            (statement) => statement.repeat !== null || statement.when !== null,
          ),
      ).map((blockDef) => blockDef.id),
    );
    for (const id of needsMutator) {
      expect(MUTATOR_FOR_BLOCK[id], id).toBeDefined();
    }
    for (const id of Object.keys(MUTATOR_FOR_BLOCK)) {
      if (catalogBlock(id) !== undefined) {
        expect(needsMutator.has(id), id).toBe(true);
      }
    }
    expect(new Set(Object.values(MUTATOR_FOR_BLOCK))).toEqual(new Set(Object.values(MUTATOR_NAME)));
  });

  it('name the mutators of W2’s blocks before the catalog has them (w2.md §2.13)', () => {
    expect(MUTATOR_FOR_BLOCK['coll.list_of']).toBe(MUTATOR_NAME.items);
    expect(MUTATOR_FOR_BLOCK['file.write']).toBe(MUTATOR_NAME.items);
    for (const id of ['control.switch', 'error.try', 'file.for_each_line', 'file.open']) {
      expect(MUTATOR_FOR_BLOCK[id], id).toBe(MUTATOR_NAME.if);
    }
    expect(MUTATOR_FOR_BLOCK['type.new']).toBe(MUTATOR_NAME.callArgs);
    expect(MUTATOR_FOR_BLOCK['type.struct']).toBe(MUTATOR_NAME.params);
    expect(MUTATOR_FOR_BLOCK['type.enum']).toBe(MUTATOR_NAME.params);
  });

  it('give the reference fields of W2’s blocks their kinds (w2.md §2.13)', () => {
    expect(CATALOG_FIELD_OPTIONS['coll.add']?.['LIST']?.kinds).toBe('assignable');
    expect(CATALOG_FIELD_OPTIONS['coll.item']?.['LIST']?.kinds).toBe('variables');
    expect(CATALOG_FIELD_OPTIONS['type.enum_value']?.['ENUM']?.kinds).toBe('enums');
    expect(CATALOG_FIELD_OPTIONS['var.flip']?.['VAR']?.kinds).toBe('assignable');
  });

  it('give every symbol reference field its kinds', () => {
    for (const blockDef of BLOCK_DEFS) {
      for (const field of blockDef.fields) {
        if (field.kind === 'symbol_ref') {
          expect(
            CATALOG_FIELD_OPTIONS[blockDef.id]?.[field.name]?.kinds,
            `${blockDef.id}.${field.name}`,
          ).toBeDefined();
        }
      }
    }
  });
});

describe('blockLayout', () => {
  it('puts labels and fields in front of the input that follows them', () => {
    expect(describeLayout(def('control.repeat'))).toEqual([
      'value:TIMES["repeat"]',
      'dummy:b2c_row0["times"]',
      'statement:BODY[]',
    ]);
    // The words naming a checkbox come right before it (03 §3.11.1).
    expect(describeLayout(def('var.declare'))).toEqual([
      'value:VALUE["create" TYPE "variable" NAME "="]',
      'dummy:b2c_row0["constant" CONST]',
    ]);
    expect(describeLayout(def('math.arithmetic'))).toEqual(['value:A[]', 'value:B[OP]']);
    expect(describeLayout(def('text.char'))).toEqual([`dummy:b2c_row0["character '" VALUE "'"]`]);
    expect(describeLayout(def('program.main'))).toEqual([
      'dummy:b2c_row0["when program starts"]',
      'statement:BODY[]',
    ]);
  });

  it('leaves repeated and optional parts to the mutator, at the repeat anchor', () => {
    expect(describeLayout(def('io.print'))).toEqual([
      `anchor:${REPEAT_ANCHOR}[]`,
      'dummy:b2c_row0[SEP "new line" NEWLINE "to" STREAM]',
    ]);
    expect(describeLayout(def('text.join'))).toEqual([`anchor:${REPEAT_ANCHOR}[]`]);
    expect(describeLayout(def('logic.operation'))).toEqual([
      `anchor:${REPEAT_ANCHOR}[]`,
      'dummy:b2c_row0[OP]',
    ]);
    expect(describeLayout(def('control.if'))).toEqual([`anchor:${REPEAT_ANCHOR}[]`]);
    expect(describeLayout(def('func.call'))).toEqual([
      'dummy:b2c_row0[FUNC]',
      `anchor:${REPEAT_ANCHOR}[]`,
    ]);
    expect(describeLayout(def('func.define'))).toEqual([
      'dummy:b2c_row0["define" NAME]',
      `anchor:${REPEAT_ANCHOR}[]`,
      'dummy:b2c_row1["returns" RETURNS]',
      'statement:BODY[]',
    ]);
    expect(blockLayout(def('control.if')).mutatorParts).toEqual(['COND', 'DO', 'ELSE']);
  });

  it('handles statements in the label and inputs the label leaves out', () => {
    const synthetic: BlockDefJson = {
      ...def('control.repeat'),
      id: 'test.synthetic',
      labelParts: [{ text: 'try' }, { arg: 'BODY' }, { text: 'then' }, { arg: 'MODE' }],
      statementsNotInLabel: ['AFTER'],
      fields: [
        {
          name: 'MODE',
          kind: 'checkbox',
          options: [],
          types: [],
          default: false,
          type: null,
          repeat: null,
        },
        {
          name: 'EXTRA',
          kind: 'checkbox',
          options: [],
          types: [],
          default: false,
          type: null,
          repeat: null,
        },
      ],
      inputs: [
        {
          name: 'LIMIT',
          check: 'integer',
          optional: true,
          repeat: null,
          default: [],
          type: null,
          assign: false,
        },
      ],
      statements: [
        { name: 'BODY', repeat: null, when: null },
        { name: 'AFTER', repeat: null, when: null },
      ],
    };
    expect(describeLayout(synthetic)).toEqual([
      'statement:BODY["try"]',
      'value:LIMIT["then" MODE EXTRA]',
      'statement:AFTER[]',
    ]);
  });

  it('leaves repeated fields to the mutator and hides SHARED with its words (w2.md §2.1)', () => {
    expect(describeLayout(w2Def('error.try'))).toEqual([
      'statement:TRY["try"]',
      `anchor:${REPEAT_ANCHOR}[]`,
    ]);
    expect(blockLayout(w2Def('error.try')).mutatorParts).toEqual(['CATCH', 'ERROR', 'NAME']);
    expect(describeLayout(w2Def('type.struct'))).toEqual([
      'dummy:b2c_row0["struct" NAME]',
      `anchor:${REPEAT_ANCHOR}[]`,
      'dummy:b2c_row1["comparable" COMPARABLE "printable" PRINTABLE ("share with other modules") (SHARED)]',
    ]);
    // A SHARED field the label leaves out is hidden too.
    const unlabelled: BlockDefJson = {
      ...w2Def('type.enum'),
      labelParts: [{ text: 'enum' }, { arg: 'NAME' }, { text: 'values' }, { repeat: true }],
    };
    expect(describeLayout(unlabelled)).toEqual([
      'dummy:b2c_row0["enum" NAME]',
      `anchor:${REPEAT_ANCHOR}[]`,
      'dummy:b2c_row1[(SHARED)]',
    ]);
  });

  it('creates the hidden SHARED field with its value but does not show it', () => {
    const struct: BlockDefJson = { ...w2Def('type.struct'), id: 'test.struct_shared' };
    Blockly.Blocks['test.struct_shared'] = blockDefinition(struct);
    try {
      const block = headlessWorkspace().newBlock(struct.id);
      const shared = block.getField(SHARED_FIELD);
      expect(shared).not.toBeNull();
      expect(shared?.isVisible()).toBe(false);
      expect(shared?.getValue()).toBe('FALSE');
      const row = block.inputList.find(
        (input) => shared !== null && input.fieldRow.includes(shared),
      );
      const words = row?.fieldRow.filter(
        (field) =>
          field instanceof Blockly.FieldLabel && field.getText() === 'share with other modules',
      );
      expect(words?.map((field) => field.isVisible())).toEqual([false]);
      expect(block.getField('COMPARABLE')?.isVisible()).toBe(true);
    } finally {
      Reflect.deleteProperty(Blockly.Blocks, 'test.struct_shared');
    }
  });

  it('shows the words of the label next to the fields they name, on the created block', () => {
    const workspace = headlessWorkspace();
    /** The text of the label field right before field `name` in its row, or null. */
    const wordsBefore = (type: string, name: string): string | null => {
      const block = workspace.newBlock(type);
      for (const input of block.inputList) {
        const index = input.fieldRow.findIndex((field) => field.name === name);
        if (index >= 0) {
          const before = input.fieldRow[index - 1];
          return before instanceof Blockly.FieldLabel ? before.getText() : null;
        }
      }
      throw new Error(`${type} has no field ${name}`);
    };
    expect(wordsBefore('var.declare', 'CONST')).toBe('constant');
    expect(wordsBefore('io.print', 'NEWLINE')).toBe('new line');
    expect(wordsBefore('io.print', 'STREAM')).toBe('to');
    // Every checkbox of the catalog has words right before it.
    for (const blockDef of BLOCK_DEFS) {
      for (const field of blockDef.fields.filter((candidate) => candidate.kind === 'checkbox')) {
        expect(wordsBefore(blockDef.id, field.name), `${blockDef.id}.${field.name}`).toMatch(
          /\p{L}/u,
        );
      }
    }
  });

  it('places every catalog field exactly once, and every fixed input', () => {
    for (const blockDef of BLOCK_DEFS) {
      const layout = blockLayout(blockDef);
      const fields = layout.inputs.flatMap((input) =>
        input.items.flatMap((item) => (item.kind === 'field' ? [item.name] : [])),
      );
      const owned = new Set(layout.mutatorParts);
      expect(fields.sort()).toEqual(
        blockDef.fields
          .map((field) => field.name)
          .filter((name) => !owned.has(name))
          .sort(),
      );
      const fixedInputs = [
        ...blockDef.inputs.map((input) => input.name),
        ...blockDef.statements.map((s) => s.name),
      ].filter((name) => !owned.has(name));
      const created = layout.inputs
        .filter((input) => input.kind === 'value' || input.kind === 'statement')
        .map((input) => input.name);
      expect(created.sort()).toEqual(fixedInputs.sort());
    }
  });
});

describe('mutator blocks', () => {
  it('get the mutator named for them, with the project extra state', () => {
    const workspace = headlessWorkspace();
    const block = workspace.newBlock('io.print');
    expect(hasB2cMutator(block)).toBe(true);
    if (!hasB2cMutator(block)) {
      return;
    }
    expect(block.b2cGetExtra()).toEqual({ itemCount: 1 });
    block.b2cSetExtra({ itemCount: 3 });
    expect(block.inputList.map((input) => input.name)).toEqual([
      'ITEM0',
      'ITEM1',
      'ITEM2',
      REPEAT_ANCHOR,
      'b2c_row0',
    ]);
  });

  it('cannot be created before the mutators are registered', () => {
    const blockDef = def('text.join');
    const definition = blockDefinition(blockDef);
    const unregistered = { ...blockDef, id: 'test.join_without_mutator' };
    Blockly.Blocks[unregistered.id] = {
      init(this: Blockly.Block) {
        Blockly.Extensions.unregister(MUTATOR_NAME.items);
        try {
          definition.init.call(this);
        } finally {
          // Leave the stub in place for the other tests in this file.
          Blockly.Extensions.registerMutator(MUTATOR_NAME.items, {
            saveExtraState: () => null,
            loadExtraState: () => undefined,
          });
        }
      },
    };
    const workspace = headlessWorkspace();
    expect(() => workspace.newBlock(unregistered.id)).toThrow(BlockRegistrationError);
  });
});

describe('placeholders', () => {
  const node: BdmBlock = {
    id: 'b100',
    type: 'sfml.window.open',
    v: 2,
    fields: { TITLE: 'Game' },
    inputs: { W: { expr: [{ num: '800' }] } },
  };

  it('name the missing pack', () => {
    expect(missingPackOf('sfml.window.open')).toBe('sfml');
    expect(missingPackOf('plain')).toBe('plain');
    expect(missingPackOf('.odd')).toBe('.odd');
  });

  it('keep the original block verbatim through Blockly serialisation', () => {
    const workspace = headlessWorkspace();
    const block = Blockly.serialization.blocks.append(
      placeholderState(node, 'statement'),
      workspace,
    );
    expect(isPlaceholder(block)).toBe(true);
    expect(readPlaceholder(block)).toEqual({ node, shape: 'statement' });
    expect(block.previousConnection).not.toBeNull();
    expect(block.getFieldValue('PACK')).toBe('Missing pack: sfml');
    expect(block.getFieldValue('TYPE')).toBe('sfml.window.open');
    const saved = Blockly.serialization.blocks.save(block);
    expect(saved?.extraState).toEqual({ node, shape: 'statement' });
    // Copies are independent of the block's own data.
    const copy = readPlaceholder(block);
    if (copy !== null) {
      (copy.node.fields as Record<string, unknown>)['TITLE'] = 'changed';
    }
    expect(readPlaceholder(block)?.node.fields).toEqual({ TITLE: 'Game' });
  });

  it('take the connections of where the block sits', () => {
    const workspace = headlessWorkspace();
    const value = Blockly.serialization.blocks.append(placeholderState(node, 'value'), workspace);
    expect(value.outputConnection).not.toBeNull();
    expect(value.previousConnection).toBeNull();
    const top = Blockly.serialization.blocks.append(placeholderState(node, 'top'), workspace);
    expect(top.outputConnection).toBeNull();
    expect(top.previousConnection).toBeNull();
    expect(top.nextConnection).toBeNull();
  });

  it('show hostile type names safely: as text, with invisible characters visible, and short', () => {
    const workspace = renderedWorkspace();
    const hostile: BdmBlock = {
      id: 'b1',
      type: '<img src=x onerror=alert(1)>\u200b ' + 'x'.repeat(100),
      v: 1,
    };
    const block = Blockly.serialization.blocks.append(placeholderState(hostile, 'top'), workspace);
    expect(block.getFieldValue('TYPE')).toContain('⟨U+200B⟩');
    expect(String(block.getFieldValue('TYPE')).length).toBeLessThanOrEqual(40);
    if (block instanceof Blockly.BlockSvg) {
      expect(block.getSvgRoot().querySelector('img')).toBeNull();
    }
  });

  it('ignore malformed extra state', () => {
    const workspace = headlessWorkspace();
    const block = Blockly.serialization.blocks.append(
      { type: PLACEHOLDER_TYPE, extraState: { node: 'x', shape: 'top' } },
      workspace,
    );
    expect(readPlaceholder(block)).toBeNull();
    expect(readPlaceholder(workspace.newBlock('math.number'))).toBeNull();
  });
});

describe('manually disabled', () => {
  it('maps only to Blockly’s MANUALLY_DISABLED reason', () => {
    const workspace = headlessWorkspace();
    const block = workspace.newBlock('io.print');
    expect(isManuallyDisabled(block)).toBe(false);
    block.setDisabledReason(true, 'SOME_PLUGIN_REASON');
    expect(isManuallyDisabled(block)).toBe(false);
    setManuallyDisabled(block, true);
    expect(isManuallyDisabled(block)).toBe(true);
    setManuallyDisabled(block, false);
    expect(isManuallyDisabled(block)).toBe(false);
    expect(block.hasDisabledReason('SOME_PLUGIN_REASON')).toBe(true);
  });
});
