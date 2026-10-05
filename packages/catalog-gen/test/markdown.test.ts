import { describe, expect, it } from 'vitest';

import { readCatalogJson } from '../src/catalog-json.ts';
import { blockReference, code, text, tokenText } from '../src/emit-markdown.ts';
import { normalize } from '../src/normalize.ts';
import { fixture, fixtureText } from './fixture.ts';

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
      '## In the toolbox\n\nThe editor also adds a getter for every variable in scope at the selected block, and the blocks that change a variable, preset to it: nothing.\n',
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
