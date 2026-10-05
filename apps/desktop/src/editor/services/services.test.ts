/** The editor services: names, scope, static types, dialogs and their installation. */
import type { BdmDocument, CoreWasm, PreviewResult, SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import {
  B2cSymbolRefField,
  EXPR_SHADOW_TYPE,
  getEditorServices,
  isExprShadowType,
  resetEditorServices,
} from '@blocks2cpp/blockly-ext';
import type * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { DialogService } from '../../app/dialogs/service';
import { documentFixture, previewFixture } from '../../app/testing/fixtures';
import { disposeWorkspaces, headlessWorkspace } from '../sync/testing';
import {
  createSymbolServices,
  defaultInputShadow,
  fieldDialogs,
  installEditorServices,
  MAX_LAST_KNOWN_NAMES,
  SymbolNames,
} from './index';

afterEach(() => {
  disposeWorkspaces();
  resetEditorServices();
});

function docDeclaring(decls: [string, string][], params: [string, string][] = []): BdmDocument {
  const doc = documentFixture();
  doc.modules = [
    {
      id: 'mod_main',
      name: 'main',
      workspace: {
        blocks: [
          {
            id: 'f',
            type: 'func.define',
            v: 1,
            x: 0,
            y: 0,
            extra: {
              params: params.map(([sym, name]) => ({ sym, name, type: 'int', mode: 'copy' })),
            },
            fields: { NAME: { sym: 's_f', name: 'helper' }, RETURNS: 'void' },
            statements: {
              BODY: decls.map(([sym, name], index) => ({
                id: `d${String(index)}`,
                type: 'var.declare',
                v: 1,
                fields: { CONST: false, NAME: { sym, name }, TYPE: 'int' },
              })),
            },
          },
        ],
      },
    },
  ];
  return doc;
}

function symbol(id: string, name: string, extra: Partial<SymbolInfo> = {}): SymbolInfo {
  return {
    id,
    name,
    kind: 'variable',
    isConst: false,
    type: 'int',
    module: 'mod_main',
    declBlock: 'd0',
    ...extra,
  } as SymbolInfo;
}

describe('symbol names', () => {
  it('come from every declaration and parameter of the document, and outlive deletion', () => {
    const names = new SymbolNames();
    expect(names.update(docDeclaring([['s_a', 'apple']], [['s_p', 'count']]))).toBe(true);
    // The same names again: nothing to redraw.
    expect(names.update(docDeclaring([['s_a', 'apple']], [['s_p', 'count']]))).toBe(false);
    expect(names.nameOf('s_a')).toBe('apple');
    expect(names.nameOf('s_p')).toBe('count');
    expect(names.nameOf('s_f')).toBe('helper');

    expect(names.update(docDeclaring([['s_a', 'avocado']]))).toBe(true);
    expect(names.nameOf('s_a')).toBe('avocado');
    // Deleted: the last-known name stays for the session.
    expect(names.nameOf('s_p')).toBe('count');

    names.alias(new Map([['s_a', 's_copy']]));
    expect(names.nameOf('s_copy')).toBe('avocado');

    names.reset();
    expect(names.nameOf('s_a')).toBeNull();
    expect(MAX_LAST_KNOWN_NAMES).toBeGreaterThan(1_000);
  });
});

describe('the symbol provider and the type oracle', () => {
  const preview: PreviewResult = {
    ...previewFixture(),
    blockTypes: JSON.parse(
      '{"b_get": "double", "__proto__": "bool", "b_bad": "nonsense"}',
    ) as never,
    symbols: [
      symbol('s_x', 'x', { type: 'char' }),
      symbol('s_fn', 'area', {
        kind: 'function',
        params: [],
        returns: 'double',
        type: 'double',
      }),
    ],
  };

  it('answers scope queries from the core, and names from the document first', () => {
    const symbolsInScope = vi.fn(() => [symbol('s_x', 'x')]);
    let core: CoreWasm | null = null;
    let current: PreviewResult | null = null;
    const services = createSymbolServices({ core: () => core, preview: () => current });
    expect(services.symbols.symbolsAt('b1', null)).toEqual([]);
    core = { symbolsInScope } as unknown as CoreWasm;
    expect(services.symbols.symbolsAt('b1', 'BODY')).toEqual([symbol('s_x', 'x')]);
    expect(symbolsInScope).toHaveBeenCalledWith('b1', 'BODY');

    expect(services.symbols.nameOf('s_x')).toBeNull();
    current = preview;
    expect(services.symbols.nameOf('s_x')).toBe('x');
    services.names.update(docDeclaring([['s_x', 'renamed']]));
    expect(services.symbols.nameOf('s_x')).toBe('renamed');
  });

  it('gives the analysed type of a block, or the type of the symbol a reference names', () => {
    let current: PreviewResult | null = null;
    const { types } = createSymbolServices({ core: () => null, preview: () => current });
    const workspace = headlessWorkspace();
    const block = (type: string, id: string): Blockly.Block => {
      const created = workspace.newBlock(type, id);
      created.initModel();
      return created;
    };
    const getter = block('var.get', 'b_get');
    expect(types.outputTypeOf(getter)).toBeNull();
    current = preview;
    expect(types.outputTypeOf(getter)).toBe('double');
    // IDs are looked up as own keys only; values that are not static types are ignored.
    expect(types.outputTypeOf(block('math.number', '__proto__'))).toBe('bool');
    expect(types.outputTypeOf(block('math.number', 'b_bad'))).toBeNull();

    const fresh = block('var.get', 'b_new');
    (fresh.getField('VAR') as B2cSymbolRefField).setRef({ ref: 's_x' });
    expect(types.outputTypeOf(fresh)).toBe('char');
    const call = block('func.call', 'b_call');
    (call.getField('FUNC') as B2cSymbolRefField).setRef({ ref: 's_fn' });
    expect(types.outputTypeOf(call)).toBe('double');
    expect(types.outputTypeOf(block('math.number', 'b_other'))).toBeNull();
  });
});

describe('installing the services', () => {
  it('reaches the fields, and is withdrawn again', () => {
    const services = createSymbolServices({ core: () => null, preview: () => null });
    const alert = vi.fn(() => Promise.resolve());
    const confirm = vi.fn(() => Promise.resolve(true));
    const prompt = vi.fn(() => Promise.resolve('typed'));
    const dialogs = { alert, confirm, prompt, choose: vi.fn() } as unknown as DialogService;
    const uninstall = installEditorServices({
      symbols: services.symbols,
      types: services.types,
      dialogs: fieldDialogs(dialogs),
    });
    expect(getEditorServices().symbols).toBe(services.symbols);
    void getEditorServices().dialogs.prompt('Name?', 'value');
    expect(prompt).toHaveBeenCalledWith({ message: 'Name?', defaultValue: 'value' });
    void getEditorServices().dialogs.confirm('Sure?');
    expect(confirm).toHaveBeenCalledWith({ message: 'Sure?' });
    void getEditorServices().dialogs.alert('Done');
    expect(alert).toHaveBeenCalledWith({ message: 'Done' });

    uninstall();
    expect(getEditorServices().symbols).not.toBe(services.symbols);
  });

  it('gives new variadic parts the catalog default as an absent shadow', () => {
    const block = {} as Blockly.Block;
    const withDefault = defaultInputShadow(
      block,
      {
        name: 'ITEM',
        check: 'any',
        optional: false,
        repeat: { count: 'itemCount', plus: 0 },
        default: [{ str: 'Hello, world!' }],
      },
      'ITEM1',
    );
    expect(withDefault?.type).toBe(EXPR_SHADOW_TYPE.str);
    expect(isExprShadowType(withDefault?.type ?? '')).toBe(true);
    expect(withDefault?.extraState).toMatchObject({ absent: true });
    expect(
      defaultInputShadow(
        block,
        { name: 'ARG', check: 'any', optional: false, repeat: null, default: [] },
        'ARG0',
      ),
    ).toBeNull();
  });
});
