/** The run gate as the commands check it, and the first-error command. */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { EditorHandle } from '../../app/editor-types';
import { resetAppStore, useAppStore } from '../../app/store';
import {
  diagnosticFixture,
  previewFixture,
  projectFixture,
  toolchainFixture,
} from '../../app/testing/fixtures';
import { focusFirstError, gateAllows } from './gate';
import { createFakeBackend, fakeDialogs, featureContext, openRunnableProject } from './testing';

beforeEach(() => {
  vi.useFakeTimers();
  resetAppStore();
});

afterEach(() => {
  vi.useRealTimers();
  document.body.replaceChildren();
});

describe('gateAllows', () => {
  it('allows a trusted project with a compiler and no errors', () => {
    openRunnableProject();
    const ctx = featureContext(createFakeBackend().ipc);
    expect(gateAllows('run', ctx)).toBe(true);
    expect(gateAllows('build', ctx)).toBe(true);
  });

  it('says that the compiler search is still running', () => {
    openRunnableProject();
    useAppStore.getState().actions.setToolchains({ list: [], discovering: true });
    const dialogs = fakeDialogs();
    const ctx = featureContext(createFakeBackend().ipc, {}, dialogs);
    expect(gateAllows('build', ctx)).toBe(false);
    expect(dialogs.alert).toHaveBeenCalledWith({
      title: 'No C++ compiler',
      message: 'Blocks2Cpp is still looking for a C++ compiler (g++). Try again in a moment.',
    });
  });

  it('runs the first-error command when there are errors', async () => {
    openRunnableProject();
    useAppStore.getState().actions.setAnalysis({ preview: previewFixture([diagnosticFixture()]) });
    const ctx = featureContext(createFakeBackend().ipc);
    const focus = vi.fn();
    ctx.commands.registerCommand('problems.focusFirstError', focus);
    expect(gateAllows('run', ctx)).toBe(false);
    await vi.advanceTimersByTimeAsync(0);
    expect(focus).toHaveBeenCalledTimes(1);
  });

  it('logs a first-error command that fails', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    openRunnableProject();
    useAppStore.getState().actions.setAnalysis({ preview: previewFixture([diagnosticFixture()]) });
    const ctx = featureContext(createFakeBackend().ipc);
    ctx.commands.registerCommand('problems.focusFirstError', () => {
      throw new Error('broken');
    });
    expect(gateAllows('run', ctx)).toBe(false);
    await vi.advanceTimersByTimeAsync(0);
    expect(error).toHaveBeenCalledWith(
      'Command problems.focusFirstError failed',
      expect.any(Error),
    );
  });
});

describe('focusFirstError', () => {
  /** A Problems tab panel with a grid whose tab stop is the first row's first cell. */
  function problemsPanel(withGrid: boolean): { panel: HTMLElement; cell: HTMLElement | null } {
    const panel = document.createElement('div');
    panel.dataset['dockPanel'] = 'problems';
    panel.tabIndex = 0;
    let cell: HTMLElement | null = null;
    if (withGrid) {
      const grid = document.createElement('div');
      grid.setAttribute('role', 'grid');
      cell = document.createElement('div');
      cell.tabIndex = 0;
      grid.append(cell);
      panel.append(grid);
    }
    document.body.append(panel);
    return { panel, cell };
  }

  it('opens Problems, selects the first error and focuses the grid', async () => {
    useAppStore.getState().actions.setProject(projectFixture());
    useAppStore.getState().actions.setToolchains({ list: [toolchainFixture()] });
    useAppStore.getState().actions.setAnalysis({
      preview: previewFixture([
        diagnosticFixture({ severity: 'warning', code: 'B2C-W0501' }),
        diagnosticFixture({ primary: { block: 'b042', part: { kind: 'whole' } } }),
      ]),
    });
    useAppStore.getState().actions.setUi({ screen: 'settings', bottomCollapsed: true });
    const { cell } = problemsPanel(true);
    const selectBlock = vi.fn();
    focusFirstError({
      store: useAppStore,
      editor: () => ({ selectBlock }) as unknown as EditorHandle,
    });
    expect(useAppStore.getState().ui).toMatchObject({
      screen: 'editor',
      bottomTab: 'problems',
      bottomCollapsed: false,
      selection: 'b042',
    });
    expect(selectBlock).toHaveBeenCalledWith('b042', { center: true });
    await vi.advanceTimersByTimeAsync(0);
    expect(document.activeElement).toBe(cell);
  });

  it('focuses the panel when it has no grid, and selects nothing for an error without a block', async () => {
    useAppStore.getState().actions.setProject(projectFixture());
    useAppStore.getState().actions.setBuild({
      diagnostics: [
        diagnosticFixture({
          code: 'C:link',
          source: 'linker',
          primary: { part: { kind: 'whole' } },
        }),
      ],
    });
    const { panel } = problemsPanel(false);
    const selectBlock = vi.fn();
    focusFirstError({
      store: useAppStore,
      editor: () => ({ selectBlock }) as unknown as EditorHandle,
    });
    expect(useAppStore.getState().ui.selection).toBeNull();
    expect(selectBlock).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(0);
    expect(document.activeElement).toBe(panel);
  });
});
