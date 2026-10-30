/**
 * The mutator foundations of milestone M3's wave W2 (docs/plans/m3-contracts/w2.md §2.1), on
 * W2-shaped definitions added to the catalog for this file (./w2-fixtures.ts): repeated fields,
 * the joining words on later copies, flag-only branches, call arguments named by a struct and the
 * row extras.
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { blockDef } from '../checker/catalog';
import { registerB2cFields } from '../fields/register';
import { B2cSymbolDeclField } from '../fields/symbol-decl';
import type { BlockDefJson } from '../generated/catalog';
import { MutatorButton } from './buttons';
import { buttonKey } from './controller';
import { MutatorConfigError, MutatorStateError } from './errors';
import { configureMutators } from './hooks';
import { isB2cMutatorBlock, refreshMutatorLabels, registerB2cMutators } from './register';
import { rowsSpec, variadicSpec } from './spec';
import { readRowsExtra, writeRow } from './state';
import {
  defineHelperBlocks,
  defineTestBlocks,
  flushEvents,
  inputNames,
  labelTexts,
} from './test-fixtures';
import type { B2cMutatorBlock } from './types';
import { labelParts, w2Def } from './w2-fixtures';

vi.mock('../generated/catalog', async (importOriginal) => {
  const original = await importOriginal<typeof import('../generated/catalog')>();
  const { W2_DEFS } = await import('./w2-fixtures');
  return { ...original, BLOCK_DEFS: [...original.BLOCK_DEFS, ...W2_DEFS] };
});

let workspace: Blockly.Workspace;

beforeAll(() => {
  registerB2cFields();
  registerB2cMutators();
  defineHelperBlocks();
});

beforeEach(() => {
  defineTestBlocks();
  workspace = new Blockly.Workspace();
});

afterEach(() => {
  configureMutators({ symbols: null, inputShadow: null, newSymbolId: null, declName: null });
  workspace.dispose();
});

function create(type: string): B2cMutatorBlock {
  const block = workspace.newBlock(type);
  if (!isB2cMutatorBlock(block)) {
    throw new Error(`${type} has no b2c mutator`);
  }
  return block;
}

function buttonKeys(block: Blockly.Block): (string | undefined)[] {
  return block.inputList.flatMap((input) =>
    input.fieldRow.filter((field) => field instanceof MutatorButton).map(buttonKey),
  );
}

function press(block: Blockly.Block, key: string): void {
  const button = block.inputList
    .flatMap((input) => input.fieldRow)
    .find(
      (field): field is MutatorButton => field instanceof MutatorButton && buttonKey(field) === key,
    );
  if (button === undefined) {
    throw new Error(`no button ${key}: ${buttonKeys(block).join(', ')}`);
  }
  button.showEditor();
}

function declOf(block: Blockly.Block, name: string): { sym: string; name: string } | null {
  const field = block.getField(name);
  return field instanceof B2cSymbolDeclField ? field.getDecl() : null;
}

describe('specs of W2-shaped blocks', () => {
  it('read repeated fields as members of the group, with the joining words (try)', () => {
    const spec = variadicSpec(w2Def('error.try'));
    expect(spec.groupLeading).toEqual(['catch']);
    expect(spec.joinTexts).toEqual(['catch']);
    expect(spec.count).toEqual({ name: 'catchCount', min: 1, max: 8, default: 1 });
    expect(spec.members.map((member) => [member.name, member.kind, member.leading])).toEqual([
      ['ERROR', 'field', []],
      ['NAME', 'field', ['as']],
      ['CATCH', 'statement', []],
    ]);
    expect(spec.anchorArgs).toEqual([]);
  });

  it('read the joining words of switch, shown before every later case', () => {
    const spec = variadicSpec(w2Def('control.switch'));
    expect(spec.groupLeading).toEqual(['case']);
    expect(spec.joinTexts).toEqual(['case']);
    expect(spec.members.map((member) => member.name)).toEqual(['CASE', 'DO']);
    expect(spec.gated).toEqual([{ name: 'DEFAULT', flag: 'hasDefault', leading: ['default'] }]);
  });

  it('read flag-only branches: no count, the words before go with the gated part', () => {
    const spec = variadicSpec(w2Def('file.for_each_line'));
    expect(spec.count).toBeNull();
    expect(spec.members).toEqual([]);
    expect(spec.groupLeading).toEqual([]);
    expect(spec.gated).toEqual([
      { name: 'FAILED', flag: 'hasFailed', leading: ["if it can't be opened"] },
    ]);
    expect(spec.flags).toEqual([{ name: 'hasFailed', default: false }]);
  });

  it('read the row extras fields and values', () => {
    const struct = rowsSpec(w2Def('type.struct'));
    expect(struct).toMatchObject({
      kind: 'params',
      rowKind: 'fields',
      groupLeading: ['has'],
      name: 'fields',
      max: 64,
      typeClass: 'value',
      rowInputs: [],
      anchorArgs: ['COMPARABLE', 'PRINTABLE', 'SHARED'],
    });
    const values = rowsSpec(w2Def('type.enum'));
    expect(values).toMatchObject({ rowKind: 'values', groupLeading: ['values'], types: [] });
  });

  it('read the inputs a row extra counts as row parts, not as a group (DEFAULT)', () => {
    const spec = rowsSpec(blockDefOf('func.define'));
    expect(spec.rowInputs.map((input) => input.name)).toEqual(['DEFAULT']);
    expect(spec.anchorArgs).toEqual(['RETURNS', 'SHARED', 'BODY']);
  });

  it('refuse what they cannot draw', () => {
    const tryDef = w2Def('error.try');
    const onlyFields: BlockDefJson = {
      ...tryDef,
      labelParts: labelParts('try %TRY catch %ERROR as %NAME catch …'),
      statements: tryDef.statements.filter((statement) => statement.name !== 'CATCH'),
    };
    expect(() => variadicSpec(onlyFields)).toThrow(/besides its fields/);
    const textField: BlockDefJson = {
      ...tryDef,
      fields: tryDef.fields.map((field) =>
        field.name === 'ERROR' ? { ...field, kind: 'text' as const } : field,
      ),
    };
    expect(() => variadicSpec(textField)).toThrow(/cannot be repeated/);
    const misnamed: BlockDefJson = {
      ...w2Def('type.enum'),
      extra: [
        {
          name: 'rows',
          kind: 'values',
          min: null,
          max: 64,
          default: null,
          types: [],
          typeClass: null,
        },
      ],
    };
    expect(() => rowsSpec(misnamed)).toThrow(MutatorConfigError);
    const fieldsWithoutTypes: BlockDefJson = {
      ...w2Def('type.struct'),
      extra: [
        {
          name: 'fields',
          kind: 'fields',
          min: null,
          max: 64,
          default: null,
          types: [],
          typeClass: null,
        },
      ],
    };
    expect(() => rowsSpec(fieldsWithoutTypes)).toThrow(/types/);
  });
});

/** A catalog definition (the mocked catalog has the real blocks too). */
function blockDefOf(id: string): BlockDefJson {
  const found = blockDef(id);
  if (found === undefined) {
    throw new Error(`no catalog block ${id}`);
  }
  return found;
}

describe('repeated fields on the canvas (try)', () => {
  it('shows each copy’s fields with its words in front of the copy’s mouth', () => {
    const block = create('error.try');
    expect(block.b2cGetExtra()).toEqual({ catchCount: 1 });
    expect(inputNames(block)).toEqual(['TRY', 'CATCH0', 'B2C_BUTTONS', 'b2c_repeat']);
    expect(labelTexts(block, 'CATCH0')).toEqual(['catch', 'as']);
    const catch0 = block.getInput('CATCH0');
    expect(catch0?.fieldRow.map((field) => field.name).filter(Boolean)).toEqual([
      'ERROR0',
      'NAME0',
    ]);
    expect(block.getFieldValue('ERROR0')).toBe('std::exception');
  });

  it('adds a copy with the joining word, a fresh symbol and the default name', () => {
    let next = 0;
    configureMutators({
      newSymbolId: () => `sym_new${String((next += 1))}`,
      declName: (_block, field) => (field === 'NAME' ? 'e' : null),
    });
    const block = create('error.try');
    press(block, 'count:add');
    expect(block.b2cGetExtra()).toEqual({ catchCount: 2 });
    expect(inputNames(block)).toEqual(['TRY', 'CATCH0', 'CATCH1', 'B2C_BUTTONS', 'b2c_repeat']);
    // The second copy reads "catch" again (w2.md §2.1, joining words).
    expect(labelTexts(block, 'CATCH1')).toEqual(['catch', 'as']);
    expect(declOf(block, 'NAME0')).toEqual({ sym: 'sym_new1', name: 'e' });
    expect(declOf(block, 'NAME1')).toEqual({ sym: 'sym_new2', name: 'e' });
  });

  it('keeps a removed copy’s fields, so undo brings back the same symbol and type', async () => {
    const block = create('error.try');
    block.b2cSetExtra({ catchCount: 2 });
    block.setFieldValue('std::runtime_error', 'ERROR1');
    const decl = declOf(block, 'NAME1');
    expect(decl).not.toBeNull();
    await flushEvents();
    press(block, 'count:remove');
    expect(block.getField('NAME1')).toBeNull();
    await flushEvents();
    workspace.undo(false);
    expect(block.b2cGetExtra()).toEqual({ catchCount: 2 });
    expect(declOf(block, 'NAME1')).toEqual(decl);
    expect(block.getFieldValue('ERROR1')).toBe('std::runtime_error');
  });

  it('refuses a count it cannot show', () => {
    const block = create('error.try');
    expect(() => {
      block.b2cSetExtra({ catchCount: 'two' });
    }).toThrow(MutatorStateError);
  });
});

describe('joined copies on the canvas (switch)', () => {
  it('shows "case" before every case and "default" with its part', () => {
    const block = create('control.switch');
    expect(block.b2cGetExtra()).toEqual({ caseCount: 2, hasDefault: true });
    expect(labelTexts(block, 'CASE0')).toEqual(['case']);
    expect(labelTexts(block, 'CASE1')).toEqual(['case']);
    expect(labelTexts(block, 'DEFAULT')).toEqual(['default']);
    press(block, 'count:add');
    expect(labelTexts(block, 'CASE2')).toEqual(['case']);
  });
});

describe('flag-only branches on the canvas (file loop)', () => {
  it('adds and removes the gated part with its words, and keeps no count', async () => {
    const block = create('file.for_each_line');
    expect(block.b2cGetExtra()).toEqual({ hasFailed: false });
    expect(buttonKeys(block)).toEqual(['flag:hasFailed:add']);
    expect(labelTexts(block, 'B2C_BUTTONS')).toEqual(["if it can't be opened"]);
    press(block, 'flag:hasFailed:add');
    expect(block.b2cGetExtra()).toEqual({ hasFailed: true });
    expect(labelTexts(block, 'FAILED')).toEqual(["if it can't be opened"]);
    expect(buttonKeys(block)).toEqual(['flag:hasFailed:remove']);
    await flushEvents();
    workspace.undo(false);
    expect(block.getInput('FAILED')).toBeNull();
    expect(() => {
      block.b2cSetExtra({ hasFailed: false, count: 1 });
    }).toThrow(MutatorStateError);
  });
});

describe('call arguments named by a struct (new … with)', () => {
  it('labels and counts the parts by the struct’s fields', () => {
    const names: Record<string, string> = { f_name: 'name', f_mark: 'mark' };
    configureMutators({
      symbols: {
        symbolsAt: () => [],
        nameOf: (id) => names[id] ?? null,
        structFields: (_block, structId) =>
          structId === 's_student' ? ['f_name', 'f_mark'] : null,
      },
    });
    const block = create('type.new');
    expect(block.b2cGetExtra()).toEqual({ fieldCount: 0 });
    // Nothing chosen: no struct, so the count buttons follow the catalog limits.
    expect(buttonKeys(block)).toEqual(['count:add']);
    block.setFieldValue('{"ref":"s_student"}', 'TYPE');
    refreshMutatorLabels(workspace);
    expect(buttonKeys(block)).toEqual(['count:add']);
    block.b2cSetExtra({ fieldCount: 2 });
    refreshMutatorLabels(workspace);
    expect(labelTexts(block, 'FIELD0')).toEqual(['with', 'name:']);
    expect(labelTexts(block, 'FIELD1')).toEqual(['mark:']);
    expect(buttonKeys(block)).toEqual([]);
  });

  it('looks the struct up among the symbols at the block without structFields', () => {
    configureMutators({
      symbols: {
        symbolsAt: () => [{ id: 's_point', kind: 'struct', fields: ['f_x'] }],
        nameOf: (id) => (id === 'f_x' ? 'x' : null),
      },
    });
    const block = create('type.new');
    block.setFieldValue('{"ref":"s_point"}', 'TYPE');
    block.b2cSetExtra({ fieldCount: 1 });
    refreshMutatorLabels(workspace);
    expect(labelTexts(block, 'FIELD0')).toEqual(['with', 'x:']);
  });
});

describe('rows of the three row extras', () => {
  it('read and write params, fields and values rows with their own keys', () => {
    const fields = rowsSpec(w2Def('type.struct'));
    const rows = readRowsExtra(fields, {
      fields: [
        { sym: 's_name', name: 'name', type: 'std::string' },
        { sym: 's_marks', name: 'marks', type: { 'std::vector': 'int' } },
      ],
    });
    expect(rows).toEqual([
      { sym: 's_name', name: 'name', type: 'std::string', mode: null },
      { sym: 's_marks', name: 'marks', type: '{"std::vector":"int"}', mode: null },
    ]);
    expect(rows.map((row) => writeRow(fields, row))).toEqual([
      { sym: 's_name', name: 'name', type: 'std::string' },
      { sym: 's_marks', name: 'marks', type: { 'std::vector': 'int' } },
    ]);
    expect(() =>
      readRowsExtra(fields, { fields: [{ sym: 's_x', name: 'x', type: 'int', mode: 'copy' }] }),
    ).toThrow(/Field 1 has an unknown key "mode"/);
    const values = rowsSpec(w2Def('type.enum'));
    expect(readRowsExtra(values, { values: [{ sym: 'v_a', name: 'Deposit' }] })).toEqual([
      { sym: 'v_a', name: 'Deposit', type: null, mode: null },
    ]);
    expect(() =>
      readRowsExtra(values, { values: [{ sym: 'v_a', name: 'A', type: 'int' }] }),
    ).toThrow(/Value 1 has an unknown key "type"/);
    expect(() => readRowsExtra(values, { values: 'A' })).toThrow(/at most 64 values/);
  });
});

describe('the SHARED field of W2’s definitions', () => {
  it('is a plain anchor of the rows mutator', () => {
    expect(rowsSpec(w2Def('type.enum')).anchorArgs).toEqual(['SHARED']);
  });
});
