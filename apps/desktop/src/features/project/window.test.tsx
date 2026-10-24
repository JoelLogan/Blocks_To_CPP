/**
 * The project feature in the whole window (Root, the toolbar with its main menu, the shortcuts,
 * the dialog host), started like the app starts: the start page first, a project opened from it,
 * the dirty marker in the top bar, `Ctrl+S`, closing from the menu, and the window's close request
 * answered with *Don't save*. The block editor is replaced by a stand-in (it is tested with the
 * real Blockly in editor.test.ts).
 *
 * Without a build of the compiler core these tests are skipped, unless B2C_REQUIRE_WASM is set
 * (as in CI), in which case a missing build fails them.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { type AppRuntime, createAppRuntime } from '../../app/bootstrap';
import { commands } from '../../app/commands';
import { setCore } from '../../app/core';
import { createDialogQueue } from '../../app/dialogs';
import { installAll } from '../../app/features';
import { Root } from '../../app/Root';
import { screens } from '../../app/screens';
import { resetAppStore, useAppStore } from '../../app/store';
import { createFakeIpc, type FakeIpc } from '../../app/testing/fixtures';
import { expectNoAxeViolations } from '../../test/axe';
import { projectFeature } from './feature';
import { CORE_BUILT, HANDLE_A, HELLO_TEXT, opened, realCore, recentEntry } from './testing';

vi.mock('../../editor/EditorWorkspace', () => ({
  EditorWorkspace: () => <div className="blockly-host" />,
}));

let core: CoreWasm;
let ipc: FakeIpc;
let runtime: AppRuntime;

beforeAll(async () => {
  if (CORE_BUILT) {
    core = await realCore();
  }
});

beforeEach(() => {
  resetAppStore();
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe(): void {
        // happy-dom has no layout.
      }
      unobserve(): void {
        // Not needed.
      }
      disconnect(): void {
        // Not needed.
      }
    },
  );
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
  ipc = createFakeIpc();
  ipc.recentList.mockResolvedValue({ entries: [recentEntry(1, { projectName: 'Old game' })] });
  ipc.projectSetDirty.mockResolvedValue({});
  ipc.projectClose.mockResolvedValue({});
  ipc.appQuit.mockResolvedValue({});
  ipc.projectSave.mockResolvedValue({ savedAt: '2026-10-05T10:42:00Z', hash: 'f'.repeat(64) });
  runtime = createAppRuntime({
    ipc,
    store: useAppStore,
    commands,
    screens,
    dialogs: createDialogQueue(),
    installFeatures: (ctx) => installAll([projectFeature], ctx),
    installBlocklyDialogs: () => () => undefined,
    window,
  });
});

afterEach(() => {
  runtime.stop();
  setCore(null);
});

async function startWindow() {
  const view = render(<Root runtime={runtime} />);
  await act(async () => {
    await runtime.start();
  });
  return view;
}

describe.skipIf(!CORE_BUILT)('the project feature in the window', () => {
  beforeEach(() => {
    setCore(core);
  });

  it('starts on the start page, opens a project, saves it and closes it', async () => {
    await startWindow();
    const page = screen.getByRole('main', { name: 'Start page' });
    expect(within(page).getByRole('heading', { level: 2, name: 'Start' })).toBeTruthy();
    expect(await within(page).findByRole('button', { name: 'Old game' })).toBeTruthy();
    // happy-dom applies no style to the `hidden` editor's `main`, so axe counts two main landmarks
    // in the window; in a browser the hidden one is not displayed.
    await expectNoAxeViolations(page, { skipRules: ['landmark-no-duplicate-main'] });

    // Open… on the start page.
    ipc.projectOpenDialog.mockResolvedValueOnce({
      status: 'ok',
      ...opened(HELLO_TEXT, { handle: HANDLE_A }),
    });
    fireEvent.click(within(page).getByRole('button', { name: 'Open…' }));
    await waitFor(() => {
      expect(screen.queryByRole('main', { name: 'Start page' })).toBeNull();
    });
    expect(screen.getByTestId('project-name').textContent).toBe('Hello World');

    // An edit shows the dirty marker in the top bar (no OS title change).
    act(() => {
      useAppStore.getState().actions.updateProject({ dirty: true });
    });
    expect(screen.getByTestId('project-name').textContent).toBe('Hello World • (unsaved changes)');

    // Ctrl+S saves.
    fireEvent.keyDown(window, { key: 's', ctrlKey: true });
    await waitFor(() => {
      expect(ipc.projectSave).toHaveBeenCalledWith({ handle: HANDLE_A, document: HELLO_TEXT });
    });
    await waitFor(() => {
      expect(screen.getByTestId('project-name').textContent).toBe('Hello World');
    });

    // The main menu closes the project.
    fireEvent.click(screen.getByRole('button', { name: 'Main menu' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Close project' }));
    await waitFor(() => {
      expect(screen.getByRole('main', { name: 'Start page' })).toBeTruthy();
    });
    expect(ipc.projectClose).toHaveBeenCalledWith({ handle: HANDLE_A });
    expect(screen.queryByTestId('project-name')).toBeNull();
  });

  it("answers the window's close request: Don't save closes the project, then quits", async () => {
    await startWindow();
    ipc.projectOpenDialog.mockResolvedValueOnce({
      status: 'ok',
      ...opened(HELLO_TEXT, { handle: HANDLE_A }),
    });
    await act(async () => {
      await commands.runCommand('project.open');
    });
    act(() => {
      useAppStore.getState().actions.updateProject({ dirty: true });
    });

    act(() => {
      ipc.pushAppEvent({ kind: 'closeRequested' });
    });
    const dialog = await screen.findByRole('dialog', { name: 'Save changes to “Hello World”?' });
    fireEvent.click(within(dialog).getByRole('button', { name: "Don't save" }));

    await waitFor(() => {
      expect(ipc.appQuit).toHaveBeenCalledOnce();
    });
    expect(ipc.projectSave).not.toHaveBeenCalled();
    expect(ipc.projectClose).toHaveBeenCalledWith({ handle: HANDLE_A });
  });
});
