/** The editor services: names, scope, static types, dialogs and their installation. */
import type { BdmDocument, CoreWasm, PreviewResult, SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import {
  B2cSymbolRefField,
  DEFAULT_EDITOR_SERVICES,
  EXPR_SHADOW_TYPE,
  getEditorServices,
  isExprShadowType,
  refreshMutatorLabels,
  resetEditorServices,
} from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { DialogService } from '../../app/dialogs/service';
import { useAppStore } from '../../app/store';
import { documentFixture, previewFixture } from '../../app/testing/fixtures';
import {
  disposeWorkspaces,
  EXAMPLE_PROJECTS,
  headlessWorkspace,
  loadText,
  present,
  startSession,
  testCore,
  type TestSession,
} from '../sync/testing';
import {
  analysisScope,
  createSymbolServices,
  defaultInputShadow,
  fieldDialogs,
  installEditorServices,
  MAX_LAST_KNOWN_NAMES,
  SymbolNames,
} from './index';

const realCore = await testCore();

let running: TestSession | null = null;

afterEach(() => {
  running?.dispose();
  running = null;
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

  it('answers scope queries from the core after the first preview, and names from the document first', () => {
    const symbolsInScope = vi.fn(() => [symbol('s_x', 'x')]);
    let core: CoreWasm | null = null;
    let current: PreviewResult | null = null;
    const services = createSymbolServices({
      core: () => core,
      preview: () => current,
      workspace: headlessWorkspace(),
    });
    expect(services.symbols.symbolsAt('b1', null)).toEqual([]);
    core = { symbolsInScope } as unknown as CoreWasm;
    // The core's kept analysis may be another project's until the first preview.
    expect(services.symbols.symbolsAt('b1', 'BODY')).toEqual([]);
    expect(symbolsInScope).not.toHaveBeenCalled();

    expect(services.symbols.nameOf('s_x')).toBeNull();
    current = preview;
    expect(services.symbols.symbolsAt('b1', 'BODY')).toEqual([symbol('s_x', 'x')]);
    expect(symbolsInScope).toHaveBeenCalledWith('b1', 'BODY');
    expect(services.symbols.nameOf('s_x')).toBe('x');
    services.names.update(docDeclaring([['s_x', 'renamed']]));
    expect(services.symbols.nameOf('s_x')).toBe('renamed');
  });

  it('answers nothing from a core that fails (a trap)', () => {
    const trapped = {
      symbolsInScope: () => {
        throw new Error('trapped');
      },
    } as unknown as CoreWasm;
    const scope = analysisScope({ core: () => trapped, preview: () => preview });
    expect(scope.symbolsAt('b1', null)).toEqual([]);
    expect(scope.allSymbols()).toEqual(preview.symbols);
    const services = createSymbolServices({
      core: () => trapped,
      preview: () => preview,
      workspace: headlessWorkspace(),
    });
    expect(services.symbols.symbolsAt('b1', null)).toEqual([]);
  });

  it('gives the analysed type of a block, or the type of the symbol a reference names', () => {
    let current: PreviewResult | null = null;
    const workspace = headlessWorkspace();
    const { types } = createSymbolServices({ core: () => null, preview: () => current, workspace });
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

describe('the scope of a block’s menus', () => {
  const guess = symbol('s_guess', 'guess', { declBlock: 'decl' });
  const secret = symbol('s_secret', 'secret', { declBlock: 'decl2' });

  /**
   * `main` (ID `main`) with `create guess` (`decl`) and `create secret` (`decl2`), a loose getter
   * (`loose`), and a core that answers for main's blocks only.
   */
  function canvas() {
    const workspace = headlessWorkspace();
    Blockly.serialization.blocks.append(
      {
        type: 'program.main',
        id: 'main',
        inputs: {
          BODY: {
            block: {
              type: 'var.declare',
              id: 'decl',
              fields: { TYPE: 'int', NAME: { sym: 's_guess', name: 'guess' }, CONST: false },
              next: {
                block: {
                  type: 'var.declare',
                  id: 'decl2',
                  fields: { TYPE: 'int', NAME: { sym: 's_secret', name: 'secret' }, CONST: false },
                },
              },
            },
          },
        },
      },
      workspace,
    );
    const loose = Blockly.serialization.blocks.append(
      { type: 'var.get', id: 'loose', fields: { VAR: { ref: 's_guess' } } },
      workspace,
    );
    const scopes: Record<string, SymbolInfo[]> = { decl: [], decl2: [guess], 'main/BODY': [] };
    const symbolsInScope = vi.fn(
      (blockId: string, input: string | null) =>
        scopes[input === null ? blockId : `${blockId}/${input}`] ?? [],
    );
    const core = { symbolsInScope } as unknown as CoreWasm;
    const analysed: PreviewResult = { ...previewFixture(), symbols: [guess, secret] };
    const services = createSymbolServices({
      core: () => core,
      preview: () => analysed,
      workspace,
    });
    return { workspace, loose, services, symbolsInScope };
  }

  it('lists what is visible at the end of main for a loose block', () => {
    const { loose, services } = canvas();
    installEditorServices({
      symbols: services.symbols,
      types: services.types,
      dialogs: DEFAULT_EDITOR_SERVICES.dialogs,
    });
    expect(services.symbols.symbolsAt('loose', null)).toEqual([guess, secret]);
    const field = loose.getField('VAR') as B2cSymbolRefField;
    expect(field.menuOptions().map(([label]) => label)).toEqual(['guess', 'secret']);
  });

  it('asks the core as it is for a block that is not on the canvas', () => {
    const { services, symbolsInScope } = canvas();
    expect(services.symbols.symbolsAt('flyout_block', null)).toEqual([]);
    expect(symbolsInScope).toHaveBeenCalledWith('flyout_block', null);
  });
});

describe.skipIf(realCore === null)('the scope of a block’s menus with the compiler core', () => {
  it('lists main’s variables in the menu of a getter left loose on the canvas', async () => {
    const core = present(realCore, 'the compiler core');
    const text = EXAMPLE_PROJECTS['guessing_game.b2c'] ?? '';
    running = startSession(core, loadText(core, text));
    const session = running;
    await session.session.flush();
    expect(useAppStore.getState().analysis.preview).not.toBeNull();
    const getter = session.workspace.newBlock('var.get');
    getter.initModel();
    (getter.getField('VAR') as B2cSymbolRefField).setRef({ ref: 's_guess' });
    await session.session.flush();
    // The analysis answers nothing for the loose getter; its menu still lists main's variables.
    expect(core.symbolsInScope(getter.id, null)).toEqual([]);
    const field = getter.getField('VAR') as B2cSymbolRefField;
    expect(field.menuOptions().map(([label]) => label)).toEqual(['guess', 'secret']);
  });
});

/** `greet(who)` (`f`, a void function of one parameter) and `main` calling it `count` times. */
function callsDocument(count: number): BdmDocument {
  const doc = documentFixture();
  const call = (index: number) => ({
    id: `call${String(index)}`,
    type: 'func.call_stmt',
    v: 1,
    extra: { argCount: 1 },
    fields: { FUNC: { ref: 's_greet' } },
    inputs: { ARG0: { expr: [{ num: String(index) }] } },
  });
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
            x: 400,
            y: 0,
            extra: { params: [{ sym: 's_who', name: 'who', type: 'int', mode: 'copy' }] },
            fields: { NAME: { sym: 's_greet', name: 'greet' }, RETURNS: 'void' },
          },
          {
            id: 'main',
            type: 'program.main',
            v: 1,
            x: 0,
            y: 0,
            statements: { BODY: Array.from({ length: count }, (_, index) => call(index)) },
          },
        ],
      },
    },
  ];
  return doc;
}

describe('the parameters of the function a call names', () => {
  const greet = symbol('s_greet', 'greet', {
    kind: 'function',
    params: ['s_who'],
    returns: 'void',
    type: 'void',
    declBlock: 'f',
  });
  const elsewhere = symbol('s_other', 'other', {
    kind: 'function',
    params: [],
    returns: 'void',
    type: 'void',
    module: 'mod_other',
    declBlock: 'f_other',
  });

  it('come from the latest preview for a call on the canvas, without asking the core', () => {
    const workspace = headlessWorkspace();
    Blockly.serialization.blocks.append({ type: 'func.define', id: 'f' }, workspace);
    Blockly.serialization.blocks.append({ type: 'func.call_stmt', id: 'call' }, workspace);
    const symbolsInScope = vi.fn(() => []);
    let current: PreviewResult | null = null;
    const { symbols } = createSymbolServices({
      core: () => ({ symbolsInScope }) as unknown as CoreWasm,
      preview: () => current,
      workspace,
    });
    expect(symbols.functionParams?.('call', 's_greet')).toBeNull();
    current = { ...previewFixture(), symbols: [greet, elsewhere, symbol('s_x', 'x')] };
    expect(symbols.functionParams?.('call', 's_greet')).toEqual(['s_who']);
    // Not a function, unknown, or another module's function (its definition is not shown here).
    expect(symbols.functionParams?.('call', 's_x')).toBeNull();
    expect(symbols.functionParams?.('call', 's_missing')).toBeNull();
    expect(symbols.functionParams?.('call', 's_other')).toBeNull();
    // A call in the toolbox's flyout is labelled by the toolbox.
    expect(symbols.functionParams?.('flyout_call', 's_greet')).toBeNull();
    expect(symbolsInScope).not.toHaveBeenCalled();
  });
});

describe.skipIf(realCore === null)('call argument labels with the compiler core', () => {
  it('cost no scope query after an analysis, with 50 calls on the canvas', async () => {
    const core = present(realCore, 'the compiler core');
    const symbolsInScope = vi.fn((blockId: string, input: string | null) =>
      core.symbolsInScope(blockId, input),
    );
    running = startSession({ ...core, symbolsInScope }, callsDocument(50));
    const { workspace, session } = running;
    await session.flush();
    expect(useAppStore.getState().analysis.preview).not.toBeNull();
    const calls = workspace.getBlocksByType('func.call_stmt', false);
    expect(calls).toHaveLength(50);
    // The call mutator's label before ARG0 (blockly-ext's `B2C_LABEL_` and the input's name).
    const labels = () => calls.map((call) => call.getField('B2C_LABEL_ARG0')?.getText() ?? null);
    expect(new Set(labels())).toEqual(new Set(['who:']));

    // Another analysis (a block added loose) relabels every call from the preview's symbols.
    symbolsInScope.mockClear();
    workspace.newBlock('io.print').initModel();
    await session.flush();
    refreshMutatorLabels(workspace);
    expect(symbolsInScope).not.toHaveBeenCalled();
    expect(new Set(labels())).toEqual(new Set(['who:']));
  });
});

describe('installing the services', () => {
  it('reaches the fields, and is withdrawn again', () => {
    const services = createSymbolServices({
      core: () => null,
      preview: () => null,
      workspace: headlessWorkspace(),
    });
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
    void getEditorServices().dialogs?.prompt('Name?', 'value');
    expect(prompt).toHaveBeenCalledWith({ message: 'Name?', defaultValue: 'value' });
    void getEditorServices().dialogs?.confirm('Sure?');
    expect(confirm).toHaveBeenCalledWith({ message: 'Sure?' });
    void getEditorServices().dialogs?.alert('Done');
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
