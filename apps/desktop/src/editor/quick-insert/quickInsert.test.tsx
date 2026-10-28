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
import { QuickInsert } from './QuickInsert';
import { QUICK_INSERT_IDS, openQuickInsertOn, quickInsertPlugin } from './plugin';
import { closeQuickInsert, quickInsertSession } from './store';
import { closeToolboxSearch, openToolboxSearch, toolboxSearchOpen } from './toolboxSearchStore';

// The tests show the continuous toolbox's flyout (FLYOUT_TEST_TIMEOUT_MS).
vi.setConfig({ testTimeout: FLYOUT_TEST_TIMEOUT_MS });

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
      Blockly.ContextMenuRegistry.registry.getItem(QUICK_INSERT_IDS.canvasMenu),
    ).not.toBeNull();
    expect(workspace).toBeDefined();
    for (const detach of detachers.splice(0)) {
      detach();
    }
    expect(
      Blockly.ShortcutRegistry.registry.getRegistry()[QUICK_INSERT_IDS.shortcut],
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

  it('opens and closes with its store', () => {
    const workspace = attached();
    openToolboxSearch(workspace);
    expect(toolboxSearchOpen()).toBe(workspace);
    closeToolboxSearch();
    expect(toolboxSearchOpen()).toBeNull();
  });
});
