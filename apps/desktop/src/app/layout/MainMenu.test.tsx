/**
 * The toolbar's main menu: it lists only registered commands (and the project items only with a
 * project), follows the WAI-ARIA menu button keyboard pattern, runs the chosen command and passes
 * axe-core. *Undo* and *Redo* are enabled from the block editor's undo and redo stacks.
 */
import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { expectNoAxeViolations } from '../../test/axe';
import { type CommandId, type CommandRegistry, createCommandRegistry } from '../commands';
import type { EditorHandle } from '../editor-types';
import { resetAppStore, useAppStore } from '../store';
import { projectFixture } from '../testing/fixtures';
import { MainMenu } from './MainMenu';

const ALL: CommandId[] = [
  'project.new',
  'project.open',
  'project.save',
  'project.saveAs',
  'project.close',
];

let registry: CommandRegistry;
let ran: CommandId[];

function registerAll(ids: readonly CommandId[] = ALL): void {
  for (const id of ids) {
    registry.registerCommand(id, () => {
      ran.push(id);
    });
  }
}

function renderMenu(editor: () => EditorHandle | null = () => null) {
  return render(
    <header>
      <MainMenu registry={registry} editor={editor} />
      <button type="button">Elsewhere</button>
    </header>,
  );
}

/** An editor whose undo and redo stacks hold `undo` and `redo` events. */
function editorWithHistory(undo: number, redo: number): EditorHandle {
  return {
    workspace: {
      getUndoStack: () => Array.from({ length: undo }),
      getRedoStack: () => Array.from({ length: redo }),
    },
  } as unknown as EditorHandle;
}

/** Whether the menu item `name` is disabled. */
function isDisabled(name: string): boolean {
  return screen.getByRole('menuitem', { name }).getAttribute('aria-disabled') === 'true';
}

function menuButton(): HTMLElement {
  return screen.getByRole('button', { name: 'Main menu' });
}

function itemNames(): string[] {
  return screen.getAllByRole('menuitem').map((item) => item.textContent);
}

beforeEach(() => {
  resetAppStore();
  registry = createCommandRegistry();
  ran = [];
});

describe('the main menu', () => {
  it('is not shown while no feature provides its commands', () => {
    renderMenu();
    expect(screen.queryByRole('button', { name: 'Main menu' })).toBeNull();
    act(() => {
      registerAll(['project.open']);
    });
    expect(menuButton().getAttribute('aria-haspopup')).toBe('menu');
  });

  it('offers New and Open without a project, and the project items with one', () => {
    registerAll();
    renderMenu();
    fireEvent.click(menuButton());
    expect(itemNames()).toEqual(['New project…', 'Open…']);
    fireEvent.click(menuButton());

    act(() => {
      useAppStore.getState().actions.setProject(projectFixture());
    });
    fireEvent.click(menuButton());
    expect(itemNames()).toEqual([
      'New project…',
      'Open…',
      'SaveCtrl+S',
      'Save as…',
      'Close project',
    ]);
    expect(screen.getByRole('menuitem', { name: 'Save' }).getAttribute('aria-keyshortcuts')).toBe(
      'Control+S',
    );
    expect(screen.getAllByRole('separator')).toHaveLength(2);
  });

  it('opens on its first item and runs the chosen command', () => {
    registerAll();
    useAppStore.getState().actions.setProject(projectFixture());
    renderMenu();
    const button = menuButton();
    fireEvent.click(button);
    expect(button.getAttribute('aria-expanded')).toBe('true');
    const menu = screen.getByRole('menu', { name: 'Main menu' });
    expect(button.getAttribute('aria-controls')).toBe(menu.id);
    expect(document.activeElement?.textContent).toBe('New project…');

    fireEvent.click(screen.getByRole('menuitem', { name: 'Save as…' }));
    expect(ran).toEqual(['project.saveAs']);
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(button);
  });

  it('moves with the arrow keys, Home and End, wrapping around', () => {
    registerAll();
    useAppStore.getState().actions.setProject(projectFixture());
    renderMenu();
    fireEvent.keyDown(menuButton(), { key: 'ArrowUp' });
    const menu = screen.getByRole('menu');
    expect(document.activeElement?.textContent).toBe('Close project');
    fireEvent.keyDown(menu, { key: 'ArrowDown' });
    expect(document.activeElement?.textContent).toBe('New project…');
    fireEvent.keyDown(menu, { key: 'ArrowUp' });
    expect(document.activeElement?.textContent).toBe('Close project');
    fireEvent.keyDown(menu, { key: 'Home' });
    expect(document.activeElement?.textContent).toBe('New project…');
    fireEvent.keyDown(menu, { key: 'End' });
    expect(document.activeElement?.textContent).toBe('Close project');
    fireEvent.keyDown(menu, { key: 'ArrowUp' });
    expect(document.activeElement?.textContent).toBe('Save as…');
    fireEvent.keyDown(menu, { key: 'x' });
    expect(document.activeElement?.textContent).toBe('Save as…');
  });

  it('opens with the down arrow and closes with Escape, Tab or a press outside', () => {
    registerAll();
    renderMenu();
    const button = menuButton();

    fireEvent.keyDown(button, { key: 'ArrowDown' });
    expect(document.activeElement?.textContent).toBe('New project…');
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(button);

    fireEvent.click(button);
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'Tab' });
    expect(screen.queryByRole('menu')).toBeNull();

    fireEvent.click(button);
    fireEvent.pointerDown(screen.getByRole('button', { name: 'Elsewhere' }));
    expect(screen.queryByRole('menu')).toBeNull();

    fireEvent.click(button);
    fireEvent.pointerDown(screen.getByRole('menuitem', { name: 'Open…' }));
    expect(screen.getByRole('menu')).toBeTruthy();
    fireEvent.click(button);
    expect(screen.queryByRole('menu')).toBeNull();
    expect(ran).toEqual([]);
  });

  it('closes when its last command goes away', () => {
    const remove = registry.registerCommand('project.open', () => undefined);
    renderMenu();
    fireEvent.click(menuButton());
    expect(screen.getByRole('menu')).toBeTruthy();
    act(() => {
      remove();
    });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(screen.queryByRole('button', { name: 'Main menu' })).toBeNull();
  });

  it('logs a command that fails', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    registry.registerCommand('project.open', () => Promise.reject(new Error('boom')));
    renderMenu();
    fireEvent.click(menuButton());
    fireEvent.click(screen.getByRole('menuitem', { name: 'Open…' }));
    await vi.waitFor(() => {
      expect(error).toHaveBeenCalled();
    });
  });

  it('lists Rename project…, Undo and Redo with a project once their commands exist', () => {
    registerAll([...ALL, 'project.rename', 'edit.undo', 'edit.redo']);
    renderMenu();
    fireEvent.click(menuButton());
    expect(itemNames()).toEqual(['New project…', 'Open…']);
    fireEvent.click(menuButton());

    act(() => {
      useAppStore.getState().actions.setProject(projectFixture());
    });
    fireEvent.click(menuButton());
    expect(itemNames()).toEqual([
      'New project…',
      'Open…',
      'SaveCtrl+S',
      'Save as…',
      'Rename project…',
      'UndoCtrl+Z',
      'RedoCtrl+Y',
      'Close project',
    ]);
    expect(screen.getAllByRole('separator')).toHaveLength(3);
    expect(screen.getByRole('menuitem', { name: 'Undo' }).getAttribute('aria-keyshortcuts')).toBe(
      'Control+Z',
    );
    expect(screen.getByRole('menuitem', { name: 'Redo' }).getAttribute('aria-keyshortcuts')).toBe(
      'Control+Y Control+Shift+Z',
    );

    fireEvent.click(screen.getByRole('menuitem', { name: 'Rename project…' }));
    expect(ran).toEqual(['project.rename']);
  });

  it('enables Undo and Redo from the editor’s history while the editor is shown', () => {
    registerAll(['project.open', 'edit.undo', 'edit.redo']);
    let editor: EditorHandle | null = editorWithHistory(2, 0);
    act(() => {
      useAppStore.getState().actions.setProject(projectFixture());
      useAppStore.getState().actions.setUi({ screen: 'editor' });
    });
    renderMenu(() => editor);

    fireEvent.click(menuButton());
    expect(isDisabled('Undo')).toBe(false);
    expect(isDisabled('Redo')).toBe(true);
    expect(isDisabled('Open…')).toBe(false);
    // A disabled item is still reached with the arrow keys, but choosing it does nothing.
    fireEvent.keyDown(screen.getByRole('menu'), { key: 'End' });
    expect(document.activeElement?.textContent).toBe('RedoCtrl+Y');
    fireEvent.click(screen.getByRole('menuitem', { name: 'Redo' }));
    expect(screen.getByRole('menu')).toBeTruthy();
    expect(ran).toEqual([]);
    fireEvent.click(screen.getByRole('menuitem', { name: 'Undo' }));
    expect(ran).toEqual(['edit.undo']);
    expect(screen.queryByRole('menu')).toBeNull();

    // Read again each time the menu opens.
    editor = editorWithHistory(0, 1);
    fireEvent.click(menuButton());
    expect(isDisabled('Undo')).toBe(true);
    expect(isDisabled('Redo')).toBe(false);
    fireEvent.click(menuButton());

    // Not while another page is shown, nor without an editor.
    act(() => {
      useAppStore.getState().actions.setUi({ screen: 'settings' });
    });
    fireEvent.click(menuButton());
    expect(isDisabled('Undo')).toBe(true);
    expect(isDisabled('Redo')).toBe(true);
    fireEvent.click(menuButton());
    act(() => {
      useAppStore.getState().actions.setUi({ screen: 'editor' });
    });
    editor = null;
    fireEvent.click(menuButton());
    expect(isDisabled('Undo')).toBe(true);
    expect(isDisabled('Redo')).toBe(true);
  });

  it('has no accessibility problems with disabled items', async () => {
    registerAll([...ALL, 'project.rename', 'edit.undo', 'edit.redo']);
    act(() => {
      useAppStore.getState().actions.setProject(projectFixture());
      useAppStore.getState().actions.setUi({ screen: 'editor' });
    });
    const { container } = renderMenu(() => editorWithHistory(1, 0));
    fireEvent.click(menuButton());
    expect(isDisabled('Redo')).toBe(true);
    await expectNoAxeViolations(container);
  });

  it('has no accessibility problems, closed or open', async () => {
    registerAll();
    useAppStore.getState().actions.setProject(projectFixture());
    const { container } = renderMenu();
    await expectNoAxeViolations(container);
    fireEvent.click(menuButton());
    await expectNoAxeViolations(container);
  });
});
