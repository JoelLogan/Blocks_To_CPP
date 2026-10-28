/**
 * The type picker (docs/plans/m3-contracts/w4-editor-tools.md §2.2): what it offers, what it reads
 * from typed text, its keys and ARIA, and the type field that opens it.
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  B2cTypeField,
  DEFAULT_EDITOR_SERVICES,
  MAX_PICKER_DEPTH,
  NO_FACTS,
  addArgument,
  argumentEntries,
  createTypePicker,
  parseTypeText,
  pathLabel,
  rootEntries,
  setBlockLabelMode,
  setEditorServices,
  termKey,
  type PickerContext,
  type PickerEntry,
  type TypeFactsTs,
  type TypeTerm,
} from '../src';
import { expectNoAxeViolations } from './axe';
import { renderedBlock, renderedWorkspace, setUpBlocks, symbol } from './helpers';

beforeEach(() => {
  setUpBlocks();
});

afterEach(() => {
  setBlockLabelMode('friendly');
});

const STUDENT = { id: 'sym_student', name: 'Student', kind: 'struct' as const };
const LEVEL = { id: 'sym_level', name: 'Level', kind: 'enum' as const };

/** Facts in which Student is not comparable and Level is. */
const FACTS: TypeFactsTs = {
  userType: (id) =>
    id === STUDENT.id
      ? { name: 'Student', kind: 'struct', comparable: false, printable: false }
      : id === LEVEL.id
        ? { name: 'Level', kind: 'enum', comparable: true, printable: true }
        : null,
};

function context(overrides: Partial<PickerContext> = {}): PickerContext {
  return {
    typeClass: 'value',
    fixed: ['int', 'double', 'bool', 'char', 'std::string', 'auto'],
    userTypes: [STUDENT, LEVEL],
    facts: FACTS,
    mode: 'friendly',
    ...overrides,
  };
}

const labels = (entries: readonly PickerEntry[]) => entries.map((entry) => entry.label);
const int: TypeTerm = { kind: 'name', name: 'int' };
const text: TypeTerm = { kind: 'name', name: 'std::string' };

describe('the picker model', () => {
  it('offers the field’s types, the module’s types and the collections of its class', () => {
    const entries = rootEntries(context());
    expect(labels(entries)).toEqual([
      'int',
      'double',
      'bool',
      'char',
      'string',
      'auto',
      'Level',
      'Student',
      'list of …',
      'deque of …',
      'set of …',
      'map from … to …',
      'queue of …',
      'stack of …',
      'priority queue of …',
    ]);
    expect(entries.map((entry) => entry.group)).toContain('mine');
  });

  it('offers only its own types without a class, and only structs for a struct class', () => {
    expect(labels(rootEntries(context({ typeClass: null })))).toHaveLength(6);
    expect(labels(rootEntries(context({ typeClass: 'struct', fixed: [] })))).toEqual(['Student']);
    expect(
      labels(rootEntries(context({ typeClass: 'exception', fixed: ['std::runtime_error'] }))),
    ).toEqual(['runtime error']);
  });

  it('names types and collections in C++ in the C++ mode', () => {
    const entries = rootEntries(context({ mode: 'cpp' }));
    expect(labels(entries)).toContain('std::string');
    expect(labels(entries)).toContain('std::map<…, …>');
  });

  it('offers only comparable keys and set elements, and stops nesting collections deep down', () => {
    const keys = labels(argumentEntries(context(), 'std::map', 0, 1));
    expect(keys).toContain('Level');
    expect(keys).not.toContain('Student');
    expect(labels(argumentEntries(context(), 'std::map', 1, 1))).toContain('Student');
    expect(labels(argumentEntries(context(), 'std::set', 0, 1))).not.toContain('Student');
    const deep = argumentEntries(context(), 'std::vector', 0, MAX_PICKER_DEPTH);
    expect(deep.some((entry) => entry.kind === 'collection')).toBe(false);
  });

  it('builds a collection argument by argument', () => {
    let step = addArgument([{ name: 'std::map', arity: 2, args: [] }], text);
    expect(step.done).toBeNull();
    expect(pathLabel(step.path, { facts: NO_FACTS, mode: 'friendly' })).toBe(
      'map from string to …',
    );
    step = addArgument([...step.path, { name: 'std::vector', arity: 1, args: [] }], int);
    expect(step.done).not.toBeNull();
    expect(termKey(step.done ?? int)).toBe('{"std::map":["std::string",{"std::vector":"int"}]}');
  });
});

describe('typed types', () => {
  const users = [STUDENT, LEVEL];
  const key = (typed: string) => {
    const term = parseTypeText(typed, users);
    return term === null ? null : termKey(term);
  };

  it('reads C++ and Friendly text', () => {
    expect(key('int')).toBe('int');
    expect(key('string')).toBe('std::string');
    expect(key('std::string')).toBe('std::string');
    expect(key('vector<int>')).toBe('{"std::vector":"int"}');
    expect(key('std::map<std::string, std::vector<int>>')).toBe(
      '{"std::map":["std::string",{"std::vector":"int"}]}',
    );
    expect(key('list of int')).toBe('{"std::vector":"int"}');
    expect(key('map from string to list of Student')).toBe(
      '{"std::map":["std::string",{"std::vector":{"ref":"sym_student"}}]}',
    );
    expect(key('priority queue of double')).toBe('{"std::priority_queue":"double"}');
    expect(key('map from (map from int to int) to int')).toBe(
      '{"std::map":[{"std::map":["int","int"]},"int"]}',
    );
    expect(key('List Of Int')).toBe('{"std::vector":"int"}');
  });

  it('gives nothing for unknown names, wrong arities, other punctuation or too much', () => {
    for (const typed of [
      '',
      'float',
      'student',
      'vector<int, int>',
      'map<int>',
      'list of',
      'int x',
      'vector<int',
      'int; system("x")',
      '__proto__',
      'list of '.repeat(40) + 'int',
    ]) {
      expect(key(typed), typed).toBeNull();
    }
  });
});

describe('the popover', () => {
  function open(overrides: Partial<PickerContext> = {}, current: string | null = 'int') {
    const onChoose = vi.fn<(key: string) => void>();
    const onCancel = vi.fn<() => void>();
    const picker = createTypePicker({
      context: context(overrides),
      current,
      label: 'Type',
      onChoose,
      onCancel,
    });
    document.body.append(picker.element);
    picker.focus();
    const input = picker.element.querySelector('input');
    if (input === null) {
      throw new Error('no search box');
    }
    const key = (name: string) => {
      input.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true }));
    };
    const active = () =>
      picker.element.querySelector(`#${input.getAttribute('aria-activedescendant') ?? 'none'}`)
        ?.textContent;
    return { picker, input, key, active, onChoose, onCancel };
  }

  it('is a labelled combobox over a grouped listbox, with the current type active', async () => {
    const { picker, input, active } = open();
    expect(input.getAttribute('role')).toBe('combobox');
    expect(input.getAttribute('aria-controls')).toBe(
      picker.element.querySelector('[role="listbox"]')?.id,
    );
    expect(document.activeElement).toBe(input);
    expect(active()).toBe('int');
    expect(picker.element.querySelectorAll('[role="group"]')).toHaveLength(3);
    expect(picker.element.querySelector('[role="status"]')?.textContent).toBe('15 types');
    await expectNoAxeViolations(picker.element);
  });

  it('chooses map from string to list of int with the keyboard alone', () => {
    const { picker, key, onChoose, active } = open();
    picker.search('map');
    expect(active()).toBe('map from … to … ▸');
    key('Enter');
    expect(picker.element.textContent).toContain('Building map from … to …');
    picker.search('string');
    key('Enter');
    picker.search('list');
    key('ArrowRight');
    picker.search('int');
    key('Enter');
    expect(onChoose).toHaveBeenCalledWith('{"std::map":["std::string",{"std::vector":"int"}]}');
  });

  it('offers a typed type first, and goes back a step with Backspace', () => {
    const { picker, key, onChoose, active } = open();
    picker.search('vector<double>');
    expect(active()).toBe('list of double');
    key('Enter');
    expect(onChoose).toHaveBeenCalledWith('{"std::vector":"double"}');

    const second = open();
    second.picker.search('set');
    second.key('Enter');
    expect(second.picker.element.textContent).toContain('Building set of …');
    second.picker.search('');
    second.key('Backspace');
    expect(second.picker.element.textContent).not.toContain('Building');
    expect(second.onChoose).not.toHaveBeenCalled();
  });

  it('moves with the arrows and closes without a change on Escape', () => {
    const { key, active, onCancel, onChoose } = open();
    key('ArrowDown');
    expect(active()).toBe('double');
    key('ArrowUp');
    key('ArrowUp');
    expect(active()).toBe('int');
    key('End');
    expect(active()).toBe('priority queue of … ▸');
    key('Escape');
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onChoose).not.toHaveBeenCalled();
  });

  it('says when nothing matches, and shows text as text', () => {
    const { picker } = open({
      userTypes: [{ id: 'sym_x', name: '<b>x</b>', kind: 'struct' }],
    });
    expect(picker.element.querySelector('b')).toBeNull();
    picker.search('zzz');
    expect(picker.element.textContent).toContain('No type matches.');
    expect(picker.element.querySelector('[role="status"]')?.textContent).toBe('0 types');
  });
});

describe('the type field', () => {
  it('opens the picker in the drop-down and sets the chosen type', () => {
    setEditorServices({
      ...DEFAULT_EDITOR_SERVICES,
      symbols: {
        symbolsAt: () => [],
        nameOf: (id) => (id === STUDENT.id ? 'Student' : null),
        userTypes: () => [symbol(STUDENT.id, 'Student', 'struct')],
      },
    });
    const workspace = renderedWorkspace();
    const block = renderedBlock(workspace, 'var.declare');
    const field = block.getField('TYPE');
    expect(field).toBeInstanceOf(B2cTypeField);
    if (!(field instanceof B2cTypeField)) {
      return;
    }
    field.showEditor();
    expect(Blockly.DropDownDiv.isVisible()).toBe(true);
    const picker = field.picker;
    expect(picker).not.toBeNull();
    expect(labels(picker?.entries() ?? [])).toContain('Student');
    picker?.search('list of Student');
    const input = Blockly.DropDownDiv.getContentDiv().querySelector('input');
    input?.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(field.getValue()).toBe('{"std::vector":{"ref":"sym_student"}}');
    expect(field.getText()).toBe('list of Student');
    expect(Blockly.DropDownDiv.isVisible()).toBe(false);
  });
});
