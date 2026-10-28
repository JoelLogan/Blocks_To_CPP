/**
 * Structs and enums on the canvas (docs/plans/m3-contracts/w2.md §2.3, slots.md §9's W2 tests):
 * the members menu, `chosenMember`, the type fields' user types, *enum value*'s `ENUM`, *new …
 * with* labelled from its struct, and *switch on* an enum taking only that enum's values as its
 * cases (package D, w2.md §2.9).
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { registerB2cBlocks } from '../blocks/register';
import { B2cSymbolRefField, NO_OWNER_LABELS } from '../fields/symbol-ref';
import { B2cTypeField, NO_TYPE_LABEL } from '../fields/type';
import { installIdGenerator } from '../ids';
import { isB2cMutatorBlock, refreshMutatorLabels, registerB2cMutators } from '../mutators/register';
import {
  resetEditorServices,
  setEditorServices,
  type SymbolInfo,
  type SymbolProvider,
  type TypeOracle,
} from '../services';
import { refreshSymbolNames } from '../shadows/state';
import { parseType, type TypeFactsTs, type TypeJson, type UserTypeTs } from '../types/static-type';
import { B2C_CHECKER_NAME } from './checker';
import { blockTyping } from './slots';

let workspace: Blockly.WorkspaceSvg;
let symbols: Map<string, SymbolInfo>;
let names: Map<string, string>;

function sym(
  id: string,
  kind: SymbolInfo['kind'],
  type: TypeJson,
  extra: Partial<SymbolInfo> = {},
): SymbolInfo {
  return { id, name: id, kind, type, module: 'mod_main', declBlock: 'blk_decl', ...extra };
}

const POINT = { ref: 's_Point' };
const COLOUR = { ref: 's_Colour' };

function program(): SymbolInfo[] {
  return [
    sym('s_Point', 'struct', POINT, { name: 'Point', fields: ['f_x', 'f_y'] }),
    sym('f_x', 'field', 'int', { name: 'x', owner: 's_Point' }),
    sym('f_y', 'field', 'double', { name: 'y', owner: 's_Point' }),
    sym('s_Team', 'struct', { ref: 's_Team' }, { name: 'Team', fields: ['f_title'] }),
    sym('f_title', 'field', 'std::string', { name: 'title', owner: 's_Team' }),
    sym('s_Colour', 'enum', COLOUR, { name: 'Colour', values: ['e_red', 'e_green'] }),
    sym('e_red', 'enumerator', COLOUR, { name: 'Red', owner: 's_Colour' }),
    sym('e_green', 'enumerator', COLOUR, { name: 'Green', owner: 's_Colour' }),
    sym('s_Size', 'enum', { ref: 's_Size' }, { name: 'Size', values: ['e_small'] }),
    sym('e_small', 'enumerator', { ref: 's_Size' }, { name: 'Small', owner: 's_Size' }),
    sym('v_p', 'variable', POINT, { name: 'p' }),
    sym('v_c', 'variable', COLOUR, { name: 'c' }),
  ];
}

function facts(): TypeFactsTs {
  return {
    userType(id: string): UserTypeTs | null {
      const symbol = symbols.get(id);
      if (symbol?.kind !== 'struct' && symbol?.kind !== 'enum') {
        return null;
      }
      return { name: symbol.name, kind: symbol.kind, comparable: false, printable: false };
    },
  };
}

function install(extra: Partial<SymbolProvider> = {}): void {
  setEditorServices({
    symbols: {
      symbolsAt: () => [...symbols.values()],
      nameOf: (id) => names.get(id) ?? symbols.get(id)?.name ?? null,
      userTypes: () => [...symbols.values()],
      ...extra,
    },
    types: oracle(),
  });
}

/** The analysis's types: a getter's from its variable, the symbols, every one visible. */
function oracle(): TypeOracle {
  return {
    outputTypeOf: (target) => {
      const field = target.type === 'var.get' ? target.getField('VAR') : null;
      const id = field instanceof B2cSymbolRefField ? field.getRef()?.ref : undefined;
      const symbol = id === undefined ? undefined : symbols.get(id);
      return symbol === undefined ? null : parseType(symbol.type);
    },
    symbol: (id) => symbols.get(id) ?? null,
    visibleAt: (id) => symbols.has(id),
    facts,
  };
}

beforeAll(() => {
  installIdGenerator();
  registerB2cMutators();
  registerB2cBlocks();
});

beforeEach(() => {
  const host = document.createElement('div');
  document.body.append(host);
  workspace = Blockly.inject(host, {
    renderer: 'zelos',
    sounds: false,
    plugins: { connectionChecker: B2C_CHECKER_NAME },
  });
  symbols = new Map(program().map((symbol) => [symbol.id, symbol]));
  names = new Map();
  install();
});

afterEach(() => {
  resetEditorServices();
  workspace.dispose();
  document.body.replaceChildren();
});

function block(type: string): Blockly.BlockSvg {
  const created = workspace.newBlock(type);
  created.initSvg();
  created.render();
  return created;
}

function ref(target: Blockly.Block, field: string): B2cSymbolRefField {
  const found = target.getField(field);
  if (!(found instanceof B2cSymbolRefField)) {
    throw new Error(`${field} is not a reference field`);
  }
  return found;
}

function typeField(target: Blockly.Block, field = 'TYPE'): B2cTypeField {
  const found = target.getField(field);
  if (!(found instanceof B2cTypeField)) {
    throw new Error(`${field} is not a type field`);
  }
  return found;
}

/** The menu's labels and values. */
function menu(field: Blockly.FieldDropdown): [string, string][] {
  return field.getOptions(false).map((option) => {
    const [label, value] = option as [string, string];
    return [label, value];
  });
}

function plug(parent: Blockly.Block, name: string, child: Blockly.Block): void {
  const to = parent.getInput(name)?.connection;
  if (to === null || to === undefined || child.outputConnection === null) {
    throw new Error(`no connection for ${name}`);
  }
  to.connect(child.outputConnection);
}

function canDrop(child: Blockly.Block, parent: Blockly.Block, name: string): boolean {
  const to = parent.getInput(name)?.connection;
  const from = child.outputConnection;
  if (to === null || to === undefined || from === null) {
    throw new Error(`no connection for ${name}`);
  }
  return workspace.connectionChecker.canConnect(from, to, true, 1e9);
}

describe('the members menu', () => {
  it('lists nothing while unbound, then the bound enum’s values in row order', () => {
    const value = block('type.enum_value');
    expect(menu(ref(value, 'VALUE'))).toEqual([[NO_OWNER_LABELS.enum, '']]);
    ref(value, 'ENUM').setRef({ ref: 's_Colour' });
    expect(menu(ref(value, 'VALUE'))).toEqual([
      ['Red', 'e_red'],
      ['Green', 'e_green'],
    ]);
  });

  it('keeps a chosen member that stops fitting, first in the menu', () => {
    const value = block('type.enum_value');
    ref(value, 'ENUM').setRef({ ref: 's_Colour' });
    ref(value, 'VALUE').setRef({ ref: 'e_green' });
    ref(value, 'ENUM').setRef({ ref: 's_Size' });
    expect(ref(value, 'VALUE').getRef()).toEqual({ ref: 'e_green' });
    expect(menu(ref(value, 'VALUE'))).toEqual([
      ['Green', 'e_green'],
      ['Small', 'e_small'],
    ]);
  });

  it('lists the fields of the struct a variable holds, or of what is plugged in', () => {
    const set = block('type.set_field');
    expect(menu(ref(set, 'MEMBER'))).toEqual([[NO_OWNER_LABELS.struct, '']]);
    ref(set, 'VAR').setRef({ ref: 'v_p' });
    expect(menu(ref(set, 'MEMBER'))).toEqual([
      ['x', 'f_x'],
      ['y', 'f_y'],
    ]);
    const field = block('type.field');
    const get = block('var.get');
    ref(get, 'VAR').setRef({ ref: 'v_p' });
    plug(field, 'OBJ', get);
    expect(menu(ref(field, 'MEMBER'))).toEqual([
      ['x', 'f_x'],
      ['y', 'f_y'],
    ]);
  });
});

describe('chosenMember', () => {
  it('types a field of the bound struct, and refuses one of another struct', () => {
    const set = block('type.set_field');
    ref(set, 'VAR').setRef({ ref: 'v_p' });
    ref(set, 'MEMBER').setRef({ ref: 'f_x' });
    expect(blockTyping(set, oracle())?.fields.get('MEMBER')).toBe('same');
    expect(canDrop(block('text.literal'), set, 'VALUE')).toBe(false);
    // Title is a field of Team, not of Point: nothing is known about the value it takes.
    ref(set, 'MEMBER').setRef({ ref: 'f_title' });
    expect(blockTyping(set, oracle())?.fields.get('MEMBER')).toBe('unknown');
    expect(canDrop(block('text.literal'), set, 'VALUE')).toBe(true);
  });
});

describe('type fields with the module’s types', () => {
  it('list structs and enums by name, then a list of each, where the class takes a value', () => {
    const create = block('var.declare');
    const options = menu(typeField(create));
    const users = options.filter(([, value]) => value.includes('"ref"'));
    expect(users).toEqual([
      ['Colour', '{"ref":"s_Colour"}'],
      ['Point', '{"ref":"s_Point"}'],
      ['Size', '{"ref":"s_Size"}'],
      ['Team', '{"ref":"s_Team"}'],
      ['list of Colour', '{"std::vector":{"ref":"s_Colour"}}'],
      ['list of Point', '{"std::vector":{"ref":"s_Point"}}'],
      ['list of Size', '{"std::vector":{"ref":"s_Size"}}'],
      ['list of Team', '{"std::vector":{"ref":"s_Team"}}'],
    ]);
    expect(options[0]).toEqual(['int', 'int']);
  });

  it('list only structs, and no lists, in new … with', () => {
    const made = block('type.new');
    expect(menu(typeField(made))).toEqual([
      ['Point', '{"ref":"s_Point"}'],
      ['Team', '{"ref":"s_Team"}'],
    ]);
    symbols.clear();
    expect(menu(typeField(made))).toEqual([[NO_TYPE_LABEL, '']]);
  });

  it('list user types in a struct’s field rows and a function’s parameter rows', () => {
    const struct = block('type.struct');
    if (!isB2cMutatorBlock(struct)) {
      throw new Error('no mutator');
    }
    struct.b2cSetExtra({ fields: [{ sym: 'f_a', name: 'a', type: 'int' }] });
    expect(menu(typeField(struct, 'B2C_FIELD0_TYPE'))).toContainEqual([
      'list of Team',
      '{"std::vector":{"ref":"s_Team"}}',
    ]);
    const define = block('func.define');
    if (!isB2cMutatorBlock(define)) {
      throw new Error('no mutator');
    }
    define.b2cSetExtra({ params: [{ sym: 'p_a', name: 'a', type: 'int', mode: 'copy' }] });
    expect(menu(typeField(define, 'B2C_PARAM0_TYPE'))).toContainEqual([
      'Point',
      '{"ref":"s_Point"}',
    ]);
  });

  it('show a chosen user type by its current name', () => {
    const create = block('var.declare');
    typeField(create).setValue('{"std::vector":{"ref":"s_Point"}}');
    expect(typeField(create).getText()).toBe('list of Point');
    names.set('s_Point', 'Spot');
    refreshSymbolNames(workspace);
    expect(typeField(create).getText()).toBe('list of Spot');
  });
});

describe('enum value’s ENUM', () => {
  it('lists enum definitions only, not variables of an enum type', () => {
    const value = block('type.enum_value');
    expect(menu(ref(value, 'ENUM'))).toEqual([
      ['Colour', 's_Colour'],
      ['Size', 's_Size'],
    ]);
  });
});

describe('new … with', () => {
  it('labels its parts by the struct’s fields and offers ⊕ until it has one per field', () => {
    install({ structFields: (_block, id) => symbols.get(id)?.fields ?? null });
    const made = block('type.new');
    if (!isB2cMutatorBlock(made)) {
      throw new Error('no mutator');
    }
    typeField(made).setValue('{"ref":"s_Point"}');
    made.b2cSetExtra({ fieldCount: 1 });
    refreshMutatorLabels(workspace);
    const labels = (name: string): string[] =>
      (made.getInput(name)?.fieldRow ?? [])
        .filter((field) => field instanceof Blockly.FieldLabel)
        .map((field) => field.getText());
    expect(labels('FIELD0')).toContain('x:');
    made.b2cSetExtra({ fieldCount: 2 });
    refreshMutatorLabels(workspace);
    expect(labels('FIELD1')).toContain('y:');
  });
});

describe('switch on an enum', () => {
  function enumValue(owner: string, value: string): Blockly.BlockSvg {
    const made = block('type.enum_value');
    ref(made, 'ENUM').setRef({ ref: owner });
    ref(made, 'VALUE').setRef({ ref: value });
    return made;
  }

  it('takes a variable of an enum, then only that enum’s values as its cases', () => {
    const switched = block('control.switch');
    const get = block('var.get');
    ref(get, 'VAR').setRef({ ref: 'v_c' });
    expect(canDrop(get, switched, 'VALUE')).toBe(true);
    plug(switched, 'VALUE', get);
    expect(canDrop(enumValue('s_Colour', 'e_red'), switched, 'CASE0')).toBe(true);
    plug(switched, 'CASE0', enumValue('s_Colour', 'e_red'));
    expect(canDrop(enumValue('s_Colour', 'e_green'), switched, 'CASE1')).toBe(true);
    // A value of another enum, and a whole number, are no case of a switch on a Colour.
    expect(canDrop(enumValue('s_Size', 'e_small'), switched, 'CASE1')).toBe(false);
    expect(canDrop(block('math.number'), switched, 'CASE1')).toBe(false);
  });

  it('takes an enum value as its first case while nothing is switched on, and then its enum only', () => {
    const switched = block('control.switch');
    plug(switched, 'CASE0', enumValue('s_Colour', 'e_green'));
    expect(canDrop(enumValue('s_Size', 'e_small'), switched, 'CASE1')).toBe(false);
    expect(canDrop(enumValue('s_Colour', 'e_red'), switched, 'CASE1')).toBe(true);
    const get = block('var.get');
    ref(get, 'VAR').setRef({ ref: 'v_c' });
    expect(canDrop(get, switched, 'VALUE')).toBe(true);
    // A struct is no value to switch on.
    const point = block('var.get');
    ref(point, 'VAR').setRef({ ref: 'v_p' });
    expect(canDrop(point, block('control.switch'), 'VALUE')).toBe(false);
  });
});
