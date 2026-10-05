/** The seven Blocks2Cpp fields (src/fields/). */
import * as Blockly from 'blockly/core';
import { beforeEach, describe, expect, it } from 'vitest';

import {
  B2cCheckboxField,
  B2cDropdownField,
  B2cNumberField,
  B2cSymbolDeclField,
  B2cSymbolRefField,
  B2cTextField,
  B2cTypeField,
  DEFAULT_EDITOR_SERVICES,
  FIELD_TYPE,
  createCatalogField,
  readB2cField,
  setEditorServices,
  writeB2cField,
  type B2cFieldValue,
} from '../src';
import {
  fakeSymbols,
  headlessWorkspace,
  renderedBlock,
  renderedWorkspace,
  setUpBlocks,
  symbol,
} from './helpers';

beforeEach(() => {
  setUpBlocks();
});

/** The text-editor input Blockly shows in its widget div. */
function editorInput(): HTMLInputElement {
  const input = document.querySelector<HTMLInputElement>('.blocklyWidgetDiv input');
  if (input === null) {
    throw new Error('no editor is open');
  }
  return input;
}

/** Waits until Blockly has fired its queued events (it fires them after an animation frame). */
async function settleEvents(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 50));
}

/** Types `text` into a field's editor, then presses `key` (Enter keeps it, Escape cancels). */
function typeInto(field: Blockly.Field, text: string, key: 'Enter' | 'Escape' = 'Enter'): void {
  field.showEditor();
  const input = editorInput();
  input.value = text;
  input.dispatchEvent(new Event('input'));
  input.dispatchEvent(new KeyboardEvent('keydown', { key }));
}

/** A field of a type on a fresh rendered block (catalog type and field name). */
function renderedField<T extends Blockly.Field>(
  type: string,
  name: string,
  kind: abstract new (...args: never[]) => T,
): T {
  const block = renderedBlock(renderedWorkspace(), type);
  const field = block.getField(name);
  if (!(field instanceof kind)) {
    throw new Error(`${type}.${name} is not the expected field`);
  }
  return field;
}

/** Saves a block with Blockly's serialiser and loads it again: the copy's field value. */
function blocklyRoundTrip(block: Blockly.Block, name: string): B2cFieldValue | null {
  const state = Blockly.serialization.blocks.save(block);
  if (state === null) {
    throw new Error('nothing saved');
  }
  const copy = Blockly.serialization.blocks.append(state, block.workspace);
  const field = copy.getField(name);
  return field === null ? null : readB2cField(field);
}

const FORBIDDEN: readonly (readonly [string, string])[] = [
  ['NUL', 'a\u0000b'],
  ['control', 'a\u001bb'],
  ['carriage return', 'a\rb'],
  ['bidi', 'a\u202eb'],
  ['lone surrogate', 'a\ud800b'],
  ['too long', 'x'.repeat(65_537)],
];

describe('every field kind', () => {
  const cases: readonly { type: string; name: string; values: readonly B2cFieldValue[] }[] = [
    {
      type: 'var.declare',
      name: 'NAME',
      values: [
        { sym: 'sym_a', name: 'score' },
        { sym: 's_x', name: 'größe' },
      ],
    },
    { type: 'var.get', name: 'VAR', values: [{ ref: 'sym_a' }, { ref: 's_secret' }] },
    { type: 'var.declare', name: 'TYPE', values: ['std::string', 'double', 'long'] },
    { type: 'math.number', name: 'VALUE', values: ['007', '1e3', '0x1F', '3.14f', 'not a number'] },
    {
      type: 'text.literal',
      name: 'VALUE',
      values: ['Hello, world!', '', 'a\tb\nc', 'zero\u200bwidth', 'x'.repeat(65_536)],
    },
    { type: 'text.char', name: 'VALUE', values: ['a', '😀', 'two chars from a file'] },
    { type: 'io.print', name: 'SEP', values: ['space', 'comma', 'not_an_option'] },
    { type: 'io.print', name: 'NEWLINE', values: [true, false] },
  ];

  it.each(cases)(
    '$type.$name round-trips its value directly and through Blockly',
    ({ type, name, values }) => {
      const workspace = headlessWorkspace();
      const block = workspace.newBlock(type);
      const field = block.getField(name);
      expect(field).not.toBeNull();
      if (field === null) {
        return;
      }
      for (const value of values) {
        expect(writeB2cField(field, value), JSON.stringify(value).slice(0, 40)).toBe(true);
        expect(readB2cField(field)).toEqual(value);
        expect(blocklyRoundTrip(block, name)).toEqual(value);
      }
    },
  );

  it.each(cases)(
    '$type.$name refuses forbidden text and keeps its value',
    ({ type, name, values }) => {
      const workspace = headlessWorkspace();
      const field = workspace.newBlock(type).getField(name);
      const first = values[0];
      if (field === null || first === undefined) {
        throw new Error('missing field');
      }
      writeB2cField(field, first);
      for (const [, text] of FORBIDDEN) {
        const value: B2cFieldValue =
          typeof first === 'object' && 'sym' in first
            ? { sym: first.sym, name: text }
            : typeof first === 'object'
              ? { ref: text }
              : typeof first === 'boolean'
                ? first
                : text;
        if (typeof value === 'boolean') {
          continue;
        }
        expect(writeB2cField(field, value)).toBe(false);
        expect(readB2cField(field)).toEqual(first);
      }
    },
  );

  it('refuse values of the wrong shape', () => {
    const workspace = headlessWorkspace();
    const block = workspace.newBlock('var.declare');
    const name = block.getField('NAME');
    const type = block.getField('TYPE');
    const constant = block.getField('CONST');
    if (name === null || type === null || constant === null) {
      throw new Error('missing field');
    }
    expect(writeB2cField(name, 'score')).toBe(false);
    expect(writeB2cField(name, { sym: 'bad id!', name: 'x' })).toBe(false);
    expect(writeB2cField(name, { sym: 'sym_a', name: 'x'.repeat(65) })).toBe(false);
    expect(writeB2cField(type, true)).toBe(false);
    expect(writeB2cField(constant, 'true')).toBe(false);
    const label = new Blockly.FieldLabel('x');
    expect(readB2cField(label)).toBeNull();
    expect(writeB2cField(label, 'y')).toBe(false);
  });

  it('are registered for block definitions and mutators under their names', () => {
    const made = [
      Blockly.fieldRegistry.fromJson({
        type: FIELD_TYPE.symbolDecl,
        value: { sym: 'sym_a', name: 'n' },
      }),
      Blockly.fieldRegistry.fromJson({
        type: FIELD_TYPE.symbolRef,
        kinds: 'functions',
        value: { ref: 'sym_f' },
      }),
      Blockly.fieldRegistry.fromJson({
        type: FIELD_TYPE.type,
        types: ['int', 'std::string'],
        value: 'std::string',
      }),
      Blockly.fieldRegistry.fromJson({ type: FIELD_TYPE.number, value: '007' }),
      Blockly.fieldRegistry.fromJson({ type: FIELD_TYPE.text, value: 'q', singleChar: true }),
      Blockly.fieldRegistry.fromJson({
        type: FIELD_TYPE.dropdown,
        options: [['A', 'a']],
        value: 'a',
      }),
      Blockly.fieldRegistry.fromJson({ type: FIELD_TYPE.checkbox, value: true }),
    ];
    expect(made.map((field) => (field === null ? null : readB2cField(field)))).toEqual([
      { sym: 'sym_a', name: 'n' },
      { ref: 'sym_f' },
      'std::string',
      '007',
      'q',
      'a',
      true,
    ]);
    expect(made[1] instanceof B2cSymbolRefField && made[1].kinds).toBe('functions');
    expect(() =>
      Blockly.fieldRegistry.fromJson({ type: FIELD_TYPE.dropdown, options: [] }),
    ).toThrow(TypeError);
    expect(() => Blockly.fieldRegistry.fromJson({ type: FIELD_TYPE.type, types: 'int' })).toThrow(
      TypeError,
    );
  });

  it('start with the catalog defaults', () => {
    const workspace = headlessWorkspace();
    const print = workspace.newBlock('io.print');
    expect(readB2cField(print.getField('SEP') ?? new Blockly.FieldLabel(''))).toBe('none');
    expect(print.getFieldValue('NEWLINE')).toBe('TRUE');
    const declare = workspace.newBlock('var.declare');
    expect(declare.getFieldValue('TYPE')).toBe('int');
    expect(readB2cField(declare.getField('NAME') ?? new Blockly.FieldLabel(''))).toBeNull();
    expect(
      readB2cField(workspace.newBlock('var.get').getField('VAR') ?? new Blockly.FieldLabel('')),
    ).toBeNull();
    expect(workspace.newBlock('math.number').getFieldValue('VALUE')).toBe('0');
    expect(workspace.newBlock('text.char').getFieldValue('VALUE')).toBe('a');
  });
});

describe('typing into text fields', () => {
  it('number fields keep the exact literal text', () => {
    const field = renderedField('math.number', 'VALUE', B2cNumberField);
    for (const literal of ['007', '1e3', '0x1F', "1'000", '2.5e-3', '10u']) {
      typeInto(field, literal);
      expect(field.getValue()).toBe(literal);
    }
  });

  it('number fields refuse spaces and other characters, keeping the last good value', () => {
    const field = renderedField('math.number', 'VALUE', B2cNumberField);
    typeInto(field, '42');
    for (const bad of ['4 2', '1;', '"1"', '', '١٢']) {
      typeInto(field, bad);
      expect(field.getValue()).toBe('42');
    }
  });

  it('text.char accepts exactly one character', () => {
    const field = renderedField('text.char', 'VALUE', B2cTextField);
    typeInto(field, 'z');
    expect(field.getValue()).toBe('z');
    typeInto(field, '😀');
    expect(field.getValue()).toBe('😀');
    typeInto(field, 'ab');
    expect(field.getValue()).toBe('😀');
    typeInto(field, '');
    expect(field.getValue()).toBe('😀');
  });

  it('text fields refuse each forbidden character class and the 64 KiB cap', () => {
    const field = renderedField('text.literal', 'VALUE', B2cTextField);
    typeInto(field, 'kept');
    // An <input> drops line breaks from its value itself, so a carriage return cannot be typed.
    for (const [label, text] of FORBIDDEN.filter(([name]) => name !== 'carriage return')) {
      typeInto(field, text);
      expect(field.getValue(), label).toBe('kept');
    }
    typeInto(field, 'x'.repeat(65_536));
    expect(field.getValue()).toHaveLength(65_536);
  });

  it('Escape restores the value from before editing', () => {
    const field = renderedField('text.literal', 'VALUE', B2cTextField);
    typeInto(field, 'first');
    typeInto(field, 'second', 'Escape');
    expect(field.getValue()).toBe('first');
  });

  it('names accept ASCII letters, digits and _ only, up to 64 characters, and keep the symbol ID', () => {
    const field = renderedField('var.declare', 'NAME', B2cSymbolDeclField);
    field.setDecl({ sym: 'sym_keep', name: 'score' });
    typeInto(field, 'high_score2');
    expect(field.getDecl()).toEqual({ sym: 'sym_keep', name: 'high_score2' });
    for (const bad of ['two words', 'größe', 'a-b', '', 'x'.repeat(65), 'a\u200bb']) {
      typeInto(field, bad);
      expect(field.getDecl(), bad).toEqual({ sym: 'sym_keep', name: 'high_score2' });
    }
    typeInto(field, 'x'.repeat(64));
    expect(field.getDecl()?.name).toHaveLength(64);
  });

  it('a name typed into a block without a declaration starts a new symbol', () => {
    const field = renderedField('var.declare', 'NAME', B2cSymbolDeclField);
    expect(field.getDecl()).toBeNull();
    typeInto(field, 'count');
    expect(field.getDecl()?.name).toBe('count');
    expect(field.getSymbolId()).toMatch(/^sym_[A-Za-z0-9]{17}$/);
  });

  it('a rename is one undoable change that keeps the symbol ID', async () => {
    const workspace = renderedWorkspace();
    const block = renderedBlock(workspace, 'var.declare');
    const field = block.getField('NAME');
    if (!(field instanceof B2cSymbolDeclField)) {
      throw new Error('no name field');
    }
    field.setDecl({ sym: 'sym_keep', name: 'before' });
    await settleEvents();
    workspace.clearUndo();
    typeInto(field, 'after');
    await settleEvents();
    expect(field.getDecl()).toEqual({ sym: 'sym_keep', name: 'after' });
    workspace.undo(false);
    expect(field.getDecl()).toEqual({ sym: 'sym_keep', name: 'before' });
  });

  it('the block shows invisible characters as placeholders but stores them unchanged', () => {
    const block = renderedBlock(renderedWorkspace(), 'text.literal');
    const field = block.getField('VALUE');
    if (field === null) {
      throw new Error('no VALUE field');
    }
    field.setValue('a\u200bb\nc');
    expect(field.getValue()).toBe('a\u200bb\nc');
    expect(field.getText()).toBe('a⟨U+200B⟩b⟨U+000A⟩c');
    block.render();
    const svgText = block.getSvgRoot().textContent;
    expect(svgText).toContain('a⟨U+200B⟩b⟨U+000A⟩c');
    expect(svgText).not.toContain('\u200b');
  });

  it('the editor input has an accessible name and no autocomplete', () => {
    const field = renderedField('math.number', 'VALUE', B2cNumberField);
    field.showEditor();
    const input = editorInput();
    expect(input.getAttribute('aria-label')).toBe('Number');
    expect(input.getAttribute('autocomplete')).toBe('off');
    expect(input.getAttribute('spellcheck')).toBe('false');
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
  });
});

describe('symbol references', () => {
  it('list the symbols in scope at the block, filtered by kind', () => {
    const workspace = headlessWorkspace();
    const block = workspace.newBlock('var.set');
    const queries = fakeSymbols(
      {
        [block.id]: [
          symbol('s_count', 'count'),
          symbol('s_max', 'MAX', 'variable', { isConst: true }),
          symbol('s_p', 'p', 'parameter', { mode: 'copy' }),
          symbol('s_i', 'i', 'loopVariable'),
          symbol('s_f', 'twice', 'function'),
        ],
      },
      {},
    );
    const field = block.getField('VAR');
    if (!(field instanceof B2cSymbolRefField)) {
      throw new Error('no VAR field');
    }
    expect(field.menuOptions()).toEqual([
      ['count', 's_count'],
      ['p', 's_p'],
      ['i', 's_i'],
    ]);
    expect(queries).toEqual([block.id]);

    const get = workspace.newBlock('var.get');
    fakeSymbols(
      {
        [get.id]: [
          symbol('s_max', 'MAX', 'variable', { isConst: true }),
          symbol('s_f', 'f', 'function'),
        ],
      },
      {},
    );
    expect((get.getField('VAR') as B2cSymbolRefField).menuOptions()).toEqual([['MAX', 's_max']]);

    const call = workspace.newBlock('func.call');
    fakeSymbols({ [call.id]: [symbol('s_x', 'x'), symbol('s_f', 'f', 'function')] }, {});
    expect((call.getField('FUNC') as B2cSymbolRefField).menuOptions()).toEqual([['f', 's_f']]);
  });

  it('always show the current name, keep an out-of-scope reference selected, and mark missing ones', () => {
    const workspace = headlessWorkspace();
    const block = workspace.newBlock('var.get');
    const field = block.getField('VAR') as B2cSymbolRefField;
    fakeSymbols({ [block.id]: [symbol('s_a', 'alpha')] }, { s_gone: 'guess', s_a: 'alpha' });
    field.setRef({ ref: 's_gone' });
    expect(field.getText()).toBe('guess');
    expect(field.menuOptions()).toEqual([
      ['guess', 's_gone'],
      ['alpha', 's_a'],
    ]);
    fakeSymbols({ [block.id]: [] }, { s_gone: 'renamed' });
    expect(field.getText()).toBe('renamed');
    fakeSymbols({}, {});
    expect(field.getText()).toBe('missing (s_gone)');
    expect(field.getRef()).toEqual({ ref: 's_gone' });
  });

  it('offer a harmless entry when nothing is in scope, which selects nothing', () => {
    const workspace = headlessWorkspace();
    const field = workspace.newBlock('var.get').getField('VAR') as B2cSymbolRefField;
    expect(field.menuOptions()).toEqual([['no variables here', '']]);
    field.setValue('');
    expect(field.getRef()).toBeNull();
    expect(field.getText()).toBe('?');
    const call = workspace.newBlock('func.call').getField('FUNC') as B2cSymbolRefField;
    expect(call.menuOptions()).toEqual([['no functions yet', '']]);
  });

  it('show duplicate names apart and invisible characters as placeholders', () => {
    const workspace = headlessWorkspace();
    const block = workspace.newBlock('var.get');
    fakeSymbols(
      {
        [block.id]: [
          symbol('s_1', 'x'),
          symbol('s_2', 'x'),
          symbol('s_3', 'a\u200bb'),
          symbol('bad id!', 'skip'),
        ],
      },
      {},
    );
    expect((block.getField('VAR') as B2cSymbolRefField).menuOptions()).toEqual([
      ['x', 's_1'],
      ['x (2)', 's_2'],
      ['a⟨U+200B⟩b', 's_3'],
    ]);
  });

  it('survive a symbol provider that throws', () => {
    const workspace = headlessWorkspace();
    const field = workspace.newBlock('var.get').getField('VAR') as B2cSymbolRefField;
    field.setRef({ ref: 's_a' });
    const failing = () => {
      throw new Error('analysis not ready');
    };
    setEditorServices({
      ...DEFAULT_EDITOR_SERVICES,
      symbols: { symbolsAt: failing, nameOf: failing },
    });
    expect(field.menuOptions()).toEqual([['missing (s_a)', 's_a']]);
    expect(field.getText()).toBe('missing (s_a)');
  });

  it('do not query the scope while a block is loaded', () => {
    const workspace = headlessWorkspace();
    const queries = fakeSymbols({}, {});
    Blockly.serialization.blocks.append(
      { type: 'var.get', fields: { VAR: { ref: 's_a' } } },
      workspace,
    );
    expect(queries).toEqual([]);
  });

  it('open a menu with the current reference checked and an accessible name', () => {
    const workspace = renderedWorkspace();
    const block = renderedBlock(workspace, 'var.get');
    fakeSymbols({ [block.id]: [symbol('s_a', 'alpha'), symbol('s_b', 'beta')] }, { s_b: 'beta' });
    const field = block.getField('VAR') as B2cSymbolRefField;
    field.setRef({ ref: 's_b' });
    field.showEditor();
    const menu = document.querySelector('.blocklyDropDownDiv .blocklyMenu');
    expect(menu?.getAttribute('aria-label')).toBe('Variables in scope');
    const checked = [
      ...document.querySelectorAll('.blocklyDropDownDiv .blocklyMenuItemSelected'),
    ].map((item) => item.textContent);
    expect(checked).toEqual(['beta']);
    Blockly.DropDownDiv.hideWithoutAnimation();
  });
});

describe('type fields', () => {
  it('show std::string as string and keep the catalog value', () => {
    const workspace = headlessWorkspace();
    const field = workspace.newBlock('var.declare').getField('TYPE') as B2cTypeField;
    field.setValue('std::string');
    expect(field.getValue()).toBe('std::string');
    expect(field.getText()).toBe('string');
    expect(
      field.getOptions(false).map((option) => (Array.isArray(option) ? option[0] : option)),
    ).toEqual(['int', 'double', 'bool', 'char', 'string', 'auto']);
    expect(field.optionValues()).toContain('std::string');
  });

  it('keep and show a type that is not on the list (the catalog check reports it)', () => {
    const workspace = headlessWorkspace();
    const field = workspace.newBlock('var.declare').getField('TYPE') as B2cTypeField;
    field.setValue('long\u200b');
    expect(field.getValue()).toBe('long\u200b');
    expect(field.getText()).toBe('long⟨U+200B⟩');
  });
});

describe('dropdown fields', () => {
  it('store the option value and show its label', () => {
    const field = new B2cDropdownField(
      [
        ['keep asking until valid', 'keep_asking'],
        ['simple', 'simple'],
      ],
      'simple',
    );
    expect(field.getValue()).toBe('simple');
    expect(field.getText()).toBe('simple');
    field.setValue('keep_asking');
    expect(field.getText()).toBe('keep asking until valid');
  });

  it('show labels exactly, without splitting common words', () => {
    const field = new B2cDropdownField([
      ['repeat while', 'while'],
      ['repeat until', 'until'],
    ]);
    expect(field.prefixField).toBeNull();
    expect(field.getOptions(false)).toEqual([
      ['repeat while', 'while'],
      ['repeat until', 'until'],
    ]);
  });

  it('refuse an empty option list', () => {
    expect(() => new B2cDropdownField([])).toThrow(TypeError);
  });
});

describe('checkbox fields', () => {
  it('read and write booleans', () => {
    const field = new B2cCheckboxField(true);
    expect(field.getBoolean()).toBe(true);
    field.setBoolean(false);
    expect(readB2cField(field)).toBe(false);
    expect(field.saveState()).toBe(false);
  });
});

describe('createCatalogField', () => {
  it('creates a field of the right class for each catalog kind', () => {
    const make = (kind: Parameters<typeof createCatalogField>[0]['kind']) =>
      createCatalogField({ name: 'F', kind, options: [['A', 'a']], types: ['int'], default: null });
    expect(make('symbol_decl')).toBeInstanceOf(B2cSymbolDeclField);
    expect(make('symbol_ref')).toBeInstanceOf(B2cSymbolRefField);
    expect(make('type')).toBeInstanceOf(B2cTypeField);
    expect(make('number')).toBeInstanceOf(B2cNumberField);
    expect(make('text')).toBeInstanceOf(B2cTextField);
    expect(make('dropdown')).toBeInstanceOf(B2cDropdownField);
    expect(make('checkbox')).toBeInstanceOf(B2cCheckboxField);
  });
});
