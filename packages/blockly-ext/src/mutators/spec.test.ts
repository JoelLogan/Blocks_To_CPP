/** What each mutator manages, worked out from the catalog definitions. */
import { describe, expect, it } from 'vitest';

import { blockDef } from '../checker/catalog';
import { BLOCK_DEFS, type BlockDefJson } from '../generated/catalog';
import { MutatorConfigError } from './errors';
import { mutatorLabelRegion, paramsSpec, variadicSpec } from './spec';
import { TEST_MUTATOR_FOR_BLOCK } from './test-fixtures';

function def(id: string): BlockDefJson {
  const found = blockDef(id);
  if (found === undefined) {
    throw new Error(`no catalog block ${id}`);
  }
  return found;
}

/** The label parts of a block's region, as text. */
function regionText(id: string): string {
  const block = def(id);
  const region = mutatorLabelRegion(block);
  if (region === null) {
    return '';
  }
  return block.labelParts
    .slice(region.start, region.end + 1)
    .map((part) => ('text' in part ? part.text : 'arg' in part ? `%${part.arg}` : '…'))
    .join(' ');
}

describe('mutatorLabelRegion', () => {
  it('marks the label parts each variadic block’s mutator draws', () => {
    expect(regionText('io.print')).toBe('print %ITEM …');
    expect(regionText('text.join')).toBe('join %ITEM …');
    expect(regionText('logic.operation')).toBe('%ITEM %OP …');
    expect(regionText('control.if')).toBe('if %COND then %DO else if … else %ELSE');
    expect(regionText('func.call')).toBe('%ARG …');
    expect(regionText('func.call_stmt')).toBe('%ARG …');
    expect(regionText('func.define')).toBe('with …');
  });

  it('is null exactly for the blocks without a mutator', () => {
    for (const block of BLOCK_DEFS) {
      expect(mutatorLabelRegion(block) !== null, block.id).toBe(block.id in TEST_MUTATOR_FOR_BLOCK);
    }
  });
});

describe('variadicSpec', () => {
  it('reads io.print', () => {
    const spec = variadicSpec(def('io.print'));
    expect(spec.groupLeading).toEqual(['print']);
    expect(spec.count).toEqual({ name: 'itemCount', min: 1, max: 32, default: 1 });
    expect(spec.plus).toBe(0);
    expect(spec.members.map((member) => [member.name, member.kind, member.leading])).toEqual([
      ['ITEM', 'value', []],
    ]);
    expect(spec.joinTexts).toEqual([]);
    expect(spec.joinField).toBeNull();
    expect(spec.gated).toEqual([]);
    expect(spec.anchorArgs).toEqual(['SEP', 'NEWLINE']);
  });

  it('reads logic.operation with its joining field', () => {
    const spec = variadicSpec(def('logic.operation'));
    expect(spec.groupLeading).toEqual([]);
    expect(spec.count).toEqual({ name: 'itemCount', min: 2, max: 32, default: 2 });
    expect(spec.joinField?.name).toBe('OP');
    expect(spec.anchorArgs).toEqual([]);
  });

  it('reads control.if', () => {
    const spec = variadicSpec(def('control.if'));
    expect(spec.groupLeading).toEqual(['if']);
    expect(spec.count).toEqual({ name: 'elseIfCount', min: 0, max: 32, default: 0 });
    expect(spec.plus).toBe(1);
    expect(spec.members.map((member) => [member.name, member.kind, member.leading])).toEqual([
      ['COND', 'value', []],
      ['DO', 'statement', ['then']],
    ]);
    expect(spec.joinTexts).toEqual(['else if']);
    expect(spec.flags).toEqual([{ name: 'hasElse', default: false }]);
    expect(spec.gated).toEqual([{ name: 'ELSE', flag: 'hasElse', leading: ['else'] }]);
  });

  it('reads the call blocks', () => {
    for (const id of ['func.call', 'func.call_stmt']) {
      const spec = variadicSpec(def(id));
      expect(spec.count).toEqual({ name: 'argCount', min: 0, max: 16, default: 0 });
      expect(spec.members.map((member) => member.name)).toEqual(['ARG']);
      expect(spec.groupLeading).toEqual([]);
    }
  });

  it('rejects blocks it cannot draw', () => {
    expect(() => variadicSpec(def('control.repeat'))).toThrow(MutatorConfigError);
    expect(() => variadicSpec(def('func.define'))).toThrow(MutatorConfigError);
    const base = def('io.print');
    const noCount: BlockDefJson = { ...base, extra: [] };
    expect(() => variadicSpec(noCount)).toThrow(/count/);
    const badCount: BlockDefJson = {
      ...base,
      extra: [{ name: 'itemCount', kind: 'count', min: null, max: 3, default: 1, types: [] }],
    };
    expect(() => variadicSpec(badCount)).toThrow(/minimum/);
    const twoFields: BlockDefJson = {
      ...def('logic.operation'),
      labelParts: [{ arg: 'ITEM' }, { arg: 'OP' }, { arg: 'OP' }, { repeat: true }],
    };
    expect(() => variadicSpec(twoFields)).toThrow(/only one field/);
    const strayAfter: BlockDefJson = {
      ...base,
      labelParts: [{ arg: 'ITEM' }, { repeat: true }, { arg: 'SEP' }, { arg: 'ITEM' }],
    };
    expect(() => variadicSpec(strayAfter)).toThrow(/flag-gated/);
    try {
      variadicSpec(noCount);
    } catch (error) {
      expect(error).toBeInstanceOf(MutatorConfigError);
      expect((error as MutatorConfigError).blockType).toBe('io.print');
    }
  });
});

describe('paramsSpec', () => {
  it('reads func.define', () => {
    const spec = paramsSpec(def('func.define'));
    expect(spec).toEqual({
      kind: 'params',
      blockType: 'func.define',
      groupLeading: ['with'],
      name: 'params',
      max: 16,
      types: ['int', 'double', 'bool', 'char', 'std::string'],
      anchorArgs: ['RETURNS', 'BODY'],
    });
  });

  it('rejects blocks without exactly one params extra', () => {
    expect(() => paramsSpec(def('io.print'))).toThrow(MutatorConfigError);
    const noTypes: BlockDefJson = {
      ...def('func.define'),
      extra: [{ name: 'params', kind: 'params', min: null, max: 16, default: null, types: [] }],
    };
    expect(() => paramsSpec(noTypes)).toThrow(/types/);
  });
});
