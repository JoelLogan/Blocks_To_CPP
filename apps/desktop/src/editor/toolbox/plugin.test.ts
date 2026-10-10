import type { BdmDocument, SymbolInfo } from '@blocks2cpp/b2c-core-wasm';
import { B2cSymbolDeclField, DEFAULT_EDITOR_SERVICES } from '@blocks2cpp/blockly-ext';
import { act, fireEvent, render, screen } from '@testing-library/react';
import * as Blockly from 'blockly/core';
import { createElement } from 'react';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { setCore } from '../../app/core';
import { DialogHost, createDialogQueue, type DialogService } from '../../app/dialogs';
import { resetAppStore, useAppStore } from '../../app/store';
import { documentFixture } from '../../app/testing/fixtures';
import { createSymbolServices, installEditorServices, servicesSessionHooks } from '../services';
import { ICON_CLASS, CATEGORY_CLASS } from './category';
import { B2cContinuousToolbox, CONTINUOUS_REFRESH_DELAY_MS } from './continuous';
import {
  LOOPS_CATEGORY,
  MAKE_VARIABLE_BUTTON,
  MY_BLOCKS_CATEGORY,
  VARIABLES_CATEGORY,
} from './contents';
import { argLabelField } from './inflater';
import type { MakeVariableResult } from './makeVariable';
import { PREVIEW_WAIT_MS, analysisSymbolSource, createToolboxPlugin } from './plugin';
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

/** The blocks the flyout shows, in order. */
function flyoutBlocks(workspace: Blockly.WorkspaceSvg): Blockly.BlockSvg[] {
  const flyout = workspace.getToolbox()?.getFlyout();
  return (flyout?.getContents() ?? []).flatMap((item) => {
    const element = item.getElement();
    return element instanceof Blockly.BlockSvg ? [element] : [];
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

describe('the toolbox plugin with the continuous toolbox', () => {
  it('shows every category in one flyout, with the dynamic ones built from the analysis', async () => {
    const workspace = attached('continuous');
    expect(workspace.getToolbox()).toBeInstanceOf(B2cContinuousToolbox);
    await settle();

    const types = flyoutBlocks(workspace).map((block) => block.type);
    expect(types).toContain('program.main');
    expect(types).toContain('io.ask');
    // With nothing selected, Variables lists what is visible at the end of main.
    expect(getterRefs(workspace)).toEqual(['s_guess']);
    // My Blocks: factorial gives a value, so it is a reporter, with its argument labelled.
    const call = flyoutBlocks(workspace).find((block) => block.type === 'func.call');
    expect(call?.getFieldValue('FUNC')).toBe('s_fact');
    expect(call?.getField(argLabelField(0))?.getText()).toBe('n:');
  });

  it('rebuilds Variables when the selection changes', async () => {
    const workspace = attached('continuous');
    await settle();
    select(workspace, 'fn');
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_n']);

    select(workspace, 'print');
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_guess']);
  });

  it('lists what is visible just after a selected statement', async () => {
    const workspace = attached('continuous');
    await settle();
    // `create int guess` itself: guess is visible after it.
    select(workspace, 'decl');
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_guess']);
  });

  it('lists the end of main for a block dropped loose on the canvas', async () => {
    const workspace = attached('continuous');
    await settle();
    select(workspace, 'fn');
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_n']);
    const loose = workspace.newBlock('io.print');
    loose.initSvg();
    loose.render();
    await settle();
    // Its analysis: the scope query has no answer for a loose block.
    publishPreview();
    select(workspace, loose.id);
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_guess']);
  });

  it('keeps the selection while the focus is in the toolbox or its flyout', async () => {
    const workspace = attached('continuous');
    await settle();
    select(workspace, 'fn');
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_n']);
    // Clicking a category moves the focus to the toolbox; the block stays the selection.
    const toolbox = workspace.getToolbox() as B2cContinuousToolbox;
    Blockly.getFocusManager().focusTree(toolbox);
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_n']);
    // Clicking the canvas background ends the selection: the end of main is listed again.
    Blockly.getFocusManager().focusNode(workspace);
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_guess']);
  });

  it('does not redraw the flyout when nothing it shows changed', async () => {
    const workspace = attached('continuous');
    await settle();
    const toolbox = workspace.getToolbox() as B2cContinuousToolbox;
    const show = vi.spyOn(toolbox.getFlyout(), 'show');
    // A block moved without changing any scope, and its new analysis with the same symbols.
    workspace.getBlockById('fn')?.moveBy(10, 10);
    await settle();
    publishPreview();
    await settle();
    expect(show).not.toHaveBeenCalled();
    // A different selection changes the Variables category: the flyout is redrawn.
    select(workspace, 'fn');
    await settle();
    expect(show).toHaveBeenCalledTimes(1);
    // Forcing always redraws.
    expect(toolbox.showAllCategories(true)).toBe(true);
    expect(toolbox.showAllCategories(false)).toBe(false);
  });

  it('keeps its listing while a preview is pending, and follows the selection once it comes', async () => {
    const workspace = attached('continuous');
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_guess']);
    // The canvas changes: the analysis does not know where things are any more.
    workspace.getBlockById('fn')?.moveBy(10, 10);
    await settle();
    select(workspace, 'fn');
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_guess']);
    // The preview comes: the listing is the selection's.
    publishPreview();
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_n']);
  });

  it('follows the selection again when no preview comes', async () => {
    const workspace = attached('continuous');
    await settle();
    workspace.getBlockById('fn')?.moveBy(10, 10);
    await settle();
    select(workspace, 'fn');
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_guess']);
    await new Promise((resolve) => setTimeout(resolve, PREVIEW_WAIT_MS));
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_n']);
  });

  it('follows the selection again when the canvas cannot be analysed', async () => {
    const workspace = attached('continuous');
    await settle();
    workspace.getBlockById('fn')?.moveBy(10, 10);
    await settle();
    select(workspace, 'fn');
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_guess']);
    useAppStore.setState((state) => ({ analysis: { ...state.analysis, notice: 'syncFailed' } }));
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_n']);
  });

  it('rebuilds after each analysis', async () => {
    const workspace = attached('continuous');
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_guess']);
    useAppStore.setState((state) => ({
      analysis: { ...state.analysis, seq: 2, preview: null },
    }));
    await settle();
    // No analysis: nothing is known to be in scope.
    expect(getterRefs(workspace)).toEqual([]);
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
    (workspace.getToolbox() as B2cContinuousToolbox).showAllCategories();
    const second = nameOf();
    expect(first?.name).toBe('value');
    expect(second?.name).toBe('value');
    expect(second?.sym).not.toBe(first?.sym);
  });
});

describe('the toolbox plugin with the category toolbox', () => {
  it('shows a dynamic category when it is selected and rebuilds it on selection change', async () => {
    const workspace = attached('category');
    const toolbox = workspace.getToolbox() as Blockly.Toolbox;
    expect(toolbox).not.toBeInstanceOf(B2cContinuousToolbox);
    toolbox.setSelectedItem(category(workspace, 'Variables'));
    expect(getterRefs(workspace)).toEqual(['s_guess']);

    select(workspace, 'fn');
    await settle();
    expect(getterRefs(workspace)).toEqual(['s_n']);
  });

  it('names a new loop and lists My Blocks in their categories', () => {
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
    for (const key of [VARIABLES_CATEGORY, LOOPS_CATEGORY, MY_BLOCKS_CATEGORY]) {
      expect(workspace.getToolboxCategoryCallback(key)).not.toBeNull();
    }
    expect(workspace.getButtonCallback(MAKE_VARIABLE_BUTTON)).not.toBeNull();

    detach();
    for (const key of [VARIABLES_CATEGORY, LOOPS_CATEGORY, MY_BLOCKS_CATEGORY]) {
      expect(workspace.getToolboxCategoryCallback(key)).toBeNull();
    }
    expect(workspace.getButtonCallback(MAKE_VARIABLE_BUTTON)).toBeNull();
    // The toolbox went back to static categories, which need no callbacks.
    expect(() => {
      (workspace.getToolbox() as B2cContinuousToolbox).showAllCategories();
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
  it('asks for a name and inserts the declaration, once at a time', async () => {
    const results: MakeVariableResult[] = [];
    let answer: (value: string | null) => void = () => undefined;
    const prompt = vi.fn<DialogService['prompt']>(
      () =>
        new Promise<string | null>((resolve) => {
          answer = resolve;
        }),
    );
    const workspace = attached('continuous', {
      dialogs: { prompt, alert: vi.fn(() => Promise.resolve()) },
      onMakeVariable: (result) => results.push(result),
    });
    const button = workspace.getButtonCallback(MAKE_VARIABLE_BUTTON);
    button?.({} as Blockly.FlyoutButton);
    button?.({} as Blockly.FlyoutButton);
    expect(prompt).toHaveBeenCalledTimes(1);
    expect(prompt.mock.calls[0]?.[0]).toMatchObject({ defaultValue: 'value' });

    answer('score');
    await settle();
    expect(results).toHaveLength(1);
    expect(results[0]?.status).toBe('created');
    // Nothing selected: it goes to the top of main, before `create int guess`.
    const main = workspace.getBlockById('main');
    const first = main?.getInputTargetBlock('BODY');
    expect((first?.getField('NAME') as B2cSymbolDeclField).getDecl()?.name).toBe('score');
    expect(first?.getNextBlock()?.id).toBe('decl');
  });

  it('inserts before the block selected on the canvas, though the focus is on the button', async () => {
    const results: MakeVariableResult[] = [];
    const workspace = attached('continuous', {
      dialogs: {
        prompt: vi.fn(() => Promise.resolve('score')),
        alert: vi.fn(() => Promise.resolve()),
      },
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
