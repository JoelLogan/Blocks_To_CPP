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
import { headlessWorkspace, renderedBlock, renderedWorkspace, setUpBlocks } from './helpers';

beforeEach(() => {
  setUpBlocks();
});

/** The layout as `kind:name[items]` strings, for readable expectations. */
function describeLayout(def: BlockDefJson): string[] {
  return blockLayout(def).inputs.map((input: LayoutInput) => {
    const items = input.items.map((item) => (item.kind === 'label' ? `"${item.text}"` : item.name));
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
      // Every catalog field exists, and no connection carries Blockly type checks.
      for (const field of blockDef.fields) {
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
    expect(kindsOf('func.call', 'FUNC')).toBe('functions');
    const char = workspace.newBlock('text.char').getField('VALUE');
    expect(char instanceof B2cTextField && char.singleChar).toBe(true);
    const text = workspace.newBlock('text.literal').getField('VALUE');
    expect(text instanceof B2cTextField && text.singleChar).toBe(false);
  });
});

describe('the catalog tables of this package', () => {
  it('name a mutator for exactly the blocks with repeated or optional parts', () => {
    const needsMutator = BLOCK_DEFS.filter(
      (blockDef) =>
        blockDef.extra.length > 0 ||
        blockDef.inputs.some((input) => input.repeat !== null) ||
        blockDef.statements.some(
          (statement) => statement.repeat !== null || statement.when !== null,
        ),
    ).map((blockDef) => blockDef.id);
    expect(Object.keys(MUTATOR_FOR_BLOCK).sort()).toEqual(needsMutator.sort());
    expect(new Set(Object.values(MUTATOR_FOR_BLOCK))).toEqual(new Set(Object.values(MUTATOR_NAME)));
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
    expect(describeLayout(def('var.declare'))).toEqual([
      'value:VALUE["create" TYPE "variable" NAME "="]',
      'dummy:b2c_row0[CONST]',
    ]);
    expect(describeLayout(def('math.arithmetic'))).toEqual(['value:A[]', 'value:B[OP]']);
    expect(describeLayout(def('text.char'))).toEqual([`dummy:b2c_row0["letter '" VALUE "'"]`]);
    expect(describeLayout(def('program.main'))).toEqual([
      'dummy:b2c_row0["when program starts"]',
      'statement:BODY[]',
    ]);
  });

  it('leaves repeated and optional parts to the mutator, at the repeat anchor', () => {
    expect(describeLayout(def('io.print'))).toEqual([
      `anchor:${REPEAT_ANCHOR}[]`,
      'dummy:b2c_row0[SEP NEWLINE STREAM]',
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
        { name: 'MODE', kind: 'checkbox', options: [], types: [], default: false },
        { name: 'EXTRA', kind: 'checkbox', options: [], types: [], default: false },
      ],
      inputs: [{ name: 'LIMIT', check: 'integer', optional: true, repeat: null, default: [] }],
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

  it('places every catalog field exactly once, and every fixed input', () => {
    for (const blockDef of BLOCK_DEFS) {
      const layout = blockLayout(blockDef);
      const fields = layout.inputs.flatMap((input) =>
        input.items.flatMap((item) => (item.kind === 'field' ? [item.name] : [])),
      );
      expect(fields.sort()).toEqual(blockDef.fields.map((field) => field.name).sort());
      const owned = new Set(layout.mutatorParts);
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
