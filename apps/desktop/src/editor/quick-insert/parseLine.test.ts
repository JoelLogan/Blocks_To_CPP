/** The one-line statements Quick Insert reads (docs/plans/m3-contracts/w4-editor-tools.md §2.4). */
import { describe, expect, it } from 'vitest';

import { symbolFixture } from '../toolbox/testing';
import { lineNames, parseLine, valueTokens } from './parseLine';

const names = lineNames([
  symbolFixture('s_x', 'x'),
  symbolFixture('s_y', 'y'),
  symbolFixture('s_ok', 'running', { type: 'bool' }),
  symbolFixture('s_f', 'area', { kind: 'function', params: [], returns: 'int' }),
]);

/** A line's block: its type, fields, declarations, mutator state and each input's tokens. */
function read(line: string) {
  const item = parseLine(line, names)?.item;
  if (item === undefined) {
    return null;
  }
  const inputs: Record<string, unknown> = {};
  for (const [name, input] of Object.entries(item.inputs ?? {})) {
    const state = (input as { shadow?: { extraState?: { tokens?: unknown } } }).shadow;
    inputs[name] = state?.extraState?.tokens;
  }
  return {
    type: item.type,
    fields: item.fields,
    declares: item.declares,
    inputs,
  };
}

describe('values', () => {
  it('become slot tokens: numbers, text, characters, true and false, variables, operators', () => {
    expect(valueTokens('y + 1', names)).toEqual([{ ref: 's_y' }, { op: '+' }, { num: '1' }]);
    expect(valueTokens('"hi" ', names)).toEqual([{ str: 'hi' }]);
    expect(valueTokens("'a'", names)).toEqual([{ chr: 'a' }]);
    expect(valueTokens('(x <= 2.5) && true', names)).toEqual([
      { op: '(' },
      { ref: 's_x' },
      { op: '<=' },
      { num: '2.5' },
      { op: ')' },
      { op: '&&' },
      { kw: 'true' },
    ]);
  });

  it('refuse unknown names, functions, assignments, escapes and stray characters', () => {
    for (const text of ['z', 'area', 'x = 1', '"a\\n"', '"open', 'x; y', 'x # y', '']) {
      expect(valueTokens(text, names), text).toBeNull();
    }
  });
});

describe('lines', () => {
  it('declare a variable', () => {
    expect(read('int total = 5')).toMatchObject({
      type: 'var.declare',
      fields: { TYPE: 'int', CONST: false },
      declares: { NAME: 'total' },
      inputs: { VALUE: [{ num: '5' }] },
    });
    expect(read('string name;')).toMatchObject({
      fields: { TYPE: 'std::string' },
      declares: { NAME: 'name' },
    });
    expect(read('int 9lives = 1')).toMatchObject({ declares: { NAME: '9lives' } });
    expect(read('int total = z')).toBeNull();
  });

  it('set and change a variable', () => {
    expect(read('x = y + 1')).toMatchObject({
      type: 'var.set',
      fields: { VAR: { ref: 's_x' } },
      inputs: { VALUE: [{ ref: 's_y' }, { op: '+' }, { num: '1' }] },
    });
    expect(read('x += 2')).toMatchObject({
      type: 'var.change',
      fields: { VAR: { ref: 's_x' }, OP: 'add' },
      inputs: { BY: [{ num: '2' }] },
    });
    expect(read('x %= 3')).toMatchObject({ fields: { OP: 'mod' } });
    expect(read('x++')).toMatchObject({ fields: { OP: 'add' }, inputs: { BY: [{ num: '1' }] } });
    expect(read('--x')).toMatchObject({ fields: { OP: 'sub' } });
    expect(read('z = 1')).toBeNull();
    expect(read('x == 1')).toBeNull();
  });

  it('print, the Friendly way and with cout', () => {
    expect(read('print "hi", x')).toMatchObject({
      type: 'io.print',
      fields: { NEWLINE: true },
      inputs: { ITEM0: [{ str: 'hi' }], ITEM1: [{ ref: 's_x' }] },
    });
    expect(read('cout << "a, b" << x << endl')).toMatchObject({
      type: 'io.print',
      fields: { NEWLINE: true },
      inputs: { ITEM0: [{ str: 'a, b' }], ITEM1: [{ ref: 's_x' }] },
    });
    expect(read('std::cout << x')).toMatchObject({ fields: { NEWLINE: false } });
  });

  it('read loops, conditions, return, break and continue', () => {
    expect(read('repeat 10')).toMatchObject({
      type: 'control.repeat',
      inputs: { TIMES: [{ num: '10' }] },
    });
    expect(read('repeat 3 times')).toMatchObject({ inputs: { TIMES: [{ num: '3' }] } });
    expect(read('while running')).toMatchObject({
      type: 'control.while',
      inputs: { COND: [{ ref: 's_ok' }] },
    });
    expect(read('if (x > 3)')).toMatchObject({
      type: 'control.if',
      inputs: { COND0: [{ ref: 's_x' }, { op: '>' }, { num: '3' }] },
    });
    // The outer brackets go only when they are one pair around the whole condition.
    expect(read('if (x > 1) && (y < 2)')).toMatchObject({
      type: 'control.if',
      inputs: {
        COND0: [
          { op: '(' },
          { ref: 's_x' },
          { op: '>' },
          { num: '1' },
          { op: ')' },
          { op: '&&' },
          { op: '(' },
          { ref: 's_y' },
          { op: '<' },
          { num: '2' },
          { op: ')' },
        ],
      },
    });
    expect(read('while (x) || (y)')).toMatchObject({
      type: 'control.while',
      inputs: {
        COND: [
          { op: '(' },
          { ref: 's_x' },
          { op: ')' },
          { op: '||' },
          { op: '(' },
          { ref: 's_y' },
          { op: ')' },
        ],
      },
    });
    expect(read('if ((x > 1))')).toMatchObject({
      inputs: { COND0: [{ op: '(' }, { ref: 's_x' }, { op: '>' }, { num: '1' }, { op: ')' }] },
    });
    // Unbalanced brackets are no value: the line falls back to the search.
    expect(parseLine('if x > 1)', names)).toBeNull();
    expect(valueTokens('(x + 1', names)).toBeNull();
    expect(read('return x')).toMatchObject({ type: 'func.return' });
    expect(read('return')).toMatchObject({ type: 'func.return' });
    expect(read('break;')).toMatchObject({ type: 'control.break' });
    expect(read('continue')).toMatchObject({ type: 'control.continue' });
  });

  it('give nothing for anything else, and for overlong lines', () => {
    for (const line of ['', 'print', 'hello world', 'for i = 0 to 10', 'x = ', 'x'.repeat(300)]) {
      expect(read(line), line).toBeNull();
    }
  });
});
