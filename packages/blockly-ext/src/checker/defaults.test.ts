/**
 * Package C2's default values on the canvas (docs/plans/m3-contracts/w2.md §2.8): a parameter row's
 * `DEFAULT<i>` takes its own parameter's type (`param:NAME` on the function's declaration), read
 * from the row's type field, so it needs no analysis.
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { registerB2cBlocks } from '../blocks/register';
import { B2cSymbolRefField } from '../fields/symbol-ref';
import { installIdGenerator } from '../ids';
import { isB2cMutatorBlock, registerB2cMutators } from '../mutators/register';
import {
  resetEditorServices,
  setEditorServices,
  type SymbolInfo,
  type SymbolProvider,
  type TypeOracle,
} from '../services';
import { parseType, type TypeFactsTs, type TypeJson, type UserTypeTs } from '../types/static-type';
import { B2C_CHECKER_NAME } from './checker';

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

function canDrop(child: Blockly.Block, parent: Blockly.Block, name: string): boolean {
  const to = parent.getInput(name)?.connection;
  const from = child.outputConnection;
  if (to === null || to === undefined || from === null) {
    throw new Error(`no connection for ${name}`);
  }
  return workspace.connectionChecker.canConnect(from, to, true, 1e9);
}

/** `define f(times: int, label: string)`. */
function define(): Blockly.BlockSvg {
  const created = block('func.define');
  if (!isB2cMutatorBlock(created)) {
    throw new Error('no mutator');
  }
  created.b2cSetExtra({
    params: [
      { sym: 'p_times', name: 'times', type: 'int', mode: 'copy' },
      { sym: 'p_label', name: 'label', type: 'std::string', mode: 'read_only' },
    ],
  });
  created.render();
  return created;
}

describe('a default value', () => {
  it('takes its own parameter’s type', () => {
    const f = define();
    const number = block('math.number');
    const text = block('text.literal');
    expect(canDrop(number, f, 'DEFAULT0')).toBe(true);
    expect(canDrop(text, f, 'DEFAULT0')).toBe(false);
    expect(canDrop(text, f, 'DEFAULT1')).toBe(true);
    expect(canDrop(number, f, 'DEFAULT1')).toBe(false);
  });

  it('follows its row’s type when it changes', () => {
    const f = define();
    f.setFieldValue('std::string', 'B2C_PARAM0_TYPE');
    expect(canDrop(block('text.literal'), f, 'DEFAULT0')).toBe(true);
    expect(canDrop(block('math.number'), f, 'DEFAULT0')).toBe(false);
  });

  it('takes a variable of its type, as the analysis lists it', () => {
    symbols.set('v_n', sym('v_n', 'variable', 'int', { name: 'n' }));
    const f = define();
    const get = block('var.get');
    const field = get.getField('VAR');
    if (!(field instanceof B2cSymbolRefField)) {
      throw new Error('VAR is not a reference field');
    }
    field.setValue({ ref: 'v_n' });
    expect(canDrop(get, f, 'DEFAULT0')).toBe(true);
    expect(canDrop(get, f, 'DEFAULT1')).toBe(false);
  });
});
