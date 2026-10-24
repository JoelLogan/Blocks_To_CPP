/**
 * Reporting the dirty flag to the backend (`project_set_dirty`): only changes are sent, one call
 * at a time, always ending with the latest value.
 */
import type { IpcClient } from '@blocks2cpp/ipc-types';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { resetAppStore, useAppStore } from '../../app/store';
import { createFakeIpc, type FakeIpc, projectFixture } from '../../app/testing/fixtures';
import { reportDirtyState } from './dirty';
import { HANDLE_A, HANDLE_B, ipcFailure } from './testing';

let ipc: FakeIpc;
let stop: () => void;

/** Lets pending promise callbacks run. */
async function settle(): Promise<void> {
  for (let i = 0; i < 5; i += 1) {
    await Promise.resolve();
  }
}

function setDirty(dirty: boolean): void {
  useAppStore.getState().actions.updateProject({ dirty });
}

beforeEach(() => {
  resetAppStore();
  ipc = createFakeIpc();
  ipc.projectSetDirty.mockResolvedValue({});
  stop = () => undefined;
});

afterEach(() => {
  stop();
});

function start(client: IpcClient = ipc): void {
  stop = reportDirtyState(useAppStore, client);
}

describe('reporting unsaved changes', () => {
  it('sends nothing for a clean project, then each change', async () => {
    start();
    useAppStore.getState().actions.setProject(projectFixture({ handle: HANDLE_A }));
    await settle();
    expect(ipc.projectSetDirty).not.toHaveBeenCalled();

    setDirty(true);
    await settle();
    setDirty(false);
    await settle();
    expect(ipc.projectSetDirty.mock.calls).toEqual([
      [{ handle: HANDLE_A, dirty: true }],
      [{ handle: HANDLE_A, dirty: false }],
    ]);
  });

  it('sends a project that is dirty from the start, also when installed after it opened', async () => {
    useAppStore.getState().actions.setProject(projectFixture({ handle: HANDLE_A, dirty: true }));
    start();
    await settle();
    expect(ipc.projectSetDirty).toHaveBeenCalledExactlyOnceWith({ handle: HANDLE_A, dirty: true });
  });

  it('sends one call at a time and ends with the latest value', async () => {
    let finish: () => void = () => undefined;
    ipc.projectSetDirty.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = () => {
            resolve({});
          };
        }),
    );
    start();
    useAppStore.getState().actions.setProject(projectFixture({ handle: HANDLE_A }));
    setDirty(true);
    setDirty(false);
    setDirty(true);
    setDirty(false);
    await settle();
    expect(ipc.projectSetDirty).toHaveBeenCalledOnce();
    finish();
    await settle();
    // The backend was told `true`; the latest value is `false`.
    expect(ipc.projectSetDirty.mock.calls).toEqual([
      [{ handle: HANDLE_A, dirty: true }],
      [{ handle: HANDLE_A, dirty: false }],
    ]);
  });

  it('starts again for each new handle', async () => {
    start();
    useAppStore.getState().actions.setProject(projectFixture({ handle: HANDLE_A, dirty: true }));
    await settle();
    useAppStore.getState().actions.setProject(projectFixture({ handle: HANDLE_B }));
    await settle();
    setDirty(true);
    await settle();
    expect(ipc.projectSetDirty.mock.calls).toEqual([
      [{ handle: HANDLE_A, dirty: true }],
      [{ handle: HANDLE_B, dirty: true }],
    ]);
  });

  it('logs a failure and sends the next change', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    ipc.projectSetDirty.mockRejectedValueOnce(ipcFailure({ code: 'unknownHandle' }));
    start();
    useAppStore.getState().actions.setProject(projectFixture({ handle: HANDLE_A }));
    setDirty(true);
    await settle();
    expect(warn).toHaveBeenCalledWith(
      'Could not report unsaved changes to the backend',
      'unknownHandle',
    );
    setDirty(false);
    await settle();
    expect(ipc.projectSetDirty).toHaveBeenCalledTimes(2);
  });

  it('stops when uninstalled', async () => {
    start();
    useAppStore.getState().actions.setProject(projectFixture({ handle: HANDLE_A }));
    stop();
    setDirty(true);
    await settle();
    expect(ipc.projectSetDirty).not.toHaveBeenCalled();
  });

  it('sends nothing after the project closed', async () => {
    start();
    useAppStore.getState().actions.setProject(projectFixture({ handle: HANDLE_A }));
    useAppStore.getState().actions.setProject(null);
    await settle();
    expect(ipc.projectSetDirty).not.toHaveBeenCalled();
  });
});
