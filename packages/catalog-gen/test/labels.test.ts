import { describe, expect, it } from 'vitest';

import { labelArgs, parseLabel } from '../src/labels.ts';

describe('parseLabel', () => {
  it.each([
    ['when program starts', [{ text: 'when program starts' }]],
    [
      'print %ITEM … %SEP %NEWLINE',
      [{ text: 'print' }, { arg: 'ITEM' }, { repeat: true }, { arg: 'SEP' }, { arg: 'NEWLINE' }],
    ],
    [
      'if %COND then %DO else if … else %ELSE',
      [
        { text: 'if' },
        { arg: 'COND' },
        { text: 'then' },
        { arg: 'DO' },
        { text: 'else if' },
        { repeat: true },
        { text: 'else' },
        { arg: 'ELSE' },
      ],
    ],
    ['"%VALUE"', [{ text: '"' }, { arg: 'VALUE' }, { text: '"' }]],
    ["letter '%VALUE'", [{ text: "letter '" }, { arg: 'VALUE' }, { text: "'" }]],
    [
      'ask %PROMPT and save answer in %VAR (%MODE)',
      [
        { text: 'ask' },
        { arg: 'PROMPT' },
        { text: 'and save answer in' },
        { arg: 'VAR' },
        { text: '(' },
        { arg: 'MODE' },
        { text: ')' },
      ],
    ],
    ['%VAR %OP %VALUE', [{ arg: 'VAR' }, { arg: 'OP' }, { arg: 'VALUE' }]],
    ['%A%B_2', [{ arg: 'A' }, { arg: 'B_2' }]],
    ['%ITEM0 …', [{ arg: 'ITEM0' }, { repeat: true }]],
  ])('parses %j', (label, parts) => {
    expect(parseLabel(label)).toEqual(parts);
  });

  it('keeps a % that does not start a name as text', () => {
    expect(parseLabel('100% of %x and %_Y %')).toEqual([{ text: '100% of %x and %_Y %' }]);
  });

  it('treats only a bare … as a repeat marker', () => {
    expect(parseLabel('wait… and …more')).toEqual([{ text: 'wait… and …more' }]);
    expect(parseLabel('…')).toEqual([{ repeat: true }]);
    expect(parseLabel('… …')).toEqual([{ repeat: true }, { repeat: true }]);
    expect(parseLabel('a\t…\nb')).toEqual([{ text: 'a' }, { repeat: true }, { text: 'b' }]);
  });

  it('trims text and collapses whitespace', () => {
    expect(parseLabel('  a   b  %X  ')).toEqual([{ text: 'a b' }, { arg: 'X' }]);
    expect(parseLabel('')).toEqual([]);
    expect(parseLabel('   ')).toEqual([]);
  });

  it('lists the argument names in order', () => {
    expect(labelArgs(parseLabel('%B then %A … %B'))).toEqual(['B', 'A', 'B']);
    expect(labelArgs(parseLabel('no arguments'))).toEqual([]);
  });
});
