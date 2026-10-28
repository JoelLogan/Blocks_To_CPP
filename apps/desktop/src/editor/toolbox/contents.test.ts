import type { SymbolInfo, TypeJson } from '@blocks2cpp/b2c-core-wasm';
import { BLOCK_DEFS, GENERATED_ID_PATTERN, TOOLBOX, catalogBlock } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';

import {
  B2C_CATEGORY_KIND,
  CATEGORY_KEY,
  MAKE_VARIABLE_BUTTON,
  MAKE_VARIABLE_TEXT,
  MAX_LISTED_FUNCTIONS,
  MAX_LISTED_GETTERS,
  MY_BLOCKS_CATEGORY,
  NO_FUNCTIONS_HINT,
  PROGRAM_CATEGORY,
  PROGRAM_HERE_NOTE,
  VARIABLES_CATEGORY,
  YOUR_FUNCTIONS_HEADING,
  categoryOfDefinition,
  displayName,
  dynamicContentsKey,
  functionsContents,
  initialToolboxDefinition,
  isShownCategory,
  moduleHeading,
  moreVariablesNote,
  newVariableBlock,
  programContents,
  staticCategoryContents,
  toolboxDefinition,
  toolboxSymbolGroups,
  variableGroups,
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
import {
  disposeTestWorkspaces,
  fakeSymbols,
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

/** The symbol a block item's VAR field is preset to, or `null`. */
function varOf(block: B2cBlockInfo | undefined): string | null {
  const value = block?.fields?.['VAR'] as { ref?: string } | undefined;
  return value?.ref ?? null;
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

/** The `create` blocks a container holds in its body, by block ID (for {@link context}). */
type Declarations = Readonly<Record<string, readonly string[]>>;

/** The Blockly state of a body holding `create` blocks with the given IDs. */
function bodyOf(ids: readonly string[] | undefined): Blockly.serialization.blocks.State['inputs'] {
  let next: Blockly.serialization.blocks.State | null = null;
  for (const id of [...(ids ?? [])].reverse()) {
    next = { type: 'var.declare', id, ...(next === null ? {} : { next: { block: next } }) };
  }
  return next === null ? undefined : { BODY: { block: next } };
}

/**
 * A context with a headless canvas: `main` (ID `main`) unless `main` is false, a `define` for each
 * ID of `functions`, and in each container's body `create` blocks with the IDs `declarations` gives
 * (so that a symbol whose `declBlock` is one of them belongs to that container). The canvas shows
 * the module `active` (`mod_main` unless given); the program starts where `programModule` says
 * (the module shown when the canvas has `main`, by default).
 */
function context(
  options: {
    all?: readonly SymbolInfo[];
    modules?: readonly ModuleInfo[];
    active?: string;
    main?: boolean;
    functions?: readonly string[];
    declarations?: Declarations;
    programModule?: string | null;
  } = {},
): ContentsContext & { workspace: Blockly.Workspace } {
  const workspace = headlessWorkspace();
  const declarations = options.declarations ?? {};
  if (options.main !== false) {
    const inputs = bodyOf(declarations['main']);
    Blockly.serialization.blocks.append(
      { type: 'program.main', id: 'main', ...(inputs === undefined ? {} : { inputs }) },
      workspace,
    );
  }
  for (const id of options.functions ?? []) {
    const inputs = bodyOf(declarations[id]);
    Blockly.serialization.blocks.append(
      { type: 'func.define', id, ...(inputs === undefined ? {} : { inputs }) },
      workspace,
    );
  }
  const active = options.active ?? 'mod_main';
  return {
    workspace,
    symbols: fakeSymbols({}, options.all ?? []),
    modules: () => options.modules ?? [{ id: 'mod_main', name: 'main' }],
    activeModuleId: () => active,
    programModuleId: () =>
      options.programModule !== undefined
        ? options.programModule
        : options.main === false
          ? null
          : active,
  };
}

/**
 * Counts the parent links followed on `workspace` from now on, and returns how to read the count.
 * Blockly's `getParent()` and `getRootBlock()` both read a block's protected `parentBlock_`, so
 * each block's is replaced by an accessor that counts its reads.
 */
function countParentLinks(workspace: Blockly.Workspace): () => number {
  let reads = 0;
  for (const block of workspace.getAllBlocks(false)) {
    const own = Object.getOwnPropertyDescriptor(block, 'parentBlock_');
    if (own === undefined || !('value' in own)) {
      throw new Error('Blockly no longer keeps a block’s parent in parentBlock_');
    }
    let parent: unknown = own.value;
    Object.defineProperty(block, 'parentBlock_', {
      configurable: true,
      enumerable: own.enumerable ?? true,
      get() {
        reads += 1;
        return parent;
      },
      set(value: unknown) {
        parent = value;
      },
    });
  }
  return () => reads;
}

/** A variable declared by `create` block `declBlock`. */
function variable(
  id: string,
  name: string,
  declBlock: string,
  overrides: Partial<Record<string, unknown>> = {},
): SymbolInfo {
  return symbolFixture(id, name, { declBlock, ...overrides });
}

describe('the toolbox definition', () => {
  it('has every category in catalog order, with name, colour style and icon key', () => {
    const definition = toolboxDefinition();
    expect(definition.kind).toBe('categoryToolbox');
    const categories = definition.contents as (Blockly.utils.toolbox.CategoryInfo & {
      name: string;
    })[];
    // M2's nine, then each of W2's four once its package has given it blocks (w2.md §2.1), so a
    // package that fills its category need not change this list.
    const w2 = ['Collections', 'Types', 'Files', 'Errors'];
    const filled = TOOLBOX.filter((category) => category.entries.length > 0).map(
      (category) => category.name,
    );
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
      ...w2.filter((name) => filled.includes(name)),
    ]);
    const shown = TOOLBOX.filter(isShownCategory);
    for (const [index, category] of categories.entries()) {
      const id = shown[index]?.id;
      expect(category.kind).toBe(B2C_CATEGORY_KIND);
      expect(category.categorystyle).toBe(`b2c_${String(id)}`);
      expect(categoryOfDefinition(category)).toBe(id);
      // Blockly gives each category a unique DOM ID itself.
      expect(category.id).toBeUndefined();
    }
  });

  it('builds Program, Variables and Functions when they are shown; Loops is static', () => {
    const custom = toolboxDefinition().contents.flatMap((item) =>
      'custom' in item ? [[categoryOfDefinition(item), item.custom]] : [],
    );
    expect(custom).toEqual([
      ['program', PROGRAM_CATEGORY],
      ['variables', VARIABLES_CATEGORY],
      ['functions', MY_BLOCKS_CATEGORY],
    ]);
  });

  it('starts with static categories only, so a workspace can be injected with it', () => {
    const initial = initialToolboxDefinition();
    expect(initial.contents.some((item) => 'custom' in item)).toBe(false);
    expect(initial.contents).toHaveLength(TOOLBOX.filter(isShownCategory).length);
  });

  it('leaves out a category without entries that the editor does not build (w2.md §2.1)', () => {
    const empty = {
      id: 'errors',
      name: 'Errors',
      icon: '⚠',
      colour: 'errors',
      dynamic: null,
      entries: [],
    } as const;
    expect(isShownCategory(empty)).toBe(false);
    expect(
      isShownCategory({ ...empty, entries: [{ block: 'x', label: null, preset: null }] }),
    ).toBe(true);
    // The editor builds Program, Variables and Functions, also while their entries are empty.
    expect(isShownCategory({ ...empty, id: 'program' })).toBe(true);
    expect(isShownCategory({ ...empty, id: 'variables', dynamic: 'variables' })).toBe(true);
    expect(toolboxDefinition().contents.map((item) => categoryOfDefinition(item))).toEqual(
      TOOLBOX.filter(isShownCategory).map((category) => category.id),
    );
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

  it('offer each block once: its other forms are chosen on the block (03 §3.1)', () => {
    // *else if* and *else* come with ⊕ on the one *if*, more cases and the default part with ⊕ on
    // the one *switch on*, *until* from the one loop's menu.
    const control = staticCategoryContents('control');
    expect(labelsOf(control)).toEqual([]);
    expect(typesOf(control)).toEqual([
      'control.if',
      'control.switch',
      'control.fallthrough',
      'control.wait',
    ]);
    expect(blocksOf(control)[0]?.extraState).toEqual({ elseIfCount: 0, hasElse: false });
    expect(blocksOf(control)[1]?.extraState).toEqual({ caseCount: 2, hasDefault: true });

    const loops = staticCategoryContents('loops');
    expect(labelsOf(loops)).toEqual([]);
    const loopTypes = typesOf(loops);
    expect(loopTypes.filter((type) => type === 'control.while')).toHaveLength(1);
    expect(new Set(loopTypes).size).toBe(loopTypes.length);
    expect(blocksOf(loops).find((block) => block.type === 'control.while')?.fields).toBeUndefined();
    expect(loopTypes.filter((type) => type === 'control.do_while')).toHaveLength(1);
  });

  it('give the presets of the toolbox metadata', () => {
    const ask = blocksOf(staticCategoryContents('io')).find((block) => block.type === 'io.ask');
    // A preset value is explicit: it is saved, unlike a catalog default.
    expect(shadowOf(ask, 'PROMPT')).toMatchObject({ value: 'Your answer: ', absent: false });
  });

  it('name a new counted loop i, whatever is in scope: it is named where it lands', () => {
    const loop = blocksOf(staticCategoryContents('loops')).find(
      (block) => block.type === 'control.for_range',
    );
    expect(loop?.declares).toEqual({ VAR: 'i' });
  });

  it('refuse a block the catalog does not have', () => {
    expect(() => presetBlock('no.such_block')).toThrow(PresetError);
  });
});

describe('the Variables category', () => {
  /**
   * In main: `guess`, the constant `answer` and the text `apple` (both sorted before `guess`); in
   * greet: a parameter and a variable.
   */
  const guess = variable('s_guess', 'guess', 'd_guess');
  const secret = variable('s_secret', 'answer', 'd_secret', { isConst: true });
  const label = variable('s_label', 'apple', 'd_label', { type: 'std::string' });
  const n = symbolFixture('s_n', 'n', { kind: 'parameter', mode: 'copy', declBlock: 'fn_greet' });
  const count = variable('s_count', 'count', 'd_count', { type: 'double' });
  const greet = symbolFixture('s_greet', 'greet', {
    kind: 'function',
    params: ['s_n'],
    returns: 'void',
    type: 'void',
    declBlock: 'fn_greet',
  });
  const game = {
    functions: ['fn_greet'],
    declarations: { main: ['d_secret', 'd_guess', 'd_label'], fn_greet: ['d_count'] },
  };

  it('offers Make a variable, the create and global blocks named value, and one set', () => {
    const items = variablesContents(context());
    expect(items[0]).toEqual({
      kind: 'button',
      text: 'Make a variable',
      callbackkey: MAKE_VARIABLE_BUTTON,
    });
    const [create, global, set, ...rest] = blocksOf(items);
    // A global, right after create (w2.md §2.7): an int variable starting at 0.
    expect(global?.type).toBe('var.global');
    expect(global?.declares).toEqual({ NAME: 'value' });
    expect(global?.fields?.['TYPE']).toBe('int');
    expect(shadowOf(global, 'VALUE')).toMatchObject({ value: '0', absent: false });
    expect(create?.type).toBe('var.declare');
    // The flyout shows the first default name: a free one is chosen where the block lands.
    expect(create?.declares).toEqual({ NAME: 'value' });
    expect(create?.fields?.['NAME']).toBeUndefined();
    expect(create?.fields?.['TYPE']).toBe('int');
    expect(shadowOf(create, 'VALUE')).toMatchObject({ value: '0', absent: false });
    // No variable yet: set has none, and no start value, and the drop may choose one.
    expect(set?.type).toBe('var.set');
    expect(varOf(set)).toBeNull();
    expect(set?.inputs).toBeUndefined();
    expect(set?.retarget).toBe('VAR');
    expect(rest).toEqual([]);
    // The name stays value when value exists: it does not depend on what is in scope.
    const named = variablesContents(context({ all: [variable('s_v', 'value', 'd_v')] }));
    expect(blocksOf(named)[0]?.declares).toEqual({ NAME: 'value' });
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

  it('has one set and one change, preset to main’s first fitting variable by name', () => {
    const items = variablesContents(
      context({ ...game, all: [guess, secret, label, n, count, greet] }),
    );
    const generic = blocksOf(items).filter((block) =>
      ['var.set', 'var.change', 'var.update'].includes(block.type),
    );
    // answer comes first by name but is a constant; apple may be set. change takes the first
    // number of main (apple is text), not count, which greet declares. The update operators are
    // in change's menu: the catalog has no separate update block (W4-5).
    expect(catalogBlock('var.update')).toBeUndefined();
    expect(generic.map((block) => [block.type, varOf(block), block.retarget])).toEqual([
      ['var.set', 's_label', 'VAR'],
      ['var.change', 's_guess', 'VAR'],
    ]);
    // set starts at the start value of its variable's type; change keeps the catalog default.
    expect(shadowOf(generic[0], 'VALUE')).toMatchObject({ type: 'b2c.expr.str', value: '' });
    expect(shadowOf(generic[1], 'BY')).toMatchObject({ value: '1', absent: true });
  });

  it('shows change only while a whole-number, decimal or character variable exists', () => {
    const generic = (all: readonly SymbolInfo[]) =>
      typesOf(variablesContents(context({ ...game, all }))).filter((type) =>
        ['var.set', 'var.change', 'var.update'].includes(type),
      );
    const only = ['var.set'];
    const all = ['var.set', 'var.change'];
    // Text, true/false, a constant, a loop counter, a read-only parameter, an unknown type.
    expect(generic([label])).toEqual(only);
    expect(generic([variable('s_b', 'b', 'd_guess', { type: 'bool' })])).toEqual(only);
    expect(generic([secret])).toEqual(only);
    expect(generic([variable('s_i', 'i', 'd_guess', { kind: 'loopVariable' })])).toEqual(only);
    const readOnly = symbolFixture('s_ro', 'ro', {
      kind: 'parameter',
      mode: 'read_only',
      declBlock: 'fn_greet',
    });
    expect(generic([readOnly])).toEqual(only);
    expect(generic([variable('s_e', 'e', 'd_guess', { type: 'error' })])).toEqual(only);
    for (const type of ['int', 'double', 'char']) {
      expect(generic([variable('s_x', 'x', 'd_guess', { type })]), type).toEqual(all);
    }
    // A number variable of greet's is enough; the presets then name it.
    const items = variablesContents(context({ ...game, all: [label, count] }));
    expect(
      blocksOf(items)
        .filter((block) => !['var.get', 'var.declare', 'var.global'].includes(block.type))
        .map((block) => [block.type, varOf(block)]),
    ).toEqual([
      ['var.set', 's_label'],
      ['var.change', 's_count'],
    ]);
  });

  it('gives set the start value of its variable’s type', () => {
    const typed = (type: TypeJson) =>
      shadowOf(
        blocksOf(
          variablesContents(context({ ...game, all: [variable('s_x', 'x', 'd_guess', { type })] })),
        ).find((block) => block.type === 'var.set'),
        'VALUE',
      );
    expect(typed('int')).toMatchObject({ type: 'b2c.expr.num', value: '0' });
    expect(typed('double')).toMatchObject({ type: 'b2c.expr.num', value: '0.0' });
    expect(typed('bool')).toMatchObject({ type: 'b2c.expr.kw', value: 'true' });
    expect(typed('char')).toMatchObject({ type: 'b2c.expr.chr', value: 'a' });
    expect(typed('std::string')).toMatchObject({ type: 'b2c.expr.str', value: '' });
    // A list has no start value in M3 (slots.md §6), so its set block's value stays empty.
    expect(typed({ 'std::vector': 'int' })).toBeUndefined();
    expect(typed('error')).toBeUndefined();
  });

  it('lists a getter per variable of the module, grouped by declaring function and by name', () => {
    const zeta = variable('s_zeta', 'zeta', 'd_zeta');
    const alpha = variable('s_alpha', 'alpha', 'd_alpha');
    const counter = variable('s_i', 'i', 'd_alpha', { kind: 'loopVariable' });
    const w = symbolFixture('s_w', 'w', { kind: 'parameter', mode: 'copy', declBlock: 'fn_area' });
    const other = variable('s_other', 'other', 'd_other', { module: 'mod_other' });
    const area = symbolFixture('s_area', 'area', {
      kind: 'function',
      params: ['s_w'],
      returns: 'int',
      declBlock: 'fn_area',
    });
    const items = variablesContents(
      context({
        functions: ['fn_greet', 'fn_area'],
        declarations: { main: ['d_zeta', 'd_alpha'], fn_greet: ['d_count'] },
        all: [zeta, n, count, alpha, counter, w, other, area, greet],
      }),
    );
    expect(labelsOf(items)).toEqual(['In when program starts', 'In area', 'In greet']);
    // The getters follow their group's heading.
    const order = items.flatMap((item) =>
      item.kind === 'label' && 'text' in item
        ? [item.text]
        : item.kind === B2C_BLOCK_KIND && (item as B2cBlockInfo).type === 'var.get'
          ? [varOf(item as B2cBlockInfo)]
          : [],
    );
    expect(order).toEqual([
      'In when program starts',
      's_alpha',
      's_i',
      's_zeta',
      'In area',
      's_w',
      'In greet',
      's_count',
      's_n',
    ]);
  });

  it('puts variables declared outside main and the functions last, without a heading', () => {
    const items = variablesContents(
      context({
        declarations: { main: ['d_guess'] },
        all: [guess, variable('s_lost', 'lost', 'gone')],
      }),
    );
    expect(labelsOf(items)).toEqual(['In when program starts']);
    expect(
      blocksOf(items)
        .filter((block) => block.type === 'var.get')
        .map(varOf),
    ).toEqual(['s_guess', 's_lost']);
  });

  it(`lists at most ${String(MAX_LISTED_GETTERS)} getters, then says how many more`, () => {
    const ids = Array.from({ length: MAX_LISTED_GETTERS + 10 }, (_, index) => `d_${String(index)}`);
    const many = ids.map((id, index) => variable(`s_${String(index)}`, `v${String(index)}`, id));
    const items = variablesContents(context({ declarations: { main: ids }, all: many }));
    expect(typesOf(items).filter((type) => type === 'var.get')).toHaveLength(MAX_LISTED_GETTERS);
    expect(labelsOf(items).at(-1)).toBe(moreVariablesNote(10));
    expect(moreVariablesNote(10)).toBe('…and 10 more');
    // Exactly the cap: no note.
    const exact = variablesContents(
      context({
        declarations: { main: ids },
        all: many.slice(0, MAX_LISTED_GETTERS),
      }),
    );
    expect(labelsOf(exact)).toEqual(['In when program starts']);
  });

  it('builds equal items for an equal project, so an unchanged toolbox is not redrawn', () => {
    const ctx = context({ ...game, all: [guess, secret, label, n, count, greet] });
    expect(JSON.stringify(variablesContents(ctx))).toBe(JSON.stringify(variablesContents(ctx)));
    expect(JSON.stringify(programContents(ctx))).toBe(JSON.stringify(programContents(ctx)));
    expect(JSON.stringify(functionsContents(ctx))).toBe(JSON.stringify(functionsContents(ctx)));
    // The order in which the analysis lists the symbols does not matter.
    const reordered = context({ ...game, all: [greet, count, n, label, secret, guess] });
    expect(JSON.stringify(variablesContents(reordered))).toBe(
      JSON.stringify(variablesContents(ctx)),
    );
  });
});

describe('flip in the Variables category', () => {
  it('is offered while a true/false variable that may change exists, preset to it', () => {
    const flips = (all: readonly SymbolInfo[]) =>
      blocksOf(variablesContents(context({ declarations: { main: ['d_a', 'd_b'] }, all })))
        .filter((block) => block.type === 'var.flip')
        .map((block) => [varOf(block), block.retarget]);
    expect(flips([variable('s_a', 'a', 'd_a')])).toEqual([]);
    expect(flips([variable('s_a', 'a', 'd_a', { type: 'bool', isConst: true })])).toEqual([]);
    expect(
      flips([variable('s_a', 'a', 'd_a'), variable('s_b', 'b', 'd_b', { type: 'bool' })]),
    ).toEqual([['s_b', 'VAR']]);
  });
});

describe('the globals of the Variables category', () => {
  it('lists them first, under Everywhere, before main’s and each function’s variables', () => {
    const ctx = context({
      functions: ['fn_greet'],
      declarations: { main: ['d_a'] },
      all: [
        variable('s_a', 'a', 'd_a'),
        symbolFixture('s_n', 'n', { kind: 'parameter', mode: 'copy', declBlock: 'fn_greet' }),
        symbolFixture('s_greet', 'greet', {
          kind: 'function',
          params: ['s_n'],
          returns: 'void',
          declBlock: 'fn_greet',
        }),
        symbolFixture('s_total', 'total', { kind: 'global', declBlock: 'gl_total' }),
        symbolFixture('s_max', 'kMax', { kind: 'global', isConst: true, declBlock: 'gl_max' }),
      ],
    });
    expect(
      variableGroups(ctx.workspace, ctx.symbols, 'mod_main').map((group) => [
        group.owner.kind,
        group.symbols.map((symbol) => symbol.id),
      ]),
    ).toEqual([
      ['everywhere', ['s_max', 's_total']],
      ['main', ['s_a']],
      ['function', ['s_n']],
    ]);
    const items = variablesContents(ctx);
    expect(labelsOf(items)).toEqual(['Everywhere', 'In when program starts', 'In greet']);
    // set and change are preset to main's variable first; a global constant is never preset.
    expect(
      blocksOf(items)
        .filter((block) => block.type === 'var.set' || block.type === 'var.change')
        .map((block) => varOf(block)),
    ).toEqual(['s_a', 's_a']);
    // With main's variable gone, set takes the global variable, never the constant.
    const globals = context({
      all: [
        symbolFixture('s_total', 'total', { kind: 'global', declBlock: 'gl_total' }),
        symbolFixture('s_max', 'kMax', { kind: 'global', isConst: true, declBlock: 'gl_max' }),
      ],
    });
    expect(
      blocksOf(variablesContents(globals))
        .filter((block) => block.type === 'var.set')
        .map((block) => varOf(block)),
    ).toEqual(['s_total']);
  });
});

describe('the variable groups and the menus of blocks in the flyout', () => {
  it('group the module’s variables like the category, then list its functions', () => {
    const ctx = context({
      functions: ['fn_greet'],
      declarations: { main: ['d_b', 'd_a'] },
      all: [
        variable('s_b', 'b', 'd_b'),
        variable('s_a', 'a', 'd_a'),
        symbolFixture('s_n', 'n', { kind: 'parameter', mode: 'copy', declBlock: 'fn_greet' }),
        symbolFixture('s_greet', 'greet', {
          kind: 'function',
          params: ['s_n'],
          returns: 'void',
          declBlock: 'fn_greet',
        }),
      ],
    });
    expect(
      variableGroups(ctx.workspace, ctx.symbols, 'mod_main').map((group) => [
        group.owner.kind,
        group.symbols.map((symbol) => symbol.id),
      ]),
    ).toEqual([
      ['main', ['s_a', 's_b']],
      ['function', ['s_n']],
    ]);
    expect(toolboxSymbolGroups(ctx).map((group) => group.map((symbol) => symbol.id))).toEqual([
      ['s_a', 's_b'],
      ['s_n'],
      ['s_greet'],
    ]);
  });
});

describe('the Program category', () => {
  it('offers when program starts while the project has none', () => {
    const items = programContents(context({ main: false }));
    expect(typesOf(items)).toEqual([
      'program.main',
      'program.exit',
      'program.use_header',
      'program.check',
    ]);
    expect(labelsOf(items)).toEqual([]);
  });

  it('says where the program starts once the project has it, and still offers stop program', () => {
    const here = programContents(context());
    expect(typesOf(here)).toEqual(['program.exit', 'program.use_header', 'program.check']);
    expect(labelsOf(here)).toEqual([...PROGRAM_HERE_NOTE]);
    expect(PROGRAM_HERE_NOTE).toEqual(['Your program starts at', '“when program starts”.']);

    const elsewhere = programContents(
      context({
        main: false,
        programModule: 'mod_game',
        modules: [
          { id: 'mod_main', name: 'helpers' },
          { id: 'mod_game', name: 'game' },
        ],
      }),
    );
    expect(typesOf(elsewhere)).toEqual(['program.exit', 'program.use_header', 'program.check']);
    expect(labelsOf(elsewhere)).toEqual(['Your program starts in', 'the module “game”.']);
  });
});

describe('the Functions category and Your functions', () => {
  const n = symbolFixture('s_n', 'n', { kind: 'parameter', mode: 'copy', type: 'int' });
  const label = symbolFixture('s_label', 'label', {
    kind: 'parameter',
    mode: 'read_only',
    type: 'std::string',
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

  it('names a new function myFunction: a free name is chosen where it lands', () => {
    const defineName = (all: readonly SymbolInfo[]) =>
      blocksOf(functionsContents(context({ all })))[0]?.declares?.['NAME'];
    expect(defineName([])).toBe('myFunction');
    expect(
      defineName([
        symbolFixture('s_f', 'myFunction', { kind: 'function', params: [], returns: 'void' }),
      ]),
    ).toBe('myFunction');
  });

  it('shows a hint under Your functions while the module has no functions', () => {
    const items = functionsContents(context());
    expect(typesOf(items)).toEqual(['func.define', 'func.return']);
    expect(labelsOf(items)).toEqual([YOUR_FUNCTIONS_HEADING, ...NO_FUNCTIONS_HINT]);
    expect(YOUR_FUNCTIONS_HEADING).toBe('Your functions');
    expect(NO_FUNCTIONS_HINT).toEqual(['No functions yet.', 'Define one above.']);
    // The two lines read as one hint: a small gap between them, not the flyout's usual one.
    expect(items.slice(-3).map((item) => [item.kind, 'gap' in item ? item.gap : null])).toEqual([
      ['label', null],
      ['sep', 4],
      ['label', null],
    ]);
  });

  it('lists a call for every function under Your functions, with no module heading for one module', () => {
    const items = functionsContents(context({ all: [factorial, n, label] }));
    expect(labelsOf(items)).toEqual([YOUR_FUNCTIONS_HEADING]);
    const calls = blocksOf(items).slice(2);
    // A function that gives a value is a reporter.
    expect(calls.map((call) => call.type)).toEqual(['func.call']);
    const [factorialCall] = calls;
    expect(factorialCall?.fields).toEqual({ FUNC: { ref: 's_fact' } });
    expect(factorialCall?.extraState).toEqual({ argCount: 1 });
    expect(factorialCall?.argLabels).toEqual(['n:']);
    expect(shadowOf(factorialCall, 'ARG0')).toMatchObject({ value: '0', absent: false });
  });

  it('lists only the shown module’s functions, under its heading when there are two modules', () => {
    const twoModules = (active: string) =>
      functionsContents(
        context({
          all: [factorial, label, n, show],
          modules: [
            { id: 'mod_main', name: 'main' },
            { id: 'mod_extra', name: 'helpers' },
          ],
          active,
        }),
      );
    // Calls across modules are B2C-E0206 in M2, so the other module's functions are left out.
    const main = twoModules('mod_main');
    expect(labelsOf(main)).toEqual([YOUR_FUNCTIONS_HEADING, 'Module: main']);
    expect(
      blocksOf(main)
        .slice(2)
        .map((call): unknown => call.fields?.['FUNC']),
    ).toEqual([{ ref: 's_fact' }]);
    const extra = twoModules('mod_extra');
    expect(labelsOf(extra)).toEqual([YOUR_FUNCTIONS_HEADING, 'Module: helpers']);
    const [showCall, ...others] = blocksOf(extra).slice(2);
    expect(others).toEqual([]);
    // A void function is a statement.
    expect(showCall?.type).toBe('func.call_stmt');
    expect(showCall?.extraState).toEqual({ argCount: 2 });
    expect(showCall?.argLabels).toEqual(['label:', 'n:']);
    expect(shadowOf(showCall, 'ARG0')).toMatchObject({ type: 'b2c.expr.str', value: '' });
    // A module without functions shows its heading and the hint.
    const empty = functionsContents(
      context({
        all: [factorial, n],
        modules: [
          { id: 'mod_main', name: 'main' },
          { id: 'mod_extra', name: 'helpers' },
        ],
        active: 'mod_extra',
      }),
    );
    expect(labelsOf(empty)).toEqual([
      YOUR_FUNCTIONS_HEADING,
      'Module: helpers',
      ...NO_FUNCTIONS_HINT,
    ]);
    expect(typesOf(empty)).toEqual(['func.define', 'func.return']);
  });

  it('labels each of several functions with one name by its parameters (overloads)', () => {
    const w = symbolFixture('s_w', 'w', { kind: 'parameter', mode: 'copy', type: 'double' });
    const words = symbolFixture('s_words', 'words', {
      kind: 'parameter',
      mode: 'read_only',
      type: { 'std::vector': 'std::string' },
    });
    const overload = (id: string, params: string[]) =>
      symbolFixture(id, 'area', { kind: 'function', params, returns: 'void', type: 'void' });
    const items = functionsContents(
      context({
        all: [
          overload('s_a1', ['s_n', 's_n']),
          overload('s_a2', ['s_w']),
          overload('s_a3', ['s_words']),
          factorial,
          n,
          w,
          words,
        ],
      }),
    );
    // factorial has no other function of its name: no label.
    expect(labelsOf(items)).toEqual([
      YOUR_FUNCTIONS_HEADING,
      'area (int, int)',
      'area (double)',
      'area (list of string)',
    ]);
  });

  it('shows unknown parameters unlabelled', () => {
    const lost = symbolFixture('s_lost', 'lost', {
      kind: 'function',
      params: ['s_gone'],
      returns: 'double',
    });
    const items = functionsContents(context({ all: [lost, factorial, n] }));
    const lostCall = blocksOf(items).find((block) => block.fields?.['FUNC'] !== undefined);
    expect(lostCall?.fields).toEqual({ FUNC: { ref: 's_lost' } });
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

describe('the key of what the dynamic categories show', () => {
  const guess = variable('s_guess', 'guess', 'd_guess');
  const base = { declarations: { main: ['d_guess', 'd_other'] }, functions: ['fn_greet'] };
  const keyOf = (options: Parameters<typeof context>[0]) => dynamicContentsKey(context(options));

  it('is equal for an equal project, and changes with what the categories show', () => {
    const key = keyOf({ ...base, all: [guess] });
    expect(keyOf({ ...base, all: [guess] })).toBe(key);
    const changed = [
      // A new variable, a renamed one, another type, a constant, another declaring function.
      keyOf({ ...base, all: [guess, variable('s_other', 'other', 'd_other')] }),
      keyOf({ ...base, all: [{ ...guess, name: 'tries' }] }),
      keyOf({ ...base, all: [{ ...guess, type: 'double' }] }),
      keyOf({ ...base, all: [variable('s_guess', 'guess', 'd_guess', { isConst: true })] }),
      keyOf({ ...base, all: [{ ...guess, declBlock: 'fn_greet' }] }),
      // A function, the program's start, the modules.
      keyOf({
        ...base,
        all: [guess, symbolFixture('s_f', 'f', { kind: 'function', params: [], returns: 'void' })],
      }),
      keyOf({ ...base, main: false, all: [guess] }),
      keyOf({
        ...base,
        all: [guess],
        modules: [
          { id: 'mod_main', name: 'main' },
          { id: 'mod_b', name: 'b' },
        ],
      }),
    ];
    for (const [index, other] of changed.entries()) {
      expect(other, String(index)).not.toBe(key);
    }
  });

  it('follows each parent link of the canvas at most once, however far down main declares', () => {
    // A statement's parent is the statement before it: the k-th `create` in main is k links
    // below main. Asking each declaration for its root block followed about n²/2 links for n
    // variables (1.2 million, 11 ms per key, at the 5,000-block benchmark).
    const count = 200;
    const ids = Array.from({ length: count }, (_, index) => `d_${String(index)}`);
    const ctx = context({
      declarations: { main: ids },
      all: ids.map((id, index) => variable(`s_${String(index)}`, `v${String(index)}`, id)),
    });
    const links = countParentLinks(ctx.workspace);
    const key = dynamicContentsKey(ctx);
    expect(links()).toBeLessThanOrEqual(ctx.workspace.getAllBlocks(false).length);
    // Still every variable, in main's group.
    expect(
      variableGroups(ctx.workspace, ctx.symbols, 'mod_main').map((group) => [
        group.owner.kind,
        group.symbols.length,
      ]),
    ).toEqual([['main', count]]);
    expect(dynamicContentsKey(ctx)).toBe(key);
  });
});

describe('headings and labels', () => {
  /** Every label and button text the flyout shows, static and dynamic, in a varied project. */
  function shownTexts(names: readonly string[]): string[] {
    const functions = names.map((_, index) => `fn_${String(index)}`);
    const declared = names.map((_, index) => `d_${String(index)}`);
    const variables = names.map((name, index) =>
      variable(`s_v${String(index)}`, name, declared[index] ?? ''),
    );
    const definitions = names.map((name, index) =>
      symbolFixture(`s_f${String(index)}`, name, {
        kind: 'function',
        params: [],
        returns: 'void',
        declBlock: functions[index],
      }),
    );
    const params = names.map((name, index) =>
      symbolFixture(`s_p${String(index)}`, name, {
        kind: 'parameter',
        mode: 'copy',
        declBlock: functions[index],
      }),
    );
    const texts: string[] = [];
    for (const modules of [
      [{ id: 'mod_main', name: 'main' }],
      names.map((name, index) => ({ id: index === 0 ? 'mod_main' : `mod_${String(index)}`, name })),
    ]) {
      for (const programModule of [null, 'mod_main', 'mod_1']) {
        const ctx = context({
          functions,
          declarations: { main: declared },
          all: [...variables, ...definitions, ...params],
          modules,
          programModule,
        });
        for (const items of [
          ...TOOLBOX.map((category) => staticCategoryContents(category.id)),
          programContents(ctx),
          variablesContents(ctx),
          functionsContents(ctx),
          functionsContents(context({ modules })),
        ]) {
          texts.push(
            ...items.flatMap((item) =>
              (item.kind === 'label' || item.kind === 'button') && 'text' in item
                ? [item.text]
                : [],
            ),
          );
        }
      }
    }
    return texts;
  }

  it('never read like a category name, which the continuous toolbox would take for its place', () => {
    const categoryNames = TOOLBOX.map((category) => category.name);
    // Projects whose own names are category names, too.
    const texts = shownTexts([...categoryNames, 'main']);
    expect(texts).toContain(YOUR_FUNCTIONS_HEADING);
    for (const line of [...NO_FUNCTIONS_HINT, ...PROGRAM_HERE_NOTE]) {
      expect(texts).toContain(line);
    }
    expect(texts).toContain(MAKE_VARIABLE_TEXT);
    expect(texts).toContain('Module: Program');
    expect(texts).toContain('In Program');
    expect(texts).toContain('In when program starts');
    for (const text of texts) {
      expect(categoryNames, text).not.toContain(text);
    }
  });
});

describe('coverage of the catalog', () => {
  it('reaches every catalog block through one entry (getters and calls: one per symbol)', () => {
    // A true/false variable brings flip (w2.md §1.9).
    const variables = [variable('s_v', 'v', 'd_v'), variable('s_w', 'w', 'd_w', { type: 'bool' })];
    const value = symbolFixture('s_f', 'f', { kind: 'function', params: [], returns: 'int' });
    const action = symbolFixture('s_g', 'g', { kind: 'function', params: [], returns: 'void' });
    // Without main, so that Program offers it.
    const ctx = context({
      main: false,
      functions: ['fn'],
      declarations: { fn: ['d_v', 'd_w'] },
      all: [...variables, value, action],
    });
    const reached = [
      ...TOOLBOX.flatMap((category) =>
        category.id === 'program' || category.id === 'variables' || category.id === 'functions'
          ? []
          : typesOf(staticCategoryContents(category.id)),
      ),
      ...typesOf(programContents(ctx)),
      ...typesOf(variablesContents(ctx)),
      ...typesOf(functionsContents(ctx)),
    ];
    // M2's 31 blocks at least; W2's packages add theirs (docs/plans/m3-contracts/w2.md §3.2).
    expect(BLOCK_DEFS.length).toBeGreaterThanOrEqual(31);
    expect([...new Set(reached)].sort()).toEqual(BLOCK_DEFS.map((def) => def.id).sort());
    const count = (type: string) => reached.filter((reachedType) => reachedType === type).length;
    for (const def of BLOCK_DEFS) {
      const expected = def.id === 'var.get' ? variables.length : 1;
      expect(count(def.id), def.id).toBe(expected);
    }
  });
});

describe('newVariableBlock', () => {
  it('is the create block of the Variables category with the given name', () => {
    const block = newVariableBlock('score');
    expect(block.type).toBe('var.declare');
    expect(block.declares).toEqual({ NAME: 'score' });
    expect(block.fields?.['TYPE']).toBe('int');
    expect(shadowOf(block, 'VALUE')).toMatchObject({ value: '0', absent: false });
    const state = blockState(block);
    expect(state.fields?.['NAME']).toMatchObject({ name: 'score' });
    expect((state.fields?.['NAME'] as { sym: string }).sym).toMatch(GENERATED_ID_PATTERN.sym);
  });

  it('has the type asked for, with that type’s start value', () => {
    const cases: [string, string, string][] = [
      ['double', 'b2c.expr.num', '0.0'],
      ['bool', 'b2c.expr.kw', 'true'],
      ['char', 'b2c.expr.chr', 'a'],
      ['std::string', 'b2c.expr.str', ''],
    ];
    for (const [type, shadow, value] of cases) {
      const block = newVariableBlock('x', type);
      expect(block.fields?.['TYPE'], type).toBe(type);
      expect(shadowOf(block, 'VALUE'), type).toMatchObject({ type: shadow, value, absent: false });
    }
  });
});

describe('presets', () => {
  it('refuse a declaration on a field that does not declare', () => {
    expect(() => presetBlock('var.declare', { declare: { TYPE: 'x' } })).toThrow(PresetError);
    expect(() => presetBlock('var.declare', { declare: { NOPE: 'x' } })).toThrow(PresetError);
  });

  it('refuse a re-targeted field that is not a reference', () => {
    expect(() => presetBlock('var.set', { retarget: 'VALUE' })).toThrow(PresetError);
    expect(presetBlock('var.set', { retarget: 'VAR' }).retarget).toBe('VAR');
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
