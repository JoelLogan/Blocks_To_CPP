/**
 * The toolbox's flyout keeps a block from one showing to the next when its item is unchanged
 * (./inflater.ts, ./continuous.ts): showing the flyout again builds only the blocks whose items
 * changed, plus the blocks that declare something, however many blocks the toolbox has. A kept
 * block is the block its item would build, and a block the user changed in the flyout is built
 * again.
 */
import type { SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import { B2cSymbolDeclField, b2cDarkTheme } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { setCore } from '../../app/core';
import { resetAppStore, useAppStore } from '../../app/store';
import {
  B2cContinuousFlyout,
  B2cContinuousToolbox,
  CONTINUOUS_REFRESH_DELAY_MS,
} from './continuous';
import { declaresSymbols, type B2cBlockInflater, type FlyoutBlockStats } from './inflater';
import { createToolboxPlugin } from './plugin';
import { B2C_BLOCK_KIND, type B2cBlockInfo } from './presets';
import { toolboxInjectOptions } from './register';
import {
  FLYOUT_TEST_TIMEOUT_MS,
  disposeTestWorkspaces,
  editorContext,
  fakeCore,
  flyoutInflater,
  guessDocument,
  injectedWorkspace,
  loadModule,
  paddedToolbox,
  setUpToolboxBlocks,
  storeAnalysis,
  symbolFixture,
} from './testing';

// Each test here shows the flyout of every catalog block (FLYOUT_TEST_TIMEOUT_MS).
vi.setConfig({ testTimeout: FLYOUT_TEST_TIMEOUT_MS });

beforeAll(() => {
  setUpToolboxBlocks();
});

beforeEach(() => {
  resetAppStore();
});

const detachers: (() => void)[] = [];

/** Detaches the toolbox plugins and disposes of the workspaces made so far. */
function disposeAll(): void {
  for (const detach of detachers.splice(0)) {
    detach();
  }
  disposeTestWorkspaces();
}

afterEach(() => {
  disposeAll();
  setCore(null);
  resetAppStore();
  vi.restoreAllMocks();
});

/** Waits for Blockly's events, our refresh and the continuous toolbox's delayed rebuild. */
function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, CONTINUOUS_REFRESH_DELAY_MS + 50));
}

const guess = symbolFixture('s_guess', 'guess', { declBlock: 'decl' });
const n = symbolFixture('s_n', 'n', { kind: 'parameter', mode: 'copy', declBlock: 'fn' });
const factorial = symbolFixture('s_fact', 'factorial', {
  kind: 'function',
  params: ['s_n'],
  returns: 'int',
  declBlock: 'fn',
});
const label = symbolFixture('s_label', 'label', { declBlock: 'decl', type: 'std::string' });

/** An injected workspace with the guess document and the toolbox plugin attached. */
function attached(): Blockly.WorkspaceSvg {
  const workspace = injectedWorkspace('continuous');
  const document = guessDocument();
  loadModule(workspace, document);
  storeAnalysis(document, [factorial, guess, n]);
  const scopes = { decl: [factorial], print: [factorial, guess], fn: [factorial] };
  detachers.push(createToolboxPlugin().attach(editorContext(workspace, fakeCore(scopes))));
  return workspace;
}

/** Publishes a new analysis of the canvas with these symbols (what the preview pipeline does). */
function publishPreview(symbols: readonly SymbolInfo[]): void {
  useAppStore.setState((state) => {
    const preview = state.analysis.preview;
    if (preview === null) {
      throw new Error('no analysis to publish again');
    }
    return {
      analysis: {
        ...state.analysis,
        seq: state.analysis.seq + 1,
        preview: { ...preview, symbols: [...symbols] },
      },
    };
  });
}

/** Adds `create int count` below `print` in main, and its analysis (a new variable in main). */
function addCount(workspace: Blockly.WorkspaceSvg): void {
  const block = Blockly.serialization.blocks.append(
    {
      type: 'var.declare',
      id: 'countDecl',
      fields: { TYPE: 'int', NAME: { sym: 's_count', name: 'count' }, CONST: false },
    },
    workspace,
  );
  const below = workspace.getBlockById('print')?.nextConnection;
  if (below === undefined || block.previousConnection === null) {
    throw new Error('no place for count');
  }
  below.connect(block.previousConnection);
  publishPreview([
    factorial,
    guess,
    n,
    symbolFixture('s_count', 'count', { declBlock: 'countDecl' }),
  ]);
}

/** A block's state without coordinates or IDs (a value slot's shadow keeps its ID otherwise). */
function stateOf(block: Blockly.Block): unknown {
  const strip = (value: unknown): unknown => {
    if (Array.isArray(value)) {
      return value.map(strip);
    }
    if (typeof value === 'object' && value !== null) {
      return Object.fromEntries(
        Object.entries(value)
          .filter(([key]) => key !== 'id')
          .map(([key, inner]) => [key, strip(inner)]),
      );
    }
    return value;
  };
  return strip(Blockly.serialization.blocks.save(block, { addCoordinates: false }));
}

/** The continuous toolbox of a workspace. */
function continuousToolbox(workspace: Blockly.WorkspaceSvg): B2cContinuousToolbox {
  const toolbox = workspace.getToolbox();
  if (!(toolbox instanceof B2cContinuousToolbox)) {
    throw new Error('not the continuous toolbox');
  }
  return toolbox;
}

/** The inflater of the toolbox's blocks of a workspace's flyout. */
function inflaterOf(workspace: Blockly.WorkspaceSvg): B2cBlockInflater {
  const inflater = flyoutInflater(workspace);
  if (inflater === null) {
    throw new Error('the flyout has shown no toolbox block');
  }
  return inflater;
}

/** How the counts of `inflater` grew while `run` ran. */
async function statsOf(inflater: B2cBlockInflater, run: () => unknown): Promise<FlyoutBlockStats> {
  const before = inflater.stats();
  await run();
  const after = inflater.stats();
  return {
    created: after.created - before.created,
    reused: after.reused - before.reused,
    disposed: after.disposed - before.disposed,
  };
}

/** The blocks the flyout shows, in order. */
function flyoutBlocks(workspace: Blockly.WorkspaceSvg): Blockly.BlockSvg[] {
  return (workspace.getFlyout()?.getContents() ?? []).flatMap((item) => {
    const element = item.getElement();
    return element instanceof Blockly.BlockSvg ? [element] : [];
  });
}

/** The toolbox block items of a flyout's contents, in order (one per flyout block). */
function blockItems(contents: Blockly.utils.toolbox.FlyoutItemInfoArray): B2cBlockInfo[] {
  return contents.filter(
    (item): item is B2cBlockInfo => (item as { kind?: unknown }).kind === B2C_BLOCK_KIND,
  );
}

/** The items as the flyout shows them now, each with its block. */
function shownItems(workspace: Blockly.WorkspaceSvg): Shown[] {
  const items = blockItems(continuousToolbox(workspace).flyoutContents());
  const blocks = flyoutBlocks(workspace);
  expect(blocks).toHaveLength(items.length);
  return items.map((info, index) => {
    const block = blocks[index];
    if (block === undefined) {
      throw new Error('a toolbox item without its block');
    }
    return { info, block };
  });
}

/** A toolbox item as the flyout shows it, with its block. */
interface Shown {
  readonly info: B2cBlockInfo;
  readonly block: Blockly.BlockSvg;
}

/** Whether a shown item's block is built again on every showing (it declares something). */
function rebuilt(shown: Shown): boolean {
  return shown.info.declares !== undefined || declaresSymbols(shown.block);
}

/** The multiset of the keys of the shown items that may be kept. */
function keptKeys(items: readonly Shown[]): Map<string, number> {
  const keys = new Map<string, number>();
  for (const item of items) {
    if (!rebuilt(item)) {
      const key = JSON.stringify(item.info);
      keys.set(key, (keys.get(key) ?? 0) + 1);
    }
  }
  return keys;
}

/** How many of `of` are not in `from`, counted as multisets. */
function missingFrom(of: Map<string, number>, from: Map<string, number>): number {
  let missing = 0;
  for (const [key, count] of of) {
    missing += Math.max(0, count - (from.get(key) ?? 0));
  }
  return missing;
}

/** How many shown items are built again on every showing. */
function declaring(items: readonly Shown[]): number {
  return items.filter(rebuilt).length;
}

/**
 * The refresh after a new variable in main: the counts it gave, what it should have given (the
 * blocks of the new and the declaring items built, the blocks of the items gone and of the
 * declaring items before disposed of), and how many declaring items there were before and after.
 * Every block of an item shown before and after is the same.
 */
async function variableRefresh(workspace: Blockly.WorkspaceSvg): Promise<{
  stats: FlyoutBlockStats;
  expected: { created: number; disposed: number };
  declaring: { before: number; after: number };
}> {
  const before = shownItems(workspace);
  // Read before the refresh: a disposed block has no fields left.
  const keysBefore = keptKeys(before);
  const declaringBefore = declaring(before);
  const stats = await statsOf(inflaterOf(workspace), async () => {
    addCount(workspace);
    await settle();
    // On a busy machine the delayed refresh may not have run yet: show the flyout now if so.
    continuousToolbox(workspace).showAllCategories(false);
  });
  const after = shownItems(workspace);
  const keysAfter = keptKeys(after);
  // Every item kept has the block it had.
  const blocksBefore = new Map<string, Set<Blockly.BlockSvg>>();
  for (const { info, block } of before) {
    const key = JSON.stringify(info);
    blocksBefore.set(key, (blocksBefore.get(key) ?? new Set()).add(block));
  }
  const kept = new Map<string, number>();
  for (const item of after) {
    const key = JSON.stringify(item.info);
    if (!rebuilt(item) && blocksBefore.get(key)?.has(item.block) === true) {
      kept.set(key, (kept.get(key) ?? 0) + 1);
    }
  }
  for (const [key, count] of keysAfter) {
    expect(kept.get(key) ?? 0).toBe(Math.min(count, keysBefore.get(key) ?? 0));
  }
  return {
    stats,
    expected: {
      created: missingFrom(keysAfter, keysBefore) + declaring(after),
      disposed: missingFrom(keysBefore, keysAfter) + declaringBefore,
    },
    declaring: { before: declaringBefore, after: declaring(after) },
  };
}

describe('showing the flyout again', () => {
  it('builds only the changed and the declaring blocks, however many blocks it shows', async () => {
    const workspace = attached();
    await settle();
    const plain = await variableRefresh(workspace);
    expect(plain.stats.created).toBe(plain.expected.created);
    expect(plain.stats.disposed).toBe(plain.expected.disposed);
    // The getter, set and change of count, and the declaring blocks.
    expect(plain.stats.created).toBeLessThan(15);

    // The same refresh with every block of the fixed categories shown twice.
    const blocks = flyoutBlocks(workspace).length;
    disposeAll();
    const padded = attached();
    padded.updateToolbox(paddedToolbox(2));
    continuousToolbox(padded).showAllCategories();
    await settle();
    const shown = flyoutBlocks(padded).length;
    expect(shown).toBeGreaterThan(blocks + 30);
    const twice = await variableRefresh(padded);
    expect(twice.stats.created).toBe(twice.expected.created);
    expect(twice.stats.disposed).toBe(twice.expected.disposed);
    // As many blocks built and disposed of (the copies leave out the declaring blocks).
    expect(twice.declaring).toEqual(plain.declaring);
    expect(twice.stats.created).toBe(plain.stats.created);
    expect(twice.stats.disposed).toBe(plain.stats.disposed);
  });

  it('builds only the declaring blocks when nothing changed', async () => {
    const workspace = attached();
    await settle();
    const items = shownItems(workspace);
    const declarations = declaring(items);
    const stats = await statsOf(inflaterOf(workspace), () =>
      continuousToolbox(workspace).showAllCategories(),
    );
    expect(declarations).toBeGreaterThanOrEqual(6);
    expect(stats.created).toBe(declarations);
    expect(stats.disposed).toBe(declarations);
    expect(stats.reused).toBe(items.length - declarations);
  });

  it('builds at the start only what the toolbox plugin changed from the injected toolbox', async () => {
    const workspace = injectedWorkspace('continuous');
    const document = guessDocument();
    loadModule(workspace, document);
    storeAnalysis(document, [factorial, guess, n]);
    const injected = keptKeys(shownItems(workspace));
    const inflater = inflaterOf(workspace);
    const stats = await statsOf(inflater, async () => {
      const scopes = { decl: [factorial], print: [factorial, guess], fn: [factorial] };
      detachers.push(createToolboxPlugin().attach(editorContext(workspace, fakeCore(scopes))));
      await settle();
      continuousToolbox(workspace).showAllCategories(false);
    });
    const shown = shownItems(workspace);
    expect(stats.created).toBe(missingFrom(keptKeys(shown), injected) + declaring(shown));
    expect(stats.reused).toBeGreaterThan(30);
  });

  it('shows a kept block exactly as a block built from its item', async () => {
    const workspace = attached();
    await settle();
    const stats = await statsOf(inflaterOf(workspace), async () => {
      addCount(workspace);
      await settle();
    });
    expect(stats.reused).toBeGreaterThan(30);
    const looks = () =>
      shownItems(workspace).map(({ info, block }) => {
        // A declaration gets a new symbol each time.
        const state = JSON.stringify(stateOf(block)).replace(/"sym":"sym_\w+"/g, '"sym":"(new)"');
        return {
          type: info.type,
          state,
          path: block.pathObject.svgPath.getAttribute('d'),
          classes: block.getSvgRoot().getAttribute('class'),
        };
      });
    const kept = looks();
    inflaterOf(workspace).reuse = false;
    const fresh = await statsOf(inflaterOf(workspace), () =>
      continuousToolbox(workspace).showAllCategories(),
    );
    expect(fresh.reused).toBe(0);
    expect(looks()).toEqual(kept);
  });

  it('gives every declaring block new symbols on every showing', async () => {
    const workspace = attached();
    await settle();
    const declarations = () =>
      shownItems(workspace)
        .filter(rebuilt)
        .map(({ block }) => ({
          block,
          decls: block
            .getDescendants(false)
            .flatMap((part) => part.inputList.flatMap((input) => input.fieldRow))
            .flatMap((field) => (field instanceof B2cSymbolDeclField ? [field.getDecl()] : [])),
        }));
    const first = declarations();
    continuousToolbox(workspace).showAllCategories();
    const second = declarations();
    expect(second.map((entry) => entry.block.type)).toEqual(
      expect.arrayContaining([
        'var.declare',
        'control.for_range',
        'func.define',
        'error.try',
        'type.struct',
        'type.enum',
        'file.for_each_line',
      ]),
    );
    expect(second).toHaveLength(first.length);
    second.forEach((entry, index) => {
      const before = first[index];
      expect(entry.block).not.toBe(before?.block);
      expect(entry.decls.length).toBeGreaterThan(0);
      entry.decls.forEach((decl, at) => {
        expect(decl?.name).toBe(before?.decls[at]?.name);
        expect(decl?.sym).not.toBe(before?.decls[at]?.sym);
      });
    });
  });
  it('shows the structs and enums of the latest analysis as a block built from its item would', async () => {
    const workspace = attached();
    await settle();
    const looks = () =>
      shownItems(workspace).map(({ info, block }) => ({
        type: info.type,
        state: JSON.stringify(stateOf(block)).replace(/"sym":"sym_\w+"/g, '"sym":"(new)"'),
        text: block.toString(),
        path: block.pathObject.svgPath.getAttribute('d'),
      }));
    // A struct and an enum declared on the canvas, then the struct renamed: each analysis changes
    // the user types the type fields list, and every kept block must look as one built now.
    const struct = (name: string): SymbolInfo =>
      symbolFixture('s_pt', name, {
        kind: 'struct',
        fields: [],
        comparable: false,
        printable: false,
        type: { ref: 's_pt' },
      });
    const colour = symbolFixture('s_col', 'Colour', {
      kind: 'enum',
      values: [],
      type: { ref: 's_col' },
    });
    // Declared, then the struct renamed: every block not declaring anything is kept both times.
    for (const symbols of [
      [factorial, guess, n, struct('Point'), colour],
      [factorial, guess, n, struct('Place'), colour],
    ]) {
      const stats = await statsOf(inflaterOf(workspace), async () => {
        publishPreview(symbols);
        await settle();
        continuousToolbox(workspace).showAllCategories();
      });
      expect(stats.reused).toBeGreaterThan(30);
    }
    const kept = looks();
    expect(kept.map((look) => look.type)).toEqual(
      expect.arrayContaining(['type.new', 'type.enum_value', 'type.struct', 'type.enum']),
    );
    inflaterOf(workspace).reuse = false;
    continuousToolbox(workspace).showAllCategories();
    expect(looks()).toEqual(kept);
  });
});

describe('the flyout’s labels', () => {
  /** The labels the flyout shows, in order. */
  function labels(workspace: Blockly.WorkspaceSvg): Blockly.FlyoutButton[] {
    return (workspace.getFlyout()?.getContents() ?? []).flatMap((item) => {
      const element = item.getElement();
      return element instanceof Blockly.FlyoutButton && element.isLabel() ? [element] : [];
    });
  }

  it('are kept, and only a new one is drawn', async () => {
    const workspace = attached();
    await settle();
    const before = labels(workspace);
    expect(before.length).toBeGreaterThan(10);
    // A text variable in main beside guess: no new heading.
    publishPreview([factorial, guess, label, n]);
    await settle();
    continuousToolbox(workspace).showAllCategories(false);
    expect(labels(workspace)).toEqual(before);
    // No variable at all: the group headings go, the others stay.
    publishPreview([factorial]);
    await settle();
    continuousToolbox(workspace).showAllCategories(false);
    const after = labels(workspace);
    const gone = before.filter((element) => !after.includes(element));
    expect(gone.length).toBeGreaterThan(0);
    expect(after.every((element) => before.includes(element))).toBe(true);
    // Gone labels are disposed of: their elements left the flyout.
    for (const element of gone) {
      expect(element.getSvgRoot().isConnected).toBe(false);
    }
    // The categories still scroll to their headings.
    const flyout = workspace.getFlyout();
    if (!(flyout instanceof B2cContinuousFlyout)) {
      throw new Error('not the continuous flyout');
    }
    expect(flyout.getCategoryScrollPosition('Functions')).not.toBeNull();
  });
});

describe('a block changed in the flyout', () => {
  /** The first kept block with a number in a value slot, and that slot's shadow. */
  function numberSlot(workspace: Blockly.WorkspaceSvg): {
    block: Blockly.BlockSvg;
    shadow: Blockly.Block;
  } {
    for (const { info, block } of shownItems(workspace)) {
      if (rebuilt({ info, block })) {
        continue;
      }
      for (const input of block.inputList) {
        const shadow = input.connection?.targetBlock() ?? null;
        if (shadow?.isShadow() === true && /^\d+$/.test(String(shadow.getFieldValue('VALUE')))) {
          return { block, shadow };
        }
      }
    }
    throw new Error('no block with a number in the flyout');
  }

  /** The *set* block of the flyout. */
  function setBlock(workspace: Blockly.WorkspaceSvg): Blockly.BlockSvg {
    const block = flyoutBlocks(workspace).find((candidate) => candidate.type === 'var.set');
    if (block === undefined) {
      throw new Error('no set in the flyout');
    }
    return block;
  }

  it('is built again from its item when a number was typed into it', async () => {
    const workspace = attached();
    await settle();
    const { block, shadow } = numberSlot(workspace);
    const value: unknown = shadow.getFieldValue('VALUE');
    const type = block.type;
    shadow.setFieldValue('987', 'VALUE');
    await settle();
    continuousToolbox(workspace).showAllCategories();
    expect(block.isDeadOrDying()).toBe(true);
    const again = numberSlot(workspace);
    expect(again.block.type).toBe(type);
    expect(again.shadow.getFieldValue('VALUE')).toBe(value);
  });

  it('is built again when its variable was changed, and its start value with it', async () => {
    const workspace = attached();
    await settle();
    publishPreview([factorial, guess, label, n]);
    await settle();
    const set = setBlock(workspace);
    const state = () => JSON.stringify(stateOf(setBlock(workspace)));
    const preset = state();
    set.setFieldValue('s_label', 'VAR');
    await settle();
    // The start value followed the variable's type there.
    expect(state()).not.toBe(preset);
    continuousToolbox(workspace).showAllCategories();
    expect(set.isDeadOrDying()).toBe(true);
    expect(state()).toBe(preset);
  });

  it('is built again when the flyout is shown before Blockly delivered the change', async () => {
    const workspace = attached();
    await settle();
    const { block, shadow } = numberSlot(workspace);
    const delivered = vi.fn();
    workspace
      .getFlyout()
      ?.getWorkspace()
      .addChangeListener((event) => {
        if (event instanceof Blockly.Events.BlockChange) {
          delivered();
        }
      });
    // The edit and the showing in the same task: no change event has reached anyone yet.
    shadow.setFieldValue('987', 'VALUE');
    continuousToolbox(workspace).showAllCategories();
    expect(delivered).not.toHaveBeenCalled();
    expect(block.isDeadOrDying()).toBe(true);
    expect(numberSlot(workspace).shadow.getFieldValue('VALUE')).not.toBe('987');
  });
});

describe('a block taken out of the flyout', () => {
  it('gets IDs of its own, which the kept flyout block does not share', async () => {
    const workspace = attached();
    await settle();
    const flyout = workspace.getFlyout();
    if (!(flyout instanceof B2cContinuousFlyout)) {
      throw new Error('not the continuous flyout');
    }
    // A block with a block in it: print's item holds a text shadow.
    const original = flyoutBlocks(workspace).find((block) => block.type === 'io.print');
    if (original === undefined) {
      throw new Error('no print block in the flyout');
    }
    const flyoutIds = new Set(original.getDescendants(false).map((block) => block.id));
    const created = flyout.createBlock(original);
    const createdIds = created.getDescendants(false).map((block) => block.id);
    expect(createdIds.length).toBe(flyoutIds.size);
    expect(createdIds.filter((id) => flyoutIds.has(id))).toEqual([]);
    // The canvas changed: the next showing keeps the print block, IDs and all.
    await settle();
    continuousToolbox(workspace).showAllCategories();
    expect(flyoutBlocks(workspace)).toContain(original);
    for (const id of flyoutIds) {
      expect(workspace.getBlockById(id)).toBeNull();
    }
  });
});

describe('a kept block', () => {
  it('has its pointer listeners once, however often it was kept', async () => {
    const workspace = attached();
    await settle();
    const toolbox = continuousToolbox(workspace);
    for (let showing = 0; showing < 3; showing += 1) {
      toolbox.showAllCategories();
    }
    const listeners = (inflaterOf(workspace) as unknown as { listeners: Map<string, unknown> })
      .listeners;
    expect(listeners.size).toBe(flyoutBlocks(workspace).length);
    const block = shownItems(workspace).find((item) => !rebuilt(item))?.block;
    if (block === undefined) {
      throw new Error('no kept block');
    }
    const start = vi
      .spyOn(Blockly.Gesture.prototype, 'handleFlyoutStart')
      .mockImplementation(() => undefined);
    block
      .getSvgRoot()
      .dispatchEvent(
        new PointerEvent('pointerdown', { bubbles: true, pointerId: 1, isPrimary: true }),
      );
    expect(start).toHaveBeenCalledTimes(1);
  });

  it('is no longer highlighted by a pointer that was over it', async () => {
    const workspace = attached();
    await settle();
    const block = shownItems(workspace).find((item) => !rebuilt(item))?.block;
    if (block === undefined) {
      throw new Error('no kept block');
    }
    block.addSelect();
    expect(block.getSvgRoot().classList.contains('blocklySelected')).toBe(true);
    continuousToolbox(workspace).showAllCategories();
    expect(flyoutBlocks(workspace)).toContain(block);
    expect(block.getSvgRoot().classList.contains('blocklySelected')).toBe(false);
  });

  it('keeps the keyboard focus, on the same element', async () => {
    const workspace = attached();
    await settle();
    const block = shownItems(workspace).find((item) => !rebuilt(item))?.block;
    if (block === undefined) {
      throw new Error('no kept block');
    }
    const manager = Blockly.getFocusManager();
    manager.focusNode(block);
    const element = block.getFocusableElement();
    expect(document.activeElement).toBe(element);
    continuousToolbox(workspace).showAllCategories();
    expect(manager.getFocusedNode()).toBe(block);
    expect(document.activeElement).toBe(element);
  });

  it('takes the colours of a new theme', async () => {
    const workspace = attached();
    await settle();
    const block = shownItems(workspace).find(
      (item) => !rebuilt(item) && item.info.type === 'io.print',
    )?.block;
    if (block === undefined) {
      throw new Error('no print block');
    }
    const light = block.getColour();
    workspace.setTheme(b2cDarkTheme);
    expect(block.getColour()).not.toBe(light);
    continuousToolbox(workspace).showAllCategories();
    expect(flyoutBlocks(workspace)).toContain(block);
    const dark = block.getColour();
    inflaterOf(workspace).reuse = false;
    continuousToolbox(workspace).showAllCategories();
    const fresh = flyoutBlocks(workspace).find((candidate) => candidate.type === 'io.print');
    expect(fresh).not.toBe(block);
    expect(fresh?.getColour()).toBe(dark);
  });
});

describe('the events of a showing', () => {
  it('include no block created, deleted or moved in the flyout, but a change made there', async () => {
    const workspace = attached();
    await settle();
    const flyoutWorkspace = workspace.getFlyout()?.getWorkspace();
    if (flyoutWorkspace === undefined) {
      throw new Error('no flyout');
    }
    const seen: string[] = [];
    flyoutWorkspace.addChangeListener((event) => {
      seen.push(event.type);
    });
    addCount(workspace);
    await settle();
    continuousToolbox(workspace).showAllCategories();
    await settle();
    expect(seen).not.toContain(Blockly.Events.BLOCK_CREATE);
    expect(seen).not.toContain(Blockly.Events.BLOCK_DELETE);
    expect(seen).not.toContain(Blockly.Events.BLOCK_MOVE);
    // An edit in the flyout still reaches its listeners (start values follow it, ./reshape.ts).
    const set = flyoutBlocks(workspace).find((block) => block.type === 'var.set');
    set?.setFieldValue('s_n', 'VAR');
    await settle();
    expect(seen).toContain(Blockly.Events.BLOCK_CHANGE);
    expect(Blockly.Events.isEnabled()).toBe(true);
  });
});

describe('a showing of the displayed flyout', () => {
  it('keeps it displayed, and leaves the drag targets to the next frame', async () => {
    const workspace = attached();
    await settle();
    const flyout = workspace.getFlyout();
    if (flyout === null) {
      throw new Error('no flyout');
    }
    // The flyout's own SVG element, whose display Blockly sets.
    const group = flyout.getWorkspace().getParentSvg();
    expect(group.classList.contains('blocklyFlyout')).toBe(true);
    const displays: string[] = [];
    const observer = new MutationObserver(() => {
      displays.push(group.style.display);
    });
    observer.observe(group, { attributes: true, attributeFilter: ['style'] });
    addCount(workspace);
    await settle();
    const record = vi.spyOn(workspace, 'recordDragTargets');
    publishPreview([factorial, guess, label, n]);
    continuousToolbox(workspace).showAllCategories();
    observer.disconnect();
    expect(displays).not.toContain('none');
    expect(group.style.display).not.toBe('none');
    expect(record).not.toHaveBeenCalled();
    // Recorded again in the next frame (the layout marked them stale).
    await settle();
    expect(record).toHaveBeenCalledTimes(1);
  });
});

describe('the canvas’s block limits', () => {
  it('cost a block added to the canvas no walk of the flyout while there are none', async () => {
    const workspace = attached();
    await settle();
    const walks = vi.spyOn(workspace.getFlyout()?.getWorkspace() ?? workspace, 'getTopBlocks');
    const block = workspace.newBlock('io.print');
    block.initSvg();
    block.render();
    await settle();
    expect(walks).not.toHaveBeenCalled();
  });

  it('still disable a flyout block once the canvas has as many as it may', async () => {
    const host = document.createElement('div');
    document.body.append(host);
    const options = toolboxInjectOptions();
    const workspace = Blockly.inject(host, {
      renderer: 'zelos',
      sounds: false,
      toolbox: options.toolbox,
      plugins: { ...options.plugins },
      maxInstances: { 'io.print': 1 },
    });
    try {
      const print = () => flyoutBlocks(workspace).find((block) => block.type === 'io.print');
      expect(print()?.isEnabled()).toBe(true);
      const block = workspace.newBlock('io.print');
      block.initSvg();
      block.render();
      await settle();
      expect(print()?.isEnabled()).toBe(false);
      // Also after the flyout is shown again, with the block kept.
      const toolbox = continuousToolbox(workspace);
      toolbox.showAllCategories();
      expect(print()?.isEnabled()).toBe(false);
      block.dispose(false);
      await settle();
      expect(print()?.isEnabled()).toBe(true);
    } finally {
      workspace.dispose();
    }
  });
});

describe('the time a showing takes', () => {
  // These measure many showings, several of every block built again: with W2's structs, enums,
  // control and file blocks, one try of the padded test took 27.5 s and of the other 16.5 s without
  // coverage on a loaded machine (load 3.7 on 4 cores), over the file's flyout limit.
  const TIMING_TEST_TIMEOUT_MS = 60_000;

  /** The median of `count` runs of `run`, in milliseconds. */
  function median(count: number, run: (index: number) => void): number {
    const times: number[] = [];
    for (let index = 0; index < count; index += 1) {
      const start = performance.now();
      run(index);
      times.push(performance.now() - start);
    }
    times.sort((a, b) => a - b);
    return times[Math.floor(times.length / 2)] ?? Number.NaN;
  }

  /** The median time of a showing after a variable came or went, on `workspace`. */
  function refreshTime(workspace: Blockly.WorkspaceSvg, count: number): number {
    const toolbox = continuousToolbox(workspace);
    return median(count, (index) => {
      publishPreview(index % 2 === 0 ? [factorial, guess, label, n] : [factorial, guess, n]);
      toolbox.showAllCategories(false);
    });
  }

  it(
    'is a fraction of building every block',
    { retry: 2, timeout: TIMING_TEST_TIMEOUT_MS },
    async () => {
      const workspace = attached();
      await settle();
      const kept = refreshTime(workspace, 5);
      inflaterOf(workspace).reuse = false;
      const rebuilt = refreshTime(workspace, 3);
      expect(kept).toBeLessThanOrEqual(0.35 * rebuilt);
    },
  );

  // A showing still lays out and checks every kept block, so its time grows with the blocks shown:
  // with every fixed category shown twice it came out 1.7 to 2.0 times as long on a loaded machine,
  // whichever happy-dom. What must stay small is what each block more costs against building one;
  // that no block is built again however many there are is counted exactly above.
  it(
    'costs a fraction of building a block for each block more it shows',
    { retry: 2, timeout: TIMING_TEST_TIMEOUT_MS },
    async () => {
      const workspace = attached();
      await settle();
      const blocks = flyoutBlocks(workspace).length;
      const kept = refreshTime(workspace, 5);
      inflaterOf(workspace).reuse = false;
      const rebuilt = refreshTime(workspace, 3);
      // Every fixed category shown twice: about 50 more blocks and 10 more labels.
      disposeAll();
      const padded = attached();
      padded.updateToolbox(paddedToolbox(2));
      continuousToolbox(padded).showAllCategories();
      await settle();
      const more = flyoutBlocks(padded).length - blocks;
      expect(more).toBeGreaterThan(30);
      const perBlockMore = (refreshTime(padded, 5) - kept) / more;
      expect(perBlockMore).toBeLessThanOrEqual((0.35 * rebuilt) / blocks);
    },
  );
});
