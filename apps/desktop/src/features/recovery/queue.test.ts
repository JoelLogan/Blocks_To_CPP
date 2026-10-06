/**
 * Restore and autosave in the project lifecycle's queue, with the real project feature and
 * compiler core: a restore and a start page action never overlap (a template click during a restore
 * asks about the restored work instead of closing it), the open project's unsaved changes are
 * settled by the lifecycle itself, and a snapshot never crosses a save or a reload.
 *
 * Without a build of the compiler core these tests are skipped, unless B2C_REQUIRE_WASM is set
 * (as in CI), in which case a missing build fails them.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import type { RecoveryRestoreResponse } from '@blocks2cpp/ipc-types';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { useAppStore } from '../../app/store';
import {
  answerChoice,
  closeAlert,
  CORE_BUILT,
  CREATED_HERE,
  HANDLE_A,
  HANDLE_C,
  HELLO_TEXT,
  type Harness,
  installHarness,
  ipcFailure,
  opened,
  realCore,
} from '../project/testing';
import { type InstalledRecoveryFeature, installRecoveryFeature } from './feature';
import { snapshotInfo } from './testing';

/** The restored work: Hello World, renamed. */
const RESTORED_TEXT = HELLO_TEXT.replace('"Hello World"', '"Restored work"');

let core: CoreWasm;
let harness: Harness;
let recovery: InstalledRecoveryFeature;

beforeAll(async () => {
  if (CORE_BUILT) {
    core = await realCore();
  }
});

beforeEach(() => {
  harness = installHarness(CORE_BUILT ? core : null);
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
  harness.ipc.recoveryList.mockResolvedValue({ snapshots: [snapshotInfo(1)] });
  harness.ipc.recoverySave.mockResolvedValue({});
  recovery = installRecoveryFeature(harness.ctx, {
    window: null,
    project: harness.feature.lifecycle,
  });
});

afterEach(() => {
  recovery.uninstall();
  harness.dispose();
});

/** A promise and the function that resolves it. */
function deferred<T>(): { promise: Promise<T>; resolve: (value: T) => void } {
  let resolve: (value: T) => void = () => undefined;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

/** Opens Hello World from a file under `handle`, then marks it changed. */
async function openChanged(handle = HANDLE_C): Promise<void> {
  harness.ipc.projectOpenDialog.mockResolvedValueOnce({
    status: 'ok',
    ...opened(HELLO_TEXT, { handle, fileName: 'game.b2c' }),
  });
  expect(await harness.feature.lifecycle.open()).toBe(true);
  edit('Changed');
}

/** An edit, as the live preview commits it. */
function edit(name: string): void {
  const project = useAppStore.getState().project;
  if (project === null) {
    throw new Error('no project is open');
  }
  const changed = structuredClone(project.document);
  changed.project.name = name;
  const canonical = core.canonical(JSON.stringify(changed));
  if (!canonical.ok) {
    throw new Error('the edited document does not load');
  }
  useAppStore.getState().actions.updateProject({
    document: changed,
    canonicalText: canonical.text,
    contentHash: canonical.hash,
    dirty: canonical.text !== project.savedCanonicalText,
  });
}

describe.skipIf(!CORE_BUILT)('restore in the project lifecycle', () => {
  it('makes a template click during a restore wait, then ask about the restored work', async () => {
    await vi.waitFor(() => {
      expect(recovery.model.getState().snapshots).toHaveLength(1);
    });
    const restore = deferred<RecoveryRestoreResponse>();
    harness.ipc.recoveryRestore.mockReturnValueOnce(restore.promise);
    const restoring = recovery.controller.restore(snapshotInfo(1).snapshotId);
    await vi.waitFor(() => {
      expect(harness.ipc.recoveryRestore).toHaveBeenCalled();
    });
    // The start page holds its buttons back and says what runs.
    expect(harness.feature.model.getState().busy).toBe('restore');

    // The user clicks Hello World anyway (or presses Ctrl+N): it waits for the restore.
    harness.ipc.projectNew.mockResolvedValueOnce({
      handle: HANDLE_C,
      document: HELLO_TEXT,
      trust: CREATED_HERE,
    });
    const creating = harness.feature.lifecycle.newProject('helloWorld');
    await Promise.resolve();
    expect(harness.ipc.projectNew).not.toHaveBeenCalled();

    restore.resolve({
      handle: HANDLE_A,
      document: RESTORED_TEXT,
      trust: CREATED_HERE,
      fileName: null,
    });
    expect(await restoring).toBe(true);
    expect(useAppStore.getState().project).toMatchObject({ handle: HANDLE_A, dirty: true });

    // Then the new project asks about the restored, unsaved work; Cancel keeps it.
    const question = await answerChoice(harness.dialogs, 'cancel');
    expect(question.options.title).toBe('Save changes to “Restored work”?');
    expect(await creating).toBe(false);
    expect(harness.ipc.projectNew).not.toHaveBeenCalled();
    expect(harness.ipc.projectClose).not.toHaveBeenCalledWith({ handle: HANDLE_A });
    expect(useAppStore.getState().project?.handle).toBe(HANDLE_A);
  });

  it("saves the open project first when asked to, with the lifecycle's own save", async () => {
    await openChanged();
    await vi.waitFor(() => {
      expect(recovery.model.getState().snapshots).toHaveLength(1);
    });
    harness.ipc.projectSave.mockResolvedValueOnce({
      savedAt: '2026-10-05T10:42:00Z',
      hash: 'f'.repeat(64),
    });
    harness.ipc.recoveryRestore.mockResolvedValueOnce({
      handle: HANDLE_A,
      document: RESTORED_TEXT,
      trust: CREATED_HERE,
      fileName: null,
    });

    const restoring = recovery.controller.restore(snapshotInfo(1).snapshotId);
    await answerChoice(harness.dialogs, 'save');
    expect(await restoring).toBe(true);

    expect(harness.ipc.projectSave).toHaveBeenCalledWith(
      expect.objectContaining({ handle: HANDLE_C }),
    );
    expect(useAppStore.getState().project?.handle).toBe(HANDLE_A);
    // The saved project is closed only after the restored one is shown.
    expect(harness.ipc.projectClose).toHaveBeenCalledWith({ handle: HANDLE_C });
  });

  it('restores nothing when that save fails, and keeps the snapshot offered', async () => {
    await openChanged();
    await vi.waitFor(() => {
      expect(recovery.model.getState().snapshots).toHaveLength(1);
    });
    harness.ipc.projectSave.mockRejectedValueOnce(ipcFailure({ code: 'io', kind: 'other' }));

    const restoring = recovery.controller.restore(snapshotInfo(1).snapshotId);
    await answerChoice(harness.dialogs, 'save');
    await closeAlert(harness.dialogs);
    expect(await restoring).toBe(false);

    expect(harness.ipc.recoveryRestore).not.toHaveBeenCalled();
    expect(recovery.model.getState().snapshots).toHaveLength(1);
    expect(useAppStore.getState().project?.handle).toBe(HANDLE_C);
    expect(harness.ipc.projectClose).not.toHaveBeenCalled();
  });
});

describe.skipIf(!CORE_BUILT)('autosave and saving', () => {
  it('lets a save wait for the snapshot being written, and writes none while it saves', async () => {
    await openChanged();
    const order: string[] = [];
    const snapshot = deferred<Record<string, never>>();
    harness.ipc.recoverySave.mockImplementation(() => {
      order.push('recovery_save');
      return Promise.resolve({});
    });
    harness.ipc.recoverySave.mockImplementationOnce(() => {
      order.push('recovery_save');
      return snapshot.promise;
    });
    const save = deferred<{ savedAt: string; hash: string }>();
    harness.ipc.projectSave.mockImplementationOnce(() => {
      order.push('project_save');
      return save.promise;
    });

    // A snapshot of the change is on its way when the user presses Ctrl+S.
    void recovery.autosave.snapshotNow();
    const saving = harness.feature.lifecycle.save();
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(order).toEqual(['recovery_save']);

    // The save goes out only once the snapshot is written (the backend then deletes it) ...
    snapshot.resolve({});
    await vi.waitFor(() => {
      expect(order).toEqual(['recovery_save', 'project_save']);
    });
    // ... and while it is on its way, no snapshot starts, not even of newer edits.
    edit('Changed again');
    await recovery.autosave.snapshotNow();
    expect(harness.ipc.recoverySave).toHaveBeenCalledTimes(1);

    save.resolve({ savedAt: '2026-10-05T10:42:00Z', hash: 'f'.repeat(64) });
    expect(await saving).toBe(true);
    // The edit made meanwhile is still unsaved, so autosave writes it again afterwards.
    expect(useAppStore.getState().project?.dirty).toBe(true);
    await recovery.autosave.snapshotNow();
    expect(harness.ipc.recoverySave).toHaveBeenCalledTimes(2);
    expect(order).toEqual(['recovery_save', 'project_save', 'recovery_save']);
  });
  it('lets a reload wait for the snapshot being written, and writes none while it reloads', async () => {
    await openChanged();
    const order: string[] = [];
    const snapshot = deferred<Record<string, never>>();
    harness.ipc.recoverySave.mockImplementation(() => {
      order.push('recovery_save');
      return Promise.resolve({});
    });
    harness.ipc.recoverySave.mockImplementationOnce(() => {
      order.push('recovery_save');
      return snapshot.promise;
    });
    const reloaded = deferred<boolean>();

    // A snapshot of the change is on its way when the user chooses Reload.
    void recovery.autosave.snapshotNow();
    const reloading = harness.feature.lifecycle.reloadWith(() => {
      order.push('project_reload');
      return reloaded.promise;
    });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(order).toEqual(['recovery_save']);

    // The reload goes out only once the snapshot is written (the backend then deletes it) ...
    snapshot.resolve({});
    await vi.waitFor(() => {
      expect(order).toEqual(['recovery_save', 'project_reload']);
    });
    // ... and while it is on its way, no snapshot of the canvas being replaced starts.
    edit('Changed again');
    await recovery.autosave.snapshotNow();
    expect(harness.ipc.recoverySave).toHaveBeenCalledTimes(1);

    reloaded.resolve(true);
    expect(await reloading).toBe(true);
    expect(order).toEqual(['recovery_save', 'project_reload']);
  });
});
