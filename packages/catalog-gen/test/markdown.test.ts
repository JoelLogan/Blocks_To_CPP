import { describe, expect, it } from 'vitest';

import { readCatalogJson } from '../src/catalog-json.ts';
import { blockReference, code, text, tokenText } from '../src/emit-markdown.ts';
import { normalize } from '../src/normalize.ts';
import { fixture, fixtureText, rustBlock } from './fixture.ts';

describe('blockReference', () => {
  it('writes an index and one page per toolbox category', () => {
    const files = blockReference(normalize(readCatalogJson(fixtureText())));
    expect([...files.keys()]).toEqual([
      'README.md',
      'text.md',
      'control.md',
      'loops.md',
      'functions.md',
    ]);
    // The whole reference, as a reviewable snapshot.
    expect(Object.fromEntries(files)).toMatchSnapshot();
  });

  it('refuses a toolbox that leaves blocks out', () => {
    const catalog = fixture();
    catalog.toolbox.category.pop();
    const model = normalize(readCatalogJson(JSON.stringify(catalog)));
    expect(() => blockReference(model)).toThrow('the toolbox categories cover 3 of 5 blocks');
  });

  it('writes no page for a category without blocks (w2.md §2.1)', () => {
    const catalog = fixture();
    catalog.toolbox.category.push({
      id: 'errors',
      name: 'Errors',
      icon: '⚠',
      colour: 'errors',
      dynamic: null,
      entry: [],
    });
    const files = blockReference(normalize(readCatalogJson(JSON.stringify(catalog))));
    expect(files.has('errors.md')).toBe(false);
    expect(files.get('README.md')).not.toContain('Errors');
  });

  it('describes repeated fields, row extras and header fields', () => {
    const catalog = fixture();
    catalog.blocks.push(
      rustBlock('errors.try', {
        category: 'errors',
        label: { friendly: 'try %TRY catch %ERROR as %NAME %CATCH catch …', cpp: 'try' },
        field: [
          {
            name: 'ERROR',
            kind: 'type',
            options: [],
            types: ['std::exception'],
            default: 'std::exception',
            repeat: { count: 'catchCount', plus: 0 },
          },
          {
            name: 'NAME',
            kind: 'symbol_decl',
            options: [],
            types: [],
            default: null,
            repeat: { count: 'catchCount', plus: 0 },
          },
          { name: 'HEADER', kind: 'header', options: [], types: [], default: '<cmath>' },
          { name: 'TYPE', kind: 'type', options: [], types: [], default: null },
        ],
        statement: [{ name: 'TRY', repeat: null, when: null }],
        extra: [
          { name: 'catchCount', kind: 'count', min: 1, max: 8, default: '1', types: [] },
          { name: 'fields', kind: 'fields', min: 0, max: 64, default: null, types: ['int'] },
          { name: 'values', kind: 'values', min: 0, max: 64, default: null, types: [] },
        ],
      }),
    );
    catalog.toolbox.category.push({
      id: 'errors',
      name: 'Errors',
      icon: '⚠',
      colour: 'errors',
      dynamic: null,
      entry: [{ block: 'errors.try', label: null, preset: null }],
    });
    const page =
      blockReference(normalize(readCatalogJson(JSON.stringify(catalog)))).get('errors.md') ?? '';
    expect(page).toContain(
      '`ERROR0`, `ERROR1`, …: a type, `std::exception`, default `std::exception`, one for each of `catchCount`.',
    );
    expect(page).toContain('`NAME0`, `NAME1`, …: the name of the new variable, one for each of');
    expect(page).toContain('`HEADER`: a standard header, default `<cmath>`.');
    expect(page).toContain('`TYPE`: a type the module defines, required.');
    expect(page).toContain('Setting `fields`: up to 64 fields, each `int`.');
    expect(page).toContain('Setting `values`: up to 64 values, each with its name.');
  });

  it('describes a dynamic Variables category', () => {
    const catalog = fixture();
    catalog.toolbox.category.push({
      id: 'variables',
      name: 'Variables',
      icon: 'x',
      colour: 'variables',
      dynamic: 'variables',
      entry: [],
    });
    const files = blockReference(normalize(readCatalogJson(JSON.stringify(catalog))));
    const page = files.get('variables.md') ?? '';
    expect(page).toContain('| Block | Friendly label |');
    expect(page).toContain(
      '## In the toolbox\n\nThe editor also adds, each with a variable already chosen, a getter for each variable and the blocks that change one ([spec §4.2](../../spec/04-user-interface.md#42-toolbox)): nothing.\n',
    );
  });
});

describe('escaping', () => {
  it('escapes Markdown and HTML in plain text', () => {
    expect(text('a <b> | *c* _d_ [e](f) `g` ~h~ &amp; \\')).toBe(
      'a \\<b\\> \\| \\*c\\* \\_d\\_ \\[e\\](f) \\`g\\` \\~h\\~ \\&amp; \\\\',
    );
    expect(text('one\ntwo\r\n  three')).toBe('one two three');
  });

  it('fences code with more backticks than it holds and escapes pipes', () => {
    expect(code('a || b')).toBe('`a \\|\\| b`');
    expect(code('x `y` z')).toBe('``x `y` z``');
    expect(code('`edge')).toBe('`` `edge ``');
    expect(code('a ``` b')).toBe('````a ``` b````');
    expect(code(' padded \n ')).toBe('`padded`');
    expect(code('   ')).toBe('nothing');
    // A backslash directly before `|` has no code-span form that GitHub and marked read alike,
    // so it becomes escaped plain text (both render it as the original text).
    expect(code('a\\|b')).toBe('a\\\\\\|b');
    expect(code('a\\\\|b')).toBe('a\\\\\\\\\\|b');
    expect(code('<x> \\| `y`')).toBe('\\<x\\> \\\\\\| \\`y\\`');
    // A backslash elsewhere stays in the code span.
    expect(code('|\\')).toBe('`\\|\\`');
    expect(code('x\\')).toBe('`x\\`');
  });

  it('writes tokens the way the editor shows them', () => {
    expect(tokenText({ num: '1.5' })).toBe('1.5');
    expect(tokenText({ str: 'say "hi"\n' })).toBe('"say \\"hi\\"\\n"');
    expect(tokenText({ chr: 'a' })).toBe("'a'");
    expect(tokenText({ chr: "'" })).toBe("'\\''");
    expect(tokenText({ chr: '\\' })).toBe("'\\\\'");
    expect(tokenText({ ref: 'sym_x' })).toBe('sym_x');
    expect(tokenText({ op: '*' })).toBe('*');
    expect(tokenText({ kw: 'true' })).toBe('true');
    expect(tokenText({ text: '1 +' })).toBe('1 +');
  });
});
