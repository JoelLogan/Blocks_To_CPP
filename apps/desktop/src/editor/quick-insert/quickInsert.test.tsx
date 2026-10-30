/**
 * Quick Insert and toolbox search on a rendered canvas with the toolbox
 * (docs/plans/m3-contracts/w4-editor-tools.md §2.4): opening, inserting at the selection or on the
 * canvas as one undo step, the popup's keys and ARIA, and the toolbox's search results.
 */
import { act, fireEvent, render, screen } from '@testing-library/react';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { setCore } from '../../app/core';
import { resetAppStore } from '../../app/store';
import { expectNoAxeViolations } from '../../test/axe';
import { CONTINUOUS_REFRESH_DELAY_MS } from '../toolbox/continuous';
import { SEARCH_RESULTS_NAME, createToolboxPlugin, toolboxSearch } from '../toolbox/plugin';
import {
  FLYOUT_TEST_TIMEOUT_MS,
  disposeTestWorkspaces,
  editorContext,
  fakeCore,
  guessDocument,
  injectedWorkspace,
  loadModule,
  setUpToolboxBlocks,
  storeAnalysis,
  symbolFixture,
} from '../toolbox/testing';
import { createKeyboardPlugin } from '../keyboard/plugin';
import { QuickInsert } from './QuickInsert';
import { SEARCH_DELAY_MS, ToolboxSearch } from './ToolboxSearch';
import { QUICK_INSERT_IDS, openQuickInsertOn, quickInsertPlugin } from './plugin';
import { closeQuickInsert, quickInsertSession } from './store';
import { closeToolboxSearch, openToolboxSearch, toolboxSearchOpen } from './toolboxSearchStore';

// The tests show the continuous toolbox's flyout (FLYOUT_TEST_TIMEOUT_MS): the slowest took 4.8 s
// in the whole suite under coverage with two workers (2026-10-30), against Vitest's 5 s default.
vi.setConfig({ testTimeout: FLYOUT_TEST_TIMEOUT_MS });

/** Counts the search index builds (`searchEntries`), which run the real one. */
const buildCount = vi.hoisted(() => vi.fn());
vi.mock('../search', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../search')>();
  return {
    ...actual,
    searchEntries: (...args: Parameters<typeof actual.searchEntries>) => {
      buildCount();
      return actual.searchEntries(...args);
    },
  };
});

beforeAll(() => {
  setUpToolboxBlocks();
});

beforeEach(() => {
  resetAppStore();
});

const detachers: (() => void)[] = [];

afterEach(() => {
  closeQuickInsert();
  closeToolboxSearch();
  for (const detach of detachers.splice(0)) {
    detach();
  }
  setCore(null);
  disposeTestWorkspaces();
  resetAppStore();
});

const guess = symbolFixture('s_guess', 'guess', { declBlock: 'decl' });

/** A canvas with the guess document, the toolbox and Quick Insert. */
function attached(): Blockly.WorkspaceSvg {
  const workspace = injectedWorkspace('continuous');
  const document = guessDocument();
  loadModule(workspace, document);
  storeAnalysis(document, [guess]);
  const context = editorContext(workspace, fakeCore({ print: [guess], decl: [] }));
  detachers.push(createToolboxPlugin().attach(context));
  detachers.push(quickInsertPlugin.attach(context));
  return workspace;
}

/** Waits for Blockly's events and the toolbox's delayed rebuild. */
function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, CONTINUOUS_REFRESH_DELAY_MS + 50));
}

/** Opens Quick Insert on `workspace`, renders it, and types `text`. */
function openAndType(workspace: Blockly.WorkspaceSvg, text: string): HTMLInputElement {
  render(<QuickInsert workspace={workspace} />);
  act(() => {
    expect(openQuickInsertOn(workspace)).toBe(true);
  });
  const box = screen.getByRole('combobox', { name: 'Insert a block' });
  if (!(box instanceof HTMLInputElement)) {
    throw new Error('no search box');
  }
  fireEvent.change(box, { target: { value: text } });
  return box;
}

describe('Quick Insert', () => {
  it('registers Ctrl+Space and the menu items while an editor is attached', () => {
    const workspace = attached();
    expect(
      Blockly.ShortcutRegistry.registry.getRegistry()[QUICK_INSERT_IDS.shortcut],
    ).toBeDefined();
    expect(
      Blockly.ShortcutRegistry.registry.getRegistry()[QUICK_INSERT_IDS.searchShortcut],
    ).toBeDefined();
    expect(
      Blockly.ContextMenuRegistry.registry.getItem(QUICK_INSERT_IDS.canvasMenu),
    ).not.toBeNull();
    expect(workspace).toBeDefined();
    for (const detach of detachers.splice(0)) {
      detach();
    }
    expect(
      Blockly.ShortcutRegistry.registry.getRegistry()[QUICK_INSERT_IDS.shortcut],
    ).toBeUndefined();
    expect(
      Blockly.ShortcutRegistry.registry.getRegistry()[QUICK_INSERT_IDS.searchShortcut],
    ).toBeUndefined();
  });

  it('inserts the best result loose on the canvas, as one undo step', async () => {
    const workspace = attached();
    await settle();
    const before = workspace.getAllBlocks(false).filter((block) => block.type === 'io.print');
    const box = openAndType(workspace, 'cout');
    expect(screen.getAllByRole('option')[0]?.textContent).toContain('print');
    expect(box.getAttribute('aria-activedescendant')).toBe(screen.getAllByRole('option')[0]?.id);
    fireEvent.keyDown(box, { key: 'Enter' });
    expect(quickInsertSession()).toBeNull();
    const after = workspace.getAllBlocks(false).filter((block) => block.type === 'io.print');
    expect(after).toHaveLength(before.length + 1);
    const added = after.find((block) => !before.includes(block));
    expect(added?.getParent()).toBeNull();
    expect((Blockly.common.getSelected() as Blockly.Block | null)?.id).toBe(added?.id);
    await settle();
    workspace.undo(false);
    expect(workspace.getBlockById(added?.id ?? '')).toBeNull();
  });

  it('inserts a typed line after the selected statement', async () => {
    const workspace = attached();
    await settle();
    const print = workspace.getBlockById('print');
    if (!(print instanceof Blockly.BlockSvg)) {
      throw new Error('no print block');
    }
    Blockly.common.setSelected(print);
    const box = openAndType(workspace, 'guess += 2');
    expect(screen.getAllByRole('option')[0]?.textContent).toContain('Insert “change guess”');
    fireEvent.keyDown(box, { key: 'Enter' });
    const next = workspace.getBlockById('print')?.getNextBlock();
    expect(next?.type).toBe('var.change');
    expect(next?.getFieldValue('OP')).toBe('add');
  });

  it('inserts at the start of the body of a selected block with nothing after it', async () => {
    const workspace = attached();
    await settle();
    const main = workspace.getBlockById('main');
    if (!(main instanceof Blockly.BlockSvg)) {
      throw new Error('no main block');
    }
    Blockly.common.setSelected(main);
    const box = openAndType(workspace, 'int total = 5');
    fireEvent.keyDown(box, { key: 'Enter' });
    const first = main.getInputTargetBlock('BODY');
    expect(first?.type).toBe('var.declare');
    expect(first?.getFieldValue('NAME')).toBe('total');
    expect(first?.getNextBlock()?.id).toBe('decl');
  });

  it('inserts at the connection the keyboard cursor is on, as one undo step, and says so', async () => {
    const workspace = attached();
    detachers.push(
      createKeyboardPlugin({ matchMedia: null }).attach(editorContext(workspace, fakeCore({}))),
    );
    await settle();
    const print = workspace.getBlockById('print');
    if (
      !(print instanceof Blockly.BlockSvg) ||
      !(print.nextConnection instanceof Blockly.RenderedConnection)
    ) {
      throw new Error('no print block');
    }
    Blockly.getFocusManager().focusNode(print.nextConnection);
    const box = openAndType(workspace, 'int total = 5');
    fireEvent.keyDown(box, { key: 'Enter' });
    const added = workspace.getBlockById('print')?.getNextBlock();
    expect(added?.type).toBe('var.declare');
    expect(added?.getFieldValue('NAME')).toBe('total');
    const announcer = workspace
      .getInjectionDiv()
      .querySelector('[data-testid="keyboard-announcer"] [role="status"]');
    expect(announcer?.textContent.trim()).toBe('Inserted “create total”');
    await settle();
    workspace.undo(false);
    expect(workspace.getBlockById(added?.id ?? '')).toBeNull();
    expect(workspace.getBlockById('print')?.getNextBlock()).toBeNull();
  });

  it('inserts a value into the selected block’s first empty value input', async () => {
    const workspace = attached();
    await settle();
    const fn = workspace.getBlockById('fn');
    if (!(fn instanceof Blockly.BlockSvg)) {
      throw new Error('no define block');
    }
    Blockly.common.setSelected(fn);
    const box = openAndType(workspace, 'sqrt');
    fireEvent.keyDown(box, { key: 'Enter' });
    const added = fn.getInputTargetBlock('DEFAULT0');
    expect(added).not.toBeNull();
    expect(added?.getParent()?.id).toBe('fn');
  });

  it('says it reads simple lines only, moves with the arrows and closes on Escape', async () => {
    const workspace = attached();
    await settle();
    const box = openAndType(workspace, 'for (int i = 0; i < 3; ++i)');
    expect(screen.getByText(/reads simple lines only/u)).toBeDefined();
    fireEvent.change(box, { target: { value: 'print' } });
    const options = screen.getAllByRole('option');
    fireEvent.keyDown(box, { key: 'ArrowDown' });
    expect(box.getAttribute('aria-activedescendant')).toBe(options[1]?.id);
    fireEvent.keyDown(box, { key: 'ArrowUp' });
    expect(box.getAttribute('aria-activedescendant')).toBe(options[0]?.id);
    await expectNoAxeViolations(screen.getByTestId('quick-insert'));
    fireEvent.keyDown(box, { key: 'Escape' });
    expect(quickInsertSession()).toBeNull();
    expect(screen.queryByTestId('quick-insert')).toBeNull();
  });
});

describe('toolbox search', () => {
  /** The types of the blocks the flyout shows. */
  function flyoutTypes(workspace: Blockly.WorkspaceSvg): string[] {
    return (workspace.getToolbox()?.getFlyout()?.getContents() ?? []).flatMap((item) => {
      const element = item.getElement();
      return element instanceof Blockly.BlockSvg ? [element.type] : [];
    });
  }

  it('shows the matching blocks in the flyout, then the categories again', async () => {
    const workspace = attached();
    await settle();
    const all = flyoutTypes(workspace).length;
    const search = toolboxSearch(workspace);
    search?.setQuery('sqrt');
    await settle();
    expect(flyoutTypes(workspace)).toEqual(['math.fn']);
    expect(search?.resultCount()).toBe(1);
    const toolbox = workspace.getToolbox() as Blockly.Toolbox;
    expect(
      toolbox.getToolboxItems().map((item) => (item as Blockly.ToolboxCategory).getName()),
    ).toEqual([SEARCH_RESULTS_NAME]);
    search?.setQuery('qqqzzz');
    await settle();
    expect(flyoutTypes(workspace)).toEqual([]);
    search?.setQuery(null);
    await settle();
    expect(flyoutTypes(workspace)).toHaveLength(all);
  });

  /** The getters (`var.get`) the flyout shows, by the symbol each reads. */
  function flyoutGetters(workspace: Blockly.WorkspaceSvg): string[] {
    return (workspace.getToolbox()?.getFlyout()?.getContents() ?? []).flatMap((item) => {
      const element = item.getElement();
      return element instanceof Blockly.BlockSvg && element.type === 'var.get'
        ? [String(element.getFieldValue('VAR'))]
        : [];
    });
  }

  it('runs the search again when a variable it found is gone', async () => {
    const workspace = attached();
    await settle();
    const search = toolboxSearch(workspace);
    search?.setQuery('guess');
    await settle();
    expect(flyoutGetters(workspace)).toEqual(['s_guess']);
    const found = search?.resultCount() ?? 0;
    expect(found).toBeGreaterThan(0);
    // The declaration deleted: the next analysis has no `guess`.
    act(() => {
      workspace.getBlockById('decl')?.dispose(true);
      storeAnalysis(guessDocument(), []);
    });
    await settle();
    expect(flyoutGetters(workspace)).toEqual([]);
    expect(search?.resultCount()).toBe(found - 1);
    expect(search?.query()).toBe('guess');
  });

  it('finds a variable made after the query was typed, without a new query', async () => {
    const workspace = attached();
    await settle();
    const search = toolboxSearch(workspace);
    search?.setQuery('total');
    await settle();
    expect(flyoutGetters(workspace)).toEqual([]);
    act(() => {
      storeAnalysis(guessDocument(), [
        guess,
        symbolFixture('s_total', 'total', { declBlock: 'decl' }),
      ]);
    });
    await settle();
    expect(flyoutGetters(workspace)).toEqual(['s_total']);
  });

  it('builds its index once for queries over the same contents', async () => {
    const workspace = attached();
    await settle();
    const search = toolboxSearch(workspace);
    buildCount.mockClear();
    search?.setQuery('pri');
    search?.setQuery('print');
    search?.setQuery('sqrt');
    await settle();
    expect(buildCount).toHaveBeenCalledTimes(1);
    act(() => {
      storeAnalysis(guessDocument(), [
        guess,
        symbolFixture('s_total', 'total', { declBlock: 'decl' }),
      ]);
    });
    await settle();
    search?.setQuery('total');
    await settle();
    expect(buildCount).toHaveBeenCalledTimes(2);
    expect(flyoutGetters(workspace)).toEqual(['s_total']);
  });

  it('is a labelled search field that passes axe and works with the keyboard', async () => {
    const workspace = attached();
    await settle();
    render(<ToolboxSearch workspace={workspace} />);
    act(() => {
      openToolboxSearch(workspace);
    });
    const region = screen.getByRole('search', { name: 'Toolbox search' });
    const field = screen.getByRole('searchbox', { name: 'Search blocks' });
    expect(document.activeElement).toBe(field);
    // ArrowDown with nothing typed stays in the field.
    fireEvent.keyDown(field, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(field);
    fireEvent.change(field, { target: { value: 'sqrt' } });
    await act(() => new Promise((resolve) => setTimeout(resolve, SEARCH_DELAY_MS + 50)));
    const status = screen.getByRole('status');
    expect(status.textContent).toBe('1 block');
    expect(field.getAttribute('aria-describedby')).toBe(status.id);
    await expectNoAxeViolations(region);
    fireEvent.keyDown(field, { key: 'ArrowDown' });
    expect(Blockly.getFocusManager().getFocusedTree()).toBe(workspace.getToolbox());
    act(() => {
      openToolboxSearch(workspace);
    });
    fireEvent.keyDown(screen.getByRole('searchbox', { name: 'Search blocks' }), {
      key: 'Escape',
    });
    expect(toolboxSearchOpen()).toBeNull();
    expect(screen.queryByTestId('toolbox-search')).toBeNull();
    expect(toolboxSearch(workspace)?.query()).toBeNull();
    expect(Blockly.getFocusManager().getFocusedTree()).toBe(workspace);
  });

  it('updates its status when the results are built again', async () => {
    const workspace = attached();
    await settle();
    render(<ToolboxSearch workspace={workspace} />);
    act(() => {
      openToolboxSearch(workspace);
    });
    const field = screen.getByRole('searchbox', { name: 'Search blocks' });
    fireEvent.change(field, { target: { value: 'total' } });
    await act(() => new Promise((resolve) => setTimeout(resolve, SEARCH_DELAY_MS + 50)));
    const before = toolboxSearch(workspace)?.resultCount() ?? 0;
    act(() => {
      storeAnalysis(guessDocument(), [
        guess,
        symbolFixture('s_total', 'total', { declBlock: 'decl' }),
      ]);
    });
    await act(() => settle());
    const after = toolboxSearch(workspace)?.resultCount() ?? 0;
    expect(after).toBe(before + 1);
    expect(screen.getByRole('status').textContent).toBe(
      after === 1 ? '1 block' : `${String(after)} blocks`,
    );
  });

  it('opens and closes with its store', () => {
    const workspace = attached();
    openToolboxSearch(workspace);
    expect(toolboxSearchOpen()).toBe(workspace);
    closeToolboxSearch();
    expect(toolboxSearchOpen()).toBeNull();
  });
});
