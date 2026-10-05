/**
 * The shipped app runs with Tauri's `freezePrototype: true`, which freezes `Object.prototype`
 * before any app script runs (docs/spec/08-security.md §8.8). Libraries that assign to inherited
 * properties (`obj.toString = …`) break under it. This file freezes the prototype first and only
 * then loads the shell's state, dialogs, hints and tabs (Zustand and Radix), so such a break shows
 * up here, before the end-to-end tests run the release build. Vitest runs each test file in its
 * own isolated environment, so the freeze does not leak into other files.
 */
import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeAll, describe, expect, it, vi } from 'vitest';

vi.mock('blockly/core', () => {
  const workspace = { setTheme: () => undefined, dispose: () => undefined };
  return {
    inject: () => workspace,
    svgResize: () => undefined,
    setLocale: () => undefined,
    Theme: { defineTheme: (name: string) => ({ name }) },
    Themes: { Zelos: { name: 'zelos' } },
  };
});
vi.mock('blockly/msg/en', () => ({}));
// The block editor (Blockly itself) is tested under the frozen prototype in
// src/editor/frozen-prototype.test.tsx; here it is left out with Blockly.
vi.mock('../editor/EditorWorkspace', () => ({ EditorWorkspace: () => null }));

beforeAll(() => {
  Object.freeze(Object.prototype);
  window.matchMedia = () =>
    ({
      matches: false,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      // xterm.js 5.5 (the console, shown while a project is open) uses the older listener API.
      addListener: () => undefined,
      removeListener: () => undefined,
    }) as unknown as MediaQueryList;
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe(): void {
        // happy-dom has no layout to observe.
      }
      unobserve(): void {
        // Nothing observed.
      }
      disconnect(): void {
        // Nothing observed.
      }
    },
  );
});

describe('with a frozen Object.prototype', () => {
  it('the store, the main window, its hints and tabs, and the dialogs work', async () => {
    expect(Object.isFrozen(Object.prototype)).toBe(true);
    const { useAppStore } = await import('./store');
    const { App } = await import('./App');
    const { HintProvider } = await import('./ui/Hint');
    const { createDialogQueue, DialogHost } = await import('./dialogs');
    const { projectFixture, toolchainFixture } = await import('./testing/fixtures');

    const { actions } = useAppStore.getState();
    actions.setProject(projectFixture());
    actions.setToolchains({ list: [toolchainFixture()] });
    const queue = createDialogQueue();
    render(
      <HintProvider>
        <App />
        <DialogHost queue={queue} />
      </HintProvider>,
    );

    fireEvent.mouseDown(screen.getByRole('tab', { name: 'Problems (0)' }));
    expect(useAppStore.getState().ui.bottomTab).toBe('problems');

    act(() => {
      screen.getByTestId('toolbar-run').focus();
    });
    expect((await screen.findByRole('tooltip')).textContent).toBe('Build if needed, then run (F5)');

    let answer!: Promise<string | null>;
    await act(async () => {
      answer = queue.prompt({ message: 'Name?', defaultValue: 'x' });
      await Promise.resolve();
    });
    fireEvent.change(screen.getByRole('textbox', { name: 'Name?' }), {
      target: { value: 'total' },
    });
    fireEvent.click(screen.getByRole('button', { name: 'OK' }));
    await expect(answer).resolves.toBe('total');
  });
});
