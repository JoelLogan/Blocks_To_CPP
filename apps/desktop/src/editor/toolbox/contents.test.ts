import type { SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import { BLOCK_DEFS, GENERATED_ID_PATTERN, TOOLBOX } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import {
  B2C_CATEGORY_KIND,
  CATEGORY_KEY,
  LOOPS_CATEGORY,
  MAKE_VARIABLE_BUTTON,
  MAX_LISTED_FUNCTIONS,
  MAX_LISTED_VARIABLES,
  MY_BLOCKS_CATEGORY,
  MY_BLOCKS_HEADING,
  VARIABLES_CATEGORY,
  categoryOfDefinition,
  displayName,
  functionsContents,
  initialToolboxDefinition,
  loopsContents,
  moduleHeading,
  newVariableBlock,
  staticCategoryContents,
  toolboxDefinition,
  variablesContents,
  type ContentsContext,
  type ModuleInfo,
} from './contents';
import {
  B2C_BLOCK_KIND,
  PresetError,
  blockState,
  instanceFields,
  presetBlock,
  type B2cBlockInfo,
} from './presets';
import type { SymbolSource } from './scope';
import {
  disposeTestWorkspaces,
  headlessWorkspace,
  setUpToolboxBlocks,
  symbolFixture,
} from './testing';

beforeAll(() => {
  setUpToolboxBlocks();
});

afterEach(() => {
  disposeTestWorkspaces();
});

type Item = Blockly.utils.toolbox.FlyoutItemInfo;

function blocksOf(items: readonly Item[]): B2cBlockInfo[] {
  return items.filter((item): item is B2cBlockInfo => item.kind === B2C_BLOCK_KIND);
}

function typesOf(items: readonly Item[]): string[] {
  return blocksOf(items).map((block) => block.type);
}

function labelsOf(items: readonly Item[]): string[] {
  return items.flatMap((item) => (item.kind === 'label' && 'text' in item ? [item.text] : []));
}

/** The shadow of an input of a toolbox block: its type, tokens and absent flag. */
function shadowOf(block: B2cBlockInfo | undefined, input: string) {
  const shadow = block?.inputs?.[input]?.shadow;
  const extra = shadow?.extraState as { tokens?: unknown; absent?: unknown } | undefined;
  return shadow === undefined
    ? undefined
    : {
        type: shadow.type,
        value: shadow.fields?.['VALUE'] as unknown,
        tokens: extra?.tokens,
        absent: extra?.absent,
      };
}

/** A context with a headless canvas holding `main` (ID `main`), and fake symbols. */
function context(
  options: {
    scopes?: Record<string, readonly SymbolInfo[]>;
    all?: readonly SymbolInfo[];
    modules?: readonly ModuleInfo[];
    main?: boolean;
  } = {},
): ContentsContext & { workspace: Blockly.Workspace } {
  const workspace = headlessWorkspace();
  if (options.main !== false) {
    Blockly.serialization.blocks.append({ type: 'program.main', id: 'main' }, workspace);
  }
  const symbols: SymbolSource = {
    symbolsAt: (blockId, input) =>
      options.scopes?.[input === null ? blockId : `${blockId}/${input}`] ?? [],
    allSymbols: () => options.all ?? [],
  };
  return {
    workspace,
    symbols,
    selected: () => null,
    modules: () => options.modules ?? [{ id: 'mod_main', name: 'main' }],
  };
}

describe('the toolbox definition', () => {
  it('has every category in catalog order, with name, colour style and icon key', () => {
    const definition = toolboxDefinition();
    expect(definition.kind).toBe('categoryToolbox');
    const categories = definition.contents as (Blockly.utils.toolbox.CategoryInfo & {
      name: string;
    })[];
    expect(categories.map((category) => category.name)).toEqual([
      'Program',
      'Variables',
      'Math',
      'Logic',
      'Text',
      'Control',
      'Loops',
      'Input / Output',
      'Functions',
    ]);
    for (const [index, category] of categories.entries()) {
      const id = TOOLBOX[index]?.id;
      expect(category.kind).toBe(B2C_CATEGORY_KIND);
      expect(category.categorystyle).toBe(`b2c_${String(id)}`);
      expect(categoryOfDefinition(category)).toBe(id);
      // Blockly gives each category a unique DOM ID itself.
      expect(category.id).toBeUndefined();
    }
  });

  it('builds Variables, Loops and Functions when they are shown', () => {
    const custom = toolboxDefinition().contents.flatMap((item) =>
      'custom' in item ? [[categoryOfDefinition(item), item.custom]] : [],
    );
    expect(custom).toEqual([
      ['variables', VARIABLES_CATEGORY],
      ['loops', LOOPS_CATEGORY],
      ['functions', MY_BLOCKS_CATEGORY],
    ]);
  });

  it('starts with static categories only, so a workspace can be injected with it', () => {
    const initial = initialToolboxDefinition();
    expect(initial.contents.some((item) => 'custom' in item)).toBe(false);
    expect(initial.contents).toHaveLength(TOOLBOX.length);
  });

  it('finds no category key in definitions that are not ours', () => {
    expect(categoryOfDefinition(null)).toBeNull();
    expect(categoryOfDefinition({ kind: 'category' })).toBeNull();
    expect(categoryOfDefinition({ [CATEGORY_KEY]: 'nonsense' })).toBeNull();
  });
});

describe('static categories', () => {
  it('show catalog defaults as editable shadows that stay absent until edited', () => {
    const math = blocksOf(staticCategoryContents('math'));
    const random = math.find((block) => block.type === 'math.random_int');
    expect(shadowOf(random, 'LOW')).toMatchObject({
      type: 'b2c.expr.num',
      value: '1',
      absent: true,
    });
    expect(shadowOf(random, 'HIGH')).toMatchObject({ value: '6', absent: true });

    const print = blocksOf(staticCategoryContents('io')).find((block) => block.type === 'io.print');
    expect(print?.extraState).toEqual({ itemCount: 1 });
    expect(shadowOf(print, 'ITEM0')).toMatchObject({
      type: 'b2c.expr.str',
      value: 'Hello, world!',
      absent: true,
    });

    const loops = blocksOf(staticCategoryContents('loops'));
    expect(
      shadowOf(
        loops.find((block) => block.type === 'control.repeat'),
        'TIMES',
      ),
    ).toMatchObject({ value: '10', absent: true });
    const conditions = [
      shadowOf(blocksOf(staticCategoryContents('control'))[0], 'COND0'),
      shadowOf(
        loops.find((block) => block.type === 'control.while'),
        'COND',
      ),
      shadowOf(
        blocksOf(staticCategoryContents('logic')).find((b) => b.type === 'logic.not'),
        'A',
      ),
    ];
    for (const condition of conditions) {
      expect(condition).toMatchObject({ type: 'b2c.expr.kw', value: 'true', absent: true });
    }
  });

  it('leave inputs without a default empty', () => {
    const fn = blocksOf(staticCategoryContents('functions')).find((b) => b.type === 'func.return');
    expect(fn?.inputs).toBeUndefined();
  });

  it('give the presets of the toolbox metadata, with the label shown above the block', () => {
    const control = staticCategoryContents('control');
    expect(labelsOf(control)).toEqual(['if … else']);
    const [plainIf, ifElse] = blocksOf(control);
    expect(plainIf?.extraState).toEqual({ elseIfCount: 0, hasElse: false });
    expect(ifElse?.extraState).toEqual({ elseIfCount: 0, hasElse: true });
    expect(shadowOf(ifElse, 'COND0')).toMatchObject({ value: 'true' });

    const loops = staticCategoryContents('loops');
    expect(labelsOf(loops)).toEqual(['repeat until']);
    const until = blocksOf(loops).filter((block) => block.type === 'control.while')[1];
    expect(until?.fields).toEqual({ MODE: 'until' });

    const ask = blocksOf(staticCategoryContents('io')).find((block) => block.type === 'io.ask');
    // A preset value is explicit: it is saved, unlike a catalog default.
    expect(shadowOf(ask, 'PROMPT')).toMatchObject({ value: 'Your answer: ', absent: false });
  });

  it('refuse a block the catalog does not have', () => {
    expect(() => presetBlock('no.such_block')).toThrow(PresetError);
  });
});

describe('the Variables category', () => {
  const guess = symbolFixture('s_guess', 'guess');
  const secret = symbolFixture('s_secret', 'secret', { isConst: true });
  const counter = symbolFixture('s_i', 'i', { kind: 'loopVariable' });
  const stop = symbolFixture('s_stop', 'stop', { kind: 'function', params: [], returns: 'void' });

  it('offers Make a variable and a create block with a free name and a fresh symbol', () => {
    const items = variablesContents(
      context({ scopes: { 'main/BODY': [symbolFixture('s_v', 'value')] } }),
    );
    expect(items[0]).toEqual({
      kind: 'button',
      text: 'Make a variable',
      callbackkey: MAKE_VARIABLE_BUTTON,
    });
    const [create] = blocksOf(items);
    expect(create?.type).toBe('var.declare');
    expect(create?.declares).toEqual({ NAME: 'value2' });
    expect(create?.fields?.['NAME']).toBeUndefined();
    expect(create?.fields?.['TYPE']).toBe('int');
    expect(shadowOf(create, 'VALUE')).toMatchObject({ value: '0', absent: false });
  });

  it('declares a new symbol for every block created from the create item', () => {
    const [create] = blocksOf(variablesContents(context()));
    if (create === undefined) {
      throw new Error('no create block');
    }
    const first = instanceFields(create)?.['NAME'] as { sym: string; name: string };
    const second = instanceFields(create)?.['NAME'] as { sym: string; name: string };
    expect(first.name).toBe('value');
    expect(first.sym).toMatch(GENERATED_ID_PATTERN.sym);
    expect(second.sym).toMatch(GENERATED_ID_PATTERN.sym);
    expect(second.sym).not.toBe(first.sym);
  });

  it('builds equal items for the same project, so an unchanged toolbox is not redrawn', () => {
    const ctx = context({ scopes: { 'main/BODY': [guess, secret] } });
    expect(JSON.stringify(variablesContents(ctx))).toBe(JSON.stringify(variablesContents(ctx)));
    expect(JSON.stringify(loopsContents(ctx))).toBe(JSON.stringify(loopsContents(ctx)));
    expect(JSON.stringify(functionsContents(ctx))).toBe(JSON.stringify(functionsContents(ctx)));
  });

  it('lists a getter, set, change and update for each variable in scope', () => {
    const items = variablesContents(context({ scopes: { 'main/BODY': [guess, counter, stop] } }));
    const listed = blocksOf(items).slice(1);
    expect(listed.map((block) => [block.type, block.fields?.['VAR'] as unknown])).toEqual([
      ['var.get', { ref: 's_guess' }],
      ['var.set', { ref: 's_guess' }],
      ['var.change', { ref: 's_guess' }],
      ['var.update', { ref: 's_guess' }],
      ['var.get', { ref: 's_i' }],
      ['var.set', { ref: 's_i' }],
      ['var.change', { ref: 's_i' }],
      ['var.update', { ref: 's_i' }],
    ]);
    // set starts at a value of the variable's type; change and update keep the catalog default.
    expect(shadowOf(listed[1], 'VALUE')).toMatchObject({ value: '0', absent: false });
    expect(shadowOf(listed[2], 'BY')).toMatchObject({ value: '1', absent: true });
  });

  it('leaves constants out of set, change and update', () => {
    const items = variablesContents(context({ scopes: { 'main/BODY': [secret] } }));
    expect(typesOf(items)).toEqual(['var.declare', 'var.get']);
  });

  it('gives set a start value of the variable type', () => {
    const typed = (type: string) =>
      shadowOf(
        blocksOf(
          variablesContents(
            context({ scopes: { 'main/BODY': [symbolFixture('s_x', 'x', { type })] } }),
          ),
        ).find((block) => block.type === 'var.set'),
        'VALUE',
      );
    expect(typed('double')).toMatchObject({ type: 'b2c.expr.num', value: '0.0' });
    expect(typed('bool')).toMatchObject({ type: 'b2c.expr.kw', value: 'true' });
    expect(typed('char')).toMatchObject({ type: 'b2c.expr.chr', value: 'a' });
    expect(typed('string')).toMatchObject({ type: 'b2c.expr.str', value: '' });
    expect(typed('error')).toBeUndefined();
  });

  it('lists at most MAX_LISTED_VARIABLES variables', () => {
    const many = Array.from({ length: MAX_LISTED_VARIABLES + 5 }, (_, index) =>
      symbolFixture(`s_${String(index)}`, `v${String(index)}`),
    );
    const items = variablesContents(context({ scopes: { 'main/BODY': many } }));
    expect(typesOf(items).filter((type) => type === 'var.get')).toHaveLength(MAX_LISTED_VARIABLES);
  });

  it('lists nothing in scope when there is no main and nothing is selected', () => {
    const items = variablesContents(context({ main: false, scopes: { 'main/BODY': [guess] } }));
    expect(typesOf(items)).toEqual(['var.declare']);
  });
});

describe('the Loops category', () => {
  it('names a new counted loop i, or the next free name', () => {
    const loopName = (scope: readonly SymbolInfo[]) =>
      blocksOf(loopsContents(context({ scopes: { 'main/BODY': scope } }))).find(
        (block) => block.type === 'control.for_range',
      )?.declares?.['VAR'];
    expect(loopName([])).toBe('i');
    expect(loopName([symbolFixture('s_i', 'i', { kind: 'loopVariable' })])).toBe('j');
    expect(loopName(['i', 'j', 'k'].map((name) => symbolFixture(`s_${name}`, name)))).toBe('i2');
  });

  it('keeps every catalog entry of Loops', () => {
    expect(typesOf(loopsContents(context()))).toEqual(
      TOOLBOX.find((category) => category.id === 'loops')?.entries.map((entry) => entry.block),
    );
  });
});

describe('the Functions category and My Blocks', () => {
  const n = symbolFixture('s_n', 'n', { kind: 'parameter', mode: 'copy', type: 'int' });
  const label = symbolFixture('s_label', 'label', {
    kind: 'parameter',
    mode: 'read_only',
    type: 'string',
  });
  const factorial = symbolFixture('s_fact', 'factorial', {
    kind: 'function',
    params: ['s_n'],
    returns: 'int',
    type: 'int',
  });
  const show = symbolFixture('s_show', 'show', {
    kind: 'function',
    params: ['s_label', 's_n'],
    returns: 'void',
    type: 'void',
    module: 'mod_extra',
  });

  it('names a new function myFunction, or the next free name', () => {
    const defineName = (all: readonly SymbolInfo[]) =>
      blocksOf(functionsContents(context({ all })))[0]?.declares?.['NAME'];
    expect(defineName([])).toBe('myFunction');
    expect(
      defineName([
        symbolFixture('s_f', 'myFunction', { kind: 'function', params: [], returns: 'void' }),
      ]),
    ).toBe('myFunction2');
  });

  it('shows only the catalog entries while there are no functions', () => {
    const items = functionsContents(context());
    expect(typesOf(items)).toEqual(['func.define', 'func.return']);
    expect(labelsOf(items)).toEqual([]);
  });

  it('lists a call for every function, grouped under module headings', () => {
    const items = functionsContents(
      context({
        all: [factorial, label, n, show],
        modules: [
          { id: 'mod_main', name: 'main' },
          { id: 'mod_extra', name: 'helpers' },
        ],
      }),
    );
    expect(labelsOf(items)).toEqual([MY_BLOCKS_HEADING, 'Module: main', 'Module: helpers']);
    const calls = blocksOf(items).slice(2);
    // A function that gives a value is a reporter; a void function is a statement.
    expect(calls.map((call) => call.type)).toEqual(['func.call', 'func.call_stmt']);
    const [factorialCall, showCall] = calls;
    expect(factorialCall?.fields).toEqual({ FUNC: { ref: 's_fact' } });
    expect(factorialCall?.extraState).toEqual({ argCount: 1 });
    expect(factorialCall?.argLabels).toEqual(['n:']);
    expect(shadowOf(factorialCall, 'ARG0')).toMatchObject({ value: '0', absent: false });
    expect(showCall?.extraState).toEqual({ argCount: 2 });
    expect(showCall?.argLabels).toEqual(['label:', 'n:']);
    expect(shadowOf(showCall, 'ARG0')).toMatchObject({ type: 'b2c.expr.str', value: '' });
  });

  it('puts modules the document does not list last, and shows unknown parameters unlabelled', () => {
    const lost = symbolFixture('s_lost', 'lost', {
      kind: 'function',
      params: ['s_gone'],
      returns: 'double',
      module: 'mod_unknown',
    });
    const items = functionsContents(context({ all: [lost, factorial, n] }));
    expect(labelsOf(items)).toEqual([MY_BLOCKS_HEADING, 'Module: main', 'Module: mod_unknown']);
    const lostCall = blocksOf(items).at(-1);
    expect(lostCall?.extraState).toEqual({ argCount: 1 });
    expect(lostCall?.argLabels).toBeUndefined();
    expect(lostCall?.inputs).toBeUndefined();
  });

  it('lists at most MAX_LISTED_FUNCTIONS functions', () => {
    const many = Array.from({ length: MAX_LISTED_FUNCTIONS + 3 }, (_, index) =>
      symbolFixture(`s_f${String(index)}`, `f${String(index)}`, {
        kind: 'function',
        params: [],
        returns: 'void',
      }),
    );
    const calls = typesOf(functionsContents(context({ all: many }))).filter(
      (type) => type === 'func.call_stmt',
    );
    expect(calls).toHaveLength(MAX_LISTED_FUNCTIONS);
  });

  it('shows hidden characters and cuts long names in headings and labels', () => {
    expect(moduleHeading('a​b')).toBe('Module: a⟨U+200B⟩b');
    expect(displayName('x'.repeat(100)).length).toBeLessThanOrEqual(40);
  });
});

describe('coverage of the catalog', () => {
  it('reaches all 32 catalog blocks through the static and dynamic categories', () => {
    const variable = symbolFixture('s_v', 'v');
    const value = symbolFixture('s_f', 'f', { kind: 'function', params: [], returns: 'int' });
    const action = symbolFixture('s_g', 'g', { kind: 'function', params: [], returns: 'void' });
    const ctx = context({ scopes: { 'main/BODY': [variable] }, all: [value, action] });
    const reached = new Set([
      ...TOOLBOX.flatMap((category) => typesOf(staticCategoryContents(category.id))),
      ...typesOf(variablesContents(ctx)),
      ...typesOf(loopsContents(ctx)),
      ...typesOf(functionsContents(ctx)),
    ]);
    expect(BLOCK_DEFS).toHaveLength(32);
    expect([...reached].sort()).toEqual(BLOCK_DEFS.map((def) => def.id).sort());
  });
});

describe('newVariableBlock', () => {
  it('is the create block of the Variables category with the given name', () => {
    const block = newVariableBlock('score');
    expect(block.type).toBe('var.declare');
    expect(block.declares).toEqual({ NAME: 'score' });
    expect(block.fields?.['TYPE']).toBe('int');
    const state = blockState(block);
    expect(state.fields?.['NAME']).toMatchObject({ name: 'score' });
    expect((state.fields?.['NAME'] as { sym: string }).sym).toMatch(GENERATED_ID_PATTERN.sym);
  });
});

describe('presets', () => {
  it('refuse a declaration on a field that does not declare', () => {
    expect(() => presetBlock('var.declare', { declare: { TYPE: 'x' } })).toThrow(PresetError);
    expect(() => presetBlock('var.declare', { declare: { NOPE: 'x' } })).toThrow(PresetError);
  });

  it('keep only fields the block has, and counts and flags from the catalog defaults', () => {
    const block = presetBlock('io.print', { fields: { NOPE: 'x', NEWLINE: false } });
    expect(block.fields).toEqual({ NEWLINE: false });
    expect(block.extraState).toEqual({ itemCount: 1 });
    expect(presetBlock('func.define').extraState).toEqual({ params: [] });
  });

  it('give static categories the first default name of each declaration', () => {
    const declares = (id: 'variables' | 'loops' | 'functions', type: string) =>
      blocksOf(staticCategoryContents(id)).find((block) => block.type === type)?.declares;
    expect(declares('variables', 'var.declare')).toEqual({ NAME: 'value' });
    expect(declares('loops', 'control.for_range')).toEqual({ VAR: 'i' });
    expect(declares('functions', 'func.define')).toEqual({ NAME: 'myFunction' });
  });
});
