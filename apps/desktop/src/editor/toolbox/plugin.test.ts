import type { BdmDocument, SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import {
  B2cSymbolDeclField,
  B2cSymbolRefField,
  DEFAULT_EDITOR_SERVICES,
  toolboxSymbolGroups,
} from '@blocks2cpp/blockly-ext';
import { act, fireEvent, render, screen } from '@testing-library/react';
import * as Blockly from 'blockly/core';
import { createElement } from 'react';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { setCore } from '../../app/core';
import {
  DialogHost,
  createDialogQueue,
  type ChoiceOptions,
  type DialogService,
} from '../../app/dialogs';
import { resetAppStore, useAppStore } from '../../app/store';
import { documentFixture } from '../../app/testing/fixtures';
import { latestTimings, resetTimings } from '../../lib/timing';
import { createSymbolServices, installEditorServices, servicesSessionHooks } from '../services';
import { ICON_CLASS, CATEGORY_CLASS } from './category';
import {
  B2cContinuousFlyout,
  B2cContinuousToolbox,
  CONTINUOUS_REFRESH_DELAY_MS,
} from './continuous';
import {
  MAKE_VARIABLE_BUTTON,
  MY_BLOCKS_CATEGORY,
  PROGRAM_CATEGORY,
  PROGRAM_HERE_NOTE,
  VARIABLES_CATEGORY,
} from './contents';
import { argLabelField } from './inflater';
import type { MakeVariableResult } from './makeVariable';
import { analysisSymbolSource, createToolboxPlugin } from './plugin';
import {
  disposeTestWorkspaces,
  editorContext,
  fakeCore,
  guessDocument,
  injectedWorkspace,
  loadModule,
  setUpToolboxBlocks,
  storeAnalysis,
  symbolFixture,
} from './testing';

beforeAll(() => {
  setUpToolboxBlocks();
});

beforeEach(() => {
  resetAppStore();
});

const detachers: (() => void)[] = [];

afterEach(() => {
  for (const detach of detachers.splice(0)) {
    detach();
  }
  setCore(null);
  disposeTestWorkspaces();
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

/** Scope answers for guessDocument(): guess is visible below its declaration, n in factorial. */
const SCOPES: Record<string, readonly SymbolInfo[]> = {
  decl: [factorial],
  print: [factorial, guess],
  'main/BODY': [factorial],
  fn: [factorial],
  'fn/BODY': [factorial, n],
};

/** Dialogs for Make a variable: the name `name`, then the first type offered (int). */
function makeDialogs(name: Promise<string | null> | (() => Promise<string | null>)) {
  return {
    prompt: vi.fn<DialogService['prompt']>(typeof name === 'function' ? name : () => name),
    alert: vi.fn<DialogService['alert']>(() => Promise.resolve()),
    choose: firstChoice(),
  };
}

/**
 * A spy for {@link DialogService.choose} answering with the first choice. (A spy's call signature
 * is not generic, so it is cast to the service's.)
 */
function firstChoice() {
  const spy = vi.fn((options: ChoiceOptions<string>) =>
    Promise.resolve(options.choices[0]?.id ?? options.cancel),
  );
  return spy as typeof spy & DialogService['choose'];
}

/** An injected workspace with the guess document and the plugin attached. */
function attached(
  toolbox: 'continuous' | 'category',
  options: Parameters<typeof createToolboxPlugin>[0] = {},
): Blockly.WorkspaceSvg {
  const workspace = injectedWorkspace(toolbox);
  const document = guessDocument();
  loadModule(workspace, document);
  storeAnalysis(document, [factorial, guess, n]);
  detachers.push(createToolboxPlugin(options).attach(editorContext(workspace, fakeCore(SCOPES))));
  return workspace;
}

/**
 * Publishes a new analysis of the canvas (what the preview pipeline does after each change), with
 * the same symbols unless others are given.
 */
function publishPreview(symbols?: readonly SymbolInfo[]): void {
  useAppStore.setState((state) => {
    const preview = state.analysis.preview;
    if (preview === null) {
      throw new Error('no analysis to publish again');
    }
    return {
      analysis: {
        ...state.analysis,
        seq: state.analysis.seq + 1,
        preview: { ...preview, symbols: [...(symbols ?? preview.symbols)] },
      },
    };
  });
}

/** The analysis with a new variable `count`, declared by a new `create` block in main. */
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

/** The blocks the flyout shows, in order. */
function flyoutBlocks(workspace: Blockly.WorkspaceSvg): Blockly.BlockSvg[] {
  const flyout = workspace.getToolbox()?.getFlyout();
  return (flyout?.getContents() ?? []).flatMap((item) => {
    const element = item.getElement();
    return element instanceof Blockly.BlockSvg ? [element] : [];
  });
}

/** The labels and buttons the flyout shows, in order. */
function flyoutTexts(workspace: Blockly.WorkspaceSvg): string[] {
  const flyout = workspace.getToolbox()?.getFlyout();
  return (flyout?.getContents() ?? []).flatMap((item) => {
    const element = item.getElement();
    return element instanceof Blockly.FlyoutButton ? [element.getButtonText()] : [];
  });
}

/** The buttons the flyout shows, in order. */
function flyoutButtons(workspace: Blockly.WorkspaceSvg): Blockly.FlyoutButton[] {
  const flyout = workspace.getToolbox()?.getFlyout();
  return (flyout?.getContents() ?? []).flatMap((item) => {
    const element = item.getElement();
    return element instanceof Blockly.FlyoutButton && !element.isLabel() ? [element] : [];
  });
}

/** Selects a block of the canvas the way Blockly 12 does: by focusing it. */
function select(workspace: Blockly.WorkspaceSvg, id: string): void {
  const block = workspace.getBlockById(id);
  if (!(block instanceof Blockly.BlockSvg)) {
    throw new Error(`no block ${id}`);
  }
  Blockly.common.setSelected(block);
}

/** The symbols the getters in the flyout refer to. */
function getterRefs(workspace: Blockly.WorkspaceSvg): unknown[] {
  return flyoutBlocks(workspace)
    .filter((block) => block.type === 'var.get')
    .map((block): unknown => block.getFieldValue('VAR'));
}

/** The toolbox's category of a name. */
function category(workspace: Blockly.WorkspaceSvg, name: string): Blockly.IToolboxItem {
  const item = (workspace.getToolbox() as Blockly.Toolbox | null)
    ?.getToolboxItems()
    .find(
      (candidate) => candidate instanceof Blockly.ToolboxCategory && candidate.getName() === name,
    );
  if (item === undefined) {
    throw new Error(`no category ${name}`);
  }
  return item;
}

/** The continuous toolbox of a workspace. */
function continuousToolbox(workspace: Blockly.WorkspaceSvg): B2cContinuousToolbox {
  const toolbox = workspace.getToolbox();
  if (!(toolbox instanceof B2cContinuousToolbox)) {
    throw new Error('not the continuous toolbox');
  }
  return toolbox;
}

describe('the toolbox plugin with the continuous toolbox', () => {
  it('shows every category in one flyout, with the dynamic ones built from the analysis', async () => {
    const workspace = attached('continuous');
    await settle();

    const types = flyoutBlocks(workspace).map((block) => block.type);
    expect(types).toContain('io.ask');
    // The canvas has main: Program says where the program starts instead of offering another.
    expect(types).not.toContain('program.main');
    expect(flyoutTexts(workspace)).toEqual(expect.arrayContaining([...PROGRAM_HERE_NOTE]));
    // Every variable of the module, grouped by the function that declares it.
    expect(getterRefs(workspace)).toEqual(['s_guess', 's_n']);
    expect(flyoutTexts(workspace)).toEqual(
      expect.arrayContaining(['In when program starts', 'In factorial']),
    );
    // One set and one change (its menu gives every update operator), for main's guess.
    expect(
      flyoutBlocks(workspace)
        .filter((block) => ['var.set', 'var.change', 'var.update'].includes(block.type))
        .map((block): unknown => [block.type, block.getFieldValue('VAR')]),
    ).toEqual([
      ['var.set', 's_guess'],
      ['var.change', 's_guess'],
    ]);
    // Your functions: factorial gives a value, so it is a reporter, with its argument labelled.
    const call = flyoutBlocks(workspace).find((block) => block.type === 'func.call');
    expect(call?.getFieldValue('FUNC')).toBe('s_fact');
    expect(call?.getField(argLabelField(0))?.getText()).toBe('n:');
  });

  it('never shows its flyout again when the selection changes', async () => {
    const workspace = attached('continuous');
    await settle();
    const toolbox = continuousToolbox(workspace);
    const show = vi.spyOn(toolbox.getFlyout(), 'show');
    const build = vi.spyOn(toolbox, 'flyoutContents');
    for (const id of ['fn', 'print', 'decl', 'main']) {
      select(workspace, id);
      await settle();
    }
    // The canvas background, and the toolbox.
    Blockly.getFocusManager().focusNode(workspace);
    await settle();
    Blockly.getFocusManager().focusTree(toolbox);
    await settle();
    expect(show).not.toHaveBeenCalled();
    expect(build).not.toHaveBeenCalled();
    expect(getterRefs(workspace)).toEqual(['s_guess', 's_n']);
  });

  it('shows its flyout again once a variable is added, and once only', async () => {
    const workspace = attached('continuous');
    await settle();
    const show = vi.spyOn(continuousToolbox(workspace).getFlyout(), 'show');
    addCount(workspace);
    await settle();
    expect(show).toHaveBeenCalledTimes(1);
    expect(getterRefs(workspace)).toEqual(['s_count', 's_guess', 's_n']);
  });

  it('builds nothing after an edit or an analysis that changes nothing it shows', async () => {
    const workspace = attached('continuous');
    await settle();
    const toolbox = continuousToolbox(workspace);
    const show = vi.spyOn(toolbox.getFlyout(), 'show');
    const build = vi.spyOn(toolbox, 'flyoutContents');
    // A block moved, a field changed (Blockly asks the toolbox to refresh), and their analysis.
    workspace.getBlockById('fn')?.moveBy(10, 10);
    workspace.getBlockById('print')?.setFieldValue(false, 'NEWLINE');
    await settle();
    publishPreview();
    await settle();
    expect(build).not.toHaveBeenCalled();
    expect(show).not.toHaveBeenCalled();
    // Forcing always redraws.
    expect(toolbox.showAllCategories(true)).toBe(true);
    expect(toolbox.showAllCategories(false)).toBe(false);
  });

  it('rebuilds after each analysis that changes what it shows', async () => {
    const workspace = attached('continuous');
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_guess', 's_n']);
    useAppStore.setState((state) => ({
      analysis: { ...state.analysis, seq: 2, preview: null },
    }));
    await settle();
    // No analysis: no variable is known.
    expect(getterRefs(workspace)).toEqual([]);
  });

  it('offers when program starts again once main is deleted', async () => {
    const workspace = attached('continuous');
    await settle();
    expect(flyoutBlocks(workspace).map((block) => block.type)).not.toContain('program.main');
    workspace.getBlockById('main')?.dispose(false);
    publishPreview([factorial, n]);
    await settle();
    expect(flyoutBlocks(workspace).map((block) => block.type)).toContain('program.main');
    expect(flyoutTexts(workspace)).not.toContain(PROGRAM_HERE_NOTE[0]);
  });

  it('lists the functions of the module shown, and follows another module being shown', async () => {
    const workspace = attached('continuous');
    await settle();
    const calls = () =>
      flyoutBlocks(workspace)
        .filter((block) => block.type === 'func.call')
        .map((block): unknown => block.getFieldValue('FUNC'));
    expect(calls()).toEqual(['s_fact']);
    // factorial belongs to mod_main: once another module is shown, it is not offered there.
    act(() => {
      useAppStore.getState().actions.updateProject({ activeModuleId: 'mod_other' });
    });
    await settle();
    expect(calls()).toEqual([]);
  });

  it('draws category bubbles with the catalog icon, coloured from the theme', () => {
    const workspace = attached('continuous');
    const program = category(workspace, 'Program') as Blockly.ToolboxCategory;
    const div = program.getDiv();
    expect(div?.classList.contains(CATEGORY_CLASS)).toBe(true);
    const icon = div?.querySelector(`.${ICON_CLASS}`) as HTMLElement | null;
    expect(icon?.textContent).toBe('▶');
    expect(icon?.getAttribute('aria-hidden')).toBe('true');
    expect(icon?.style.backgroundColor).not.toBe('');
    // The name is the accessible label of the category row.
    expect(div?.textContent).toContain('Program');
  });

  it('gives the flyout new presets on each showing (no block recycling)', async () => {
    const workspace = attached('continuous');
    await settle();
    const nameOf = () =>
      (
        flyoutBlocks(workspace)
          .find((block) => block.type === 'var.declare')
          ?.getField('NAME') as B2cSymbolDeclField | undefined
      )?.getDecl();
    const first = nameOf();
    continuousToolbox(workspace).showAllCategories();
    const second = nameOf();
    expect(first?.name).toBe('value');
    expect(second?.name).toBe('value');
    expect(second?.sym).not.toBe(first?.sym);
  });

  it('gives a block in the flyout every variable of the module in its menu, grouped', async () => {
    const workspace = attached('continuous');
    await settle();
    const menuOf = (type: string): unknown[] => {
      const block = flyoutBlocks(workspace).find((candidate) => candidate.type === type);
      const field = block?.getField('VAR');
      if (!(field instanceof B2cSymbolRefField)) {
        throw new Error(`no ${type} in the flyout`);
      }
      return field.menuOptions();
    };
    const between = Blockly.FieldDropdown.SEPARATOR;
    expect(menuOf('var.get')).toEqual([['guess', 's_guess'], between, ['n', 's_n']]);
    expect(menuOf('var.set')).toEqual([['guess', 's_guess'], between, ['n', 's_n']]);
    // Once the toolbox is gone, the flyout's menus have nothing of it.
    for (const detach of detachers.splice(0)) {
      detach();
    }
    expect(toolboxSymbolGroups()).toBeNull();
  });
});

/** The flyout's scroll offset, in pixels. */
function flyoutScroll(flyout: B2cContinuousFlyout): number {
  return -flyout.getWorkspace().scrollY;
}

/** Where a category's heading is on screen, relative to the top of the flyout's view, in pixels. */
function headingTop(flyout: B2cContinuousFlyout, name: string): number {
  const position = flyout.getCategoryScrollPosition(name);
  if (position === null) {
    throw new Error(`no heading of ${name}`);
  }
  return position * flyout.getWorkspace().scale - flyoutScroll(flyout);
}

/** Lets the flyout's scroll (and the plugin's frame after it) finish. */
function frames(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 100));
}

describe('the flyout shown again with new contents', () => {
  /** The guess document on a laid-out canvas, its flyout scrolled so that `name` is at the top. */
  async function scrolledTo(name: string): Promise<{
    workspace: Blockly.WorkspaceSvg;
    flyout: B2cContinuousFlyout;
  }> {
    const workspace = attached('continuous');
    workspace.setCachedParentSvgSize(1000, 500);
    workspace.resize();
    await settle();
    const flyout = continuousToolbox(workspace).getFlyout();
    if (!(flyout instanceof B2cContinuousFlyout)) {
      throw new Error('not the continuous flyout');
    }
    const position = flyout.getCategoryScrollPosition(name) ?? 0;
    flyout.getWorkspace().scrollbar?.setY((position + 10) * flyout.getWorkspace().scale);
    await frames();
    return { workspace, flyout };
  }

  it('keeps the category at the top of its view where it was (Math, after a new variable)', async () => {
    const { workspace, flyout } = await scrolledTo('Math');
    const before = headingTop(flyout, 'Math');
    expect(flyoutScroll(flyout)).toBeGreaterThan(0);
    expect(before).toBeLessThan(0);
    const scrollBefore = flyoutScroll(flyout);
    const positionBefore = flyout.getCategoryScrollPosition('Math') ?? 0;
    addCount(workspace);
    await settle();
    await frames();
    // Variables, above Math, grew by a getter: the flyout scrolled with it.
    expect(flyout.getCategoryScrollPosition('Math') ?? 0).toBeGreaterThan(positionBefore);
    expect(flyoutScroll(flyout)).toBeGreaterThan(scrollBefore);
    expect(headingTop(flyout, 'Math')).toBeCloseTo(before, 6);
  });

  it('keeps the selected category selected', async () => {
    const { workspace, flyout } = await scrolledTo('Math');
    const toolbox = continuousToolbox(workspace);
    toolbox.selectCategoryByName('Functions');
    addCount(workspace);
    await settle();
    await frames();
    expect(toolbox.getSelectedItem()?.getName()).toBe('Functions');
    expect(headingTop(flyout, 'Math')).toBeLessThan(0);
  });
});

/**
 * Presses the pointer on the canvas as far as Blockly's state goes: a gesture is in progress, as
 * from Blockly's handling of a `pointerdown` until the release.
 */
function press(workspace: Blockly.WorkspaceSvg): Blockly.Gesture {
  const gesture = new Blockly.Gesture(new PointerEvent('pointerdown'), workspace);
  workspace.currentGesture_ = gesture;
  return gesture;
}

/**
 * Dispatches `type` on the document; with `gesture`, Blockly's own listener ends it then, as on a
 * release (it listens on the document in the bubbling phase).
 */
function release(
  type: 'pointerup' | 'pointercancel',
  gesture: Blockly.Gesture | null = null,
): void {
  const end = (): void => {
    gesture?.dispose();
  };
  document.addEventListener(type, end, { once: true });
  document.dispatchEvent(new PointerEvent(type, { bubbles: true }));
}

describe('a rebuild asked for while the pointer is pressed', () => {
  it('comes once the press is released', async () => {
    const workspace = attached('continuous');
    await settle();
    // Blockly refreshes no toolbox while a gesture runs.
    const gesture = press(workspace);
    addCount(workspace);
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_guess', 's_n']);
    release('pointerup', gesture);
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_count', 's_guess', 's_n']);
  });

  it('comes once a cancelled press has ended', async () => {
    const workspace = attached('continuous');
    await settle();
    const gesture = press(workspace);
    addCount(workspace);
    await settle();
    release('pointercancel', gesture);
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_count', 's_guess', 's_n']);
  });

  it('waits again when the gesture goes on after a release (another finger still down)', async () => {
    const workspace = attached('continuous');
    await settle();
    const gesture = press(workspace);
    addCount(workspace);
    await settle();
    release('pointerup');
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_guess', 's_n']);
    release('pointerup', gesture);
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_count', 's_guess', 's_n']);
  });

  it('stops waiting when the toolbox is detached', async () => {
    const workspace = injectedWorkspace('continuous');
    const document_ = guessDocument();
    loadModule(workspace, document_);
    storeAnalysis(document_, [factorial, guess, n]);
    const detach = createToolboxPlugin().attach(editorContext(workspace, fakeCore(SCOPES)));
    await settle();
    const gesture = press(workspace);
    addCount(workspace);
    await settle();
    const remove = vi.spyOn(document, 'removeEventListener');
    detach();
    expect(remove).toHaveBeenCalledWith('pointerup', expect.any(Function), true);
    expect(remove).toHaveBeenCalledWith('pointercancel', expect.any(Function), true);
    const refreshes = vi.spyOn(workspace, 'refreshToolboxSelection');
    release('pointerup', gesture);
    await settle();
    expect(refreshes).not.toHaveBeenCalled();
  });
});

describe('the span b2c:toolbox-refresh', () => {
  it('covers the continuous toolbox’s rebuild of its flyout, which comes later', async () => {
    const workspace = attached('continuous');
    await settle();
    resetTimings();
    const flyout = continuousToolbox(workspace).getFlyout();
    const show = flyout.show.bind(flyout);
    // A slow showing of the flyout: the span must include it.
    const slowMs = 30;
    vi.spyOn(flyout, 'show').mockImplementation((contents) => {
      const end = performance.now() + slowMs;
      while (performance.now() < end) {
        // Busy, as a large flyout's rebuild is.
      }
      show(contents);
    });
    addCount(workspace);
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_count', 's_guess', 's_n']);
    expect(latestTimings()['b2c:toolbox-refresh']).toBeGreaterThanOrEqual(slowMs);
  });
});

describe('the toolbox plugin with the category toolbox', () => {
  it('shows a dynamic category when it is selected, whatever block is selected', async () => {
    const workspace = attached('category');
    const toolbox = workspace.getToolbox() as Blockly.Toolbox;
    expect(toolbox).not.toBeInstanceOf(B2cContinuousToolbox);
    toolbox.setSelectedItem(category(workspace, 'Variables'));
    expect(getterRefs(workspace)).toEqual(['s_guess', 's_n']);
    const flyout = toolbox.getFlyout();
    if (flyout === null) {
      throw new Error('no flyout');
    }
    const show = vi.spyOn(flyout, 'show');
    select(workspace, 'fn');
    await settle();
    expect(show).not.toHaveBeenCalled();
    addCount(workspace);
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_count', 's_guess', 's_n']);
  });

  it('names a new loop i and a new function myFunction, and lists the calls under Your functions', () => {
    const workspace = attached('category');
    const toolbox = workspace.getToolbox() as Blockly.Toolbox;
    toolbox.setSelectedItem(category(workspace, 'Loops'));
    const loop = flyoutBlocks(workspace).find((block) => block.type === 'control.for_range');
    expect((loop?.getField('VAR') as B2cSymbolDeclField).getDecl()?.name).toBe('i');

    toolbox.setSelectedItem(category(workspace, 'Functions'));
    const types = flyoutBlocks(workspace).map((block) => block.type);
    expect(types).toEqual(['func.define', 'func.return', 'func.call']);
    const define = flyoutBlocks(workspace)[0]?.getField('NAME') as B2cSymbolDeclField;
    expect(define.getDecl()?.name).toBe('myFunction');
  });
});

describe('names in the flyout after a rename', () => {
  /** The text Blockly drew for field `field` of the first flyout block of `type`. */
  function drawn(workspace: Blockly.WorkspaceSvg, type: string, field: string): string | null {
    const block = flyoutBlocks(workspace).find((candidate) => candidate.type === type);
    return block?.getField(field)?.getSvgRoot()?.textContent ?? null;
  }

  /** guessDocument() with the declaration of `sym` named `name`. */
  function renamed(sym: string, name: string): BdmDocument {
    const document = guessDocument();
    for (const top of document.modules[0]?.workspace.blocks ?? []) {
      for (const block of [top, ...(top.statements?.['BODY'] ?? [])]) {
        const decl = block.fields?.['NAME'];
        if (typeof decl === 'object' && 'sym' in decl && decl.sym === sym) {
          block.fields = { ...block.fields, NAME: { sym, name } };
        }
      }
    }
    return document;
  }

  /** The editor's symbol services and session hooks on the attached workspace. */
  function withServices(workspace: Blockly.WorkspaceSvg) {
    const services = createSymbolServices({
      core: () => fakeCore(SCOPES),
      preview: () => useAppStore.getState().analysis.preview,
      workspace,
    });
    detachers.push(
      installEditorServices({
        symbols: services.symbols,
        types: services.types,
        dialogs: DEFAULT_EDITOR_SERVICES.dialogs,
      }),
    );
    const hooks = servicesSessionHooks(services, workspace);
    hooks.onLoaded?.(guessDocument());
    return hooks;
  }

  /** What follows a commit of the renamed document: Blockly's renders and the toolbox's rebuild. */
  async function renders(): Promise<void> {
    await Blockly.renderManagement.finishQueuedRenders();
    await settle();
  }

  it('shows the new name of a renamed variable, and lays the flyout out again', async () => {
    const workspace = attached('continuous');
    const hooks = withServices(workspace);
    await settle();
    expect(drawn(workspace, 'var.get', 'VAR')).toBe('guess');
    const flyout = workspace.getToolbox()?.getFlyout();
    if (flyout === null || flyout === undefined) {
      throw new Error('no flyout');
    }
    const reflow = vi.spyOn(flyout, 'reflow');

    hooks.onCommitted?.(renamed('s_guess', 'attemptsSoFar'));
    publishPreview([factorial, { ...guess, name: 'attemptsSoFar' }, n]);
    await renders();
    expect(drawn(workspace, 'var.get', 'VAR')).toBe('attemptsSoFar');
    expect(reflow).toHaveBeenCalled();
  });

  it('shows the new name of a renamed function on its call block', async () => {
    const workspace = attached('continuous');
    const hooks = withServices(workspace);
    await settle();
    expect(drawn(workspace, 'func.call', 'FUNC')).toBe('factorial');

    hooks.onCommitted?.(renamed('s_fact', 'fact'));
    publishPreview([{ ...factorial, name: 'fact' }, guess, n]);
    await renders();
    expect(drawn(workspace, 'func.call', 'FUNC')).toBe('fact');
  });

  it('labels a call’s arguments with the new name of a renamed parameter', async () => {
    const workspace = attached('continuous');
    withServices(workspace);
    await settle();
    expect(drawn(workspace, 'func.call', argLabelField(0))).toBe('n:');

    // The labels are part of what the flyout lists, so the flyout shows new blocks.
    publishPreview([factorial, guess, { ...n, name: 'count' }]);
    await renders();
    expect(drawn(workspace, 'func.call', argLabelField(0))).toBe('count:');
  });
});

describe('attaching and detaching', () => {
  it('registers the dynamic categories and Make a variable, and removes them again', () => {
    const workspace = injectedWorkspace('continuous');
    storeAnalysis(documentFixture(), []);
    const detach = createToolboxPlugin().attach(editorContext(workspace, fakeCore({})));
    const keys = [PROGRAM_CATEGORY, VARIABLES_CATEGORY, MY_BLOCKS_CATEGORY];
    for (const key of keys) {
      expect(workspace.getToolboxCategoryCallback(key)).not.toBeNull();
    }
    expect(workspace.getButtonCallback(MAKE_VARIABLE_BUTTON)).not.toBeNull();
    expect(toolboxSymbolGroups()).not.toBeNull();

    detach();
    for (const key of keys) {
      expect(workspace.getToolboxCategoryCallback(key)).toBeNull();
    }
    expect(workspace.getButtonCallback(MAKE_VARIABLE_BUTTON)).toBeNull();
    expect(toolboxSymbolGroups()).toBeNull();
    // The toolbox went back to static categories, which need no callbacks.
    expect(() => {
      continuousToolbox(workspace).showAllCategories();
    }).not.toThrow();
  });

  it('warns and leaves the workspace alone when it has no category toolbox', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const host = document.createElement('div');
    document.body.append(host);
    const workspace = Blockly.inject(host, { renderer: 'zelos', sounds: false });
    try {
      const detach = createToolboxPlugin().attach(editorContext(workspace, fakeCore({})));
      expect(warn).toHaveBeenCalledWith(expect.stringContaining('without a category toolbox'));
      detach();
    } finally {
      workspace.dispose();
    }
  });

  it('shows a note instead of failing when a category cannot be built', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const workspace = attached('category');
    useAppStore.setState((state) => ({
      analysis: {
        ...state.analysis,
        // A broken analysis: symbols that are not a list.
        preview: { ...(state.analysis.preview as object), symbols: 7 } as never,
      },
    }));
    await settle();
    (workspace.getToolbox() as Blockly.Toolbox).setSelectedItem(category(workspace, 'Functions'));
    const flyout = workspace.getToolbox()?.getFlyout();
    const texts = (flyout?.getContents() ?? []).map((item) => {
      const element = item.getElement();
      return element instanceof Blockly.FlyoutButton ? element.getButtonText() : null;
    });
    expect(texts).toContain('These blocks could not be shown.');
    expect(error).toHaveBeenCalled();
  });
});

describe('Make a variable from the flyout button', () => {
  it('asks for a name and a type and inserts the declaration, once at a time', async () => {
    const results: MakeVariableResult[] = [];
    let answer: (value: string | null) => void = () => undefined;
    const dialogs = makeDialogs(
      () =>
        new Promise<string | null>((resolve) => {
          answer = resolve;
        }),
    );
    const workspace = attached('continuous', {
      dialogs,
      onMakeVariable: (result) => results.push(result),
    });
    const button = workspace.getButtonCallback(MAKE_VARIABLE_BUTTON);
    button?.({} as Blockly.FlyoutButton);
    button?.({} as Blockly.FlyoutButton);
    expect(dialogs.prompt).toHaveBeenCalledTimes(1);
    expect(dialogs.prompt.mock.calls[0]?.[0]).toMatchObject({ defaultValue: 'value' });

    answer('score');
    await settle();
    expect(dialogs.choose).toHaveBeenCalledTimes(1);
    expect(results).toHaveLength(1);
    expect(results[0]?.status).toBe('created');
    // Nothing selected: it goes to the top of main, before `create int guess`.
    const main = workspace.getBlockById('main');
    const first = main?.getInputTargetBlock('BODY');
    expect((first?.getField('NAME') as B2cSymbolDeclField).getDecl()?.name).toBe('score');
    expect(first?.getFieldValue('TYPE')).toBe('int');
    expect(first?.getNextBlock()?.id).toBe('decl');
  });

  it('inserts before the block selected on the canvas, though the focus is on the button', async () => {
    const results: MakeVariableResult[] = [];
    const workspace = attached('continuous', {
      dialogs: makeDialogs(Promise.resolve('score')),
      onMakeVariable: (result) => results.push(result),
    });
    await settle();
    select(workspace, 'print');
    await settle();
    // Pressing the button focuses it in the flyout, which ends Blockly's own selection.
    const button = flyoutButtons(workspace).find(
      (item) => item.getButtonText() === 'Make a variable',
    );
    if (button === undefined) {
      throw new Error('no Make a variable button');
    }
    Blockly.getFocusManager().focusNode(button);
    expect(Blockly.getSelected()).toBeNull();
    workspace.getButtonCallback(MAKE_VARIABLE_BUTTON)?.(button);
    await settle();
    expect(results[0]?.status).toBe('created');
    const inserted = workspace.getBlockById('print')?.getPreviousBlock();
    expect((inserted?.getField('NAME') as B2cSymbolDeclField).getDecl()?.name).toBe('score');
    expect(inserted?.getPreviousBlock()?.id).toBe('decl');
  });
});

describe('Make a variable through the app’s dialogs', () => {
  it('lends Blockly’s focus to the dialog, and will not make main where another module has it', async () => {
    const queue = createDialogQueue();
    render(createElement(DialogHost, { queue }));
    const results: MakeVariableResult[] = [];
    const workspace = injectedWorkspace('continuous');
    const document = documentFixture();
    document.modules = [
      { id: 'mod_main', name: 'main', workspace: { blocks: [] } },
      {
        id: 'mod_other',
        name: 'other',
        workspace: { blocks: [{ id: 'main', type: 'program.main', v: 1, x: 0, y: 0 }] },
      },
    ];
    storeAnalysis(document, []);
    detachers.push(
      createToolboxPlugin({
        dialogs: queue,
        onMakeVariable: (result) => results.push(result),
      }).attach(editorContext(workspace, fakeCore({}))),
    );

    await act(async () => {
      workspace.getButtonCallback(MAKE_VARIABLE_BUTTON)?.({} as Blockly.FlyoutButton);
      await Promise.resolve();
    });
    const dialog = await screen.findByRole('dialog', { name: 'Make a variable' });
    expect(dialog.textContent).toContain('no “when program starts” block');
    expect(Blockly.getFocusManager().ephemeralFocusTaken()).toBe(true);

    fireEvent.click(screen.getByRole('button', { name: 'OK' }));
    await act(async () => {
      await settle();
    });
    expect(results).toEqual([{ status: 'refused', reason: 'noMain' }]);
    expect(Blockly.getFocusManager().ephemeralFocusTaken()).toBe(false);
    expect(workspace.getAllBlocks(false)).toHaveLength(0);
  });

  it('says where the program starts when another module has it', async () => {
    const workspace = injectedWorkspace('continuous');
    const document = documentFixture();
    document.modules = [
      { id: 'mod_main', name: 'helpers', workspace: { blocks: [] } },
      {
        id: 'mod_game',
        name: 'game',
        workspace: { blocks: [{ id: 'main', type: 'program.main', v: 1, x: 0, y: 0 }] },
      },
    ];
    storeAnalysis(document, []);
    detachers.push(createToolboxPlugin().attach(editorContext(workspace, fakeCore({}))));
    await settle();
    expect(flyoutTexts(workspace)).toEqual(
      expect.arrayContaining(['Your program starts in', 'the module “game”.']),
    );
    expect(flyoutBlocks(workspace).map((block) => block.type)).not.toContain('program.main');
  });
});

describe('the package entry', () => {
  it('gives the app’s toolbox plugin', async () => {
    const entry = await import('./index');
    expect(entry.toolboxPlugin.name).toBe('toolbox');
    expect(typeof entry.toolboxPlugin.attach).toBe('function');
    expect(entry.toolboxInjectOptions().plugins).toEqual({
      toolbox: entry.CONTINUOUS_TOOLBOX,
      flyoutsVerticalToolbox: entry.CONTINUOUS_FLYOUT,
      metricsManager: entry.CONTINUOUS_METRICS,
    });
  });
});

describe('the analysis symbol source', () => {
  it('answers nothing before the first analysis, and from the current core after it', () => {
    const core = fakeCore({ x: [guess] });
    const source = analysisSymbolSource(useAppStore, () => core);
    expect(source.symbolsAt('x', null)).toEqual([]);
    expect(source.allSymbols()).toEqual([]);
    storeAnalysis(documentFixture(), [guess]);
    expect(source.symbolsAt('x', null)).toEqual([guess]);
    expect(source.allSymbols()).toEqual([guess]);
  });

  it('answers nothing from a core that fails (a trap) or is missing', () => {
    storeAnalysis(documentFixture(), [guess]);
    const trapped = {
      ...fakeCore({}),
      symbolsInScope: () => {
        throw new Error('trapped');
      },
    };
    expect(analysisSymbolSource(useAppStore, () => trapped).symbolsAt('x', null)).toEqual([]);
    expect(analysisSymbolSource(useAppStore, () => null).symbolsAt('x', null)).toEqual([]);
  });
});
