/**
 * The toolbar's main menu: it lists only registered commands (and the project items only with a
 * project), follows the WAI-ARIA menu button keyboard pattern, runs the chosen command and passes
 * axe-core.
 */
import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { expectNoAxeViolations } from '../../test/axe';
import { type CommandId, type CommandRegistry, createCommandRegistry } from '../commands';
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

function renderMenu() {
  return render(
    <header>
      <MainMenu registry={registry} />
      <button type="button">Elsewhere</button>
    </header>,
  );
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

  it('has no accessibility problems, closed or open', async () => {
    registerAll();
    useAppStore.getState().actions.setProject(projectFixture());
    const { container } = renderMenu();
    await expectNoAxeViolations(container);
    fireEvent.click(menuButton());
    await expectNoAxeViolations(container);
  });
});
