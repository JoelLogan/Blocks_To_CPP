/**
 * Autosave with fake timers and a fake backend: a recovery snapshot every 30 s only while the
 * project has unsaved changes, one when the window loses focus, one call at a time, and quiet
 * failures.
 */
import { MAX_DOCUMENT_BYTES } from '@blocks2cpp/b2c-core-wasm';
import type { IpcClient } from '@blocks2cpp/ipc-types';
import { afterEach, beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

import { resetAppStore, useAppStore } from '../../app/store';
import { projectFixture } from '../../app/testing/fixtures';
import {
  AUTOSAVE_INTERVAL_MS,
  type Autosave,
  exceedsDocumentLimit,
  startAutosave,
} from './autosave';
import { HANDLE_A, HANDLE_B, ipcFailure } from './testing';

type RecoverySave = Mock<IpcClient['recoverySave']>;

let recoverySave: RecoverySave;
let autosave: Autosave | null = null;
const blurTarget = new EventTarget();

function start(intervalMs?: number): Autosave {
  autosave = startAutosave({
    store: useAppStore,
    ipc: { recoverySave },
    window: blurTarget,
    ...(intervalMs === undefined ? {} : { intervalMs }),
  });
  return autosave;
}

function open(overrides: Parameters<typeof projectFixture>[0] = {}): void {
  useAppStore
    .getState()
    .actions.setProject(
      projectFixture({ handle: HANDLE_A, canonicalText: '{"v":0}', ...overrides }),
    );
}

/** An edit: new canonical text, unsaved. */
function edit(text: string): void {
  useAppStore.getState().actions.updateProject({ canonicalText: text, dirty: true });
}

function blur(): void {
  blurTarget.dispatchEvent(new Event('blur'));
}

beforeEach(() => {
  vi.useFakeTimers();
  resetAppStore();
  recoverySave = vi.fn<IpcClient['recoverySave']>(() => Promise.resolve({}));
});

afterEach(() => {
  autosave?.stop();
  autosave = null;
  vi.useRealTimers();
});

describe('autosave every 30 seconds', () => {
  it('writes snapshots only while the project has unsaved changes', async () => {
    open();
    start();
    await vi.advanceTimersByTimeAsync(3 * AUTOSAVE_INTERVAL_MS);
    expect(recoverySave).not.toHaveBeenCalled();

    edit('{"v":1}');
    await vi.advanceTimersByTimeAsync(AUTOSAVE_INTERVAL_MS - 1);
    expect(recoverySave).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(recoverySave).toHaveBeenCalledTimes(1);
    expect(recoverySave).toHaveBeenLastCalledWith({ handle: HANDLE_A, document: '{"v":1}' });

    edit('{"v":2}');
    await vi.advanceTimersByTimeAsync(AUTOSAVE_INTERVAL_MS);
    expect(recoverySave).toHaveBeenCalledTimes(2);
    expect(recoverySave).toHaveBeenLastCalledWith({ handle: HANDLE_A, document: '{"v":2}' });

    // Saved: no unsaved changes, so the timer stops.
    useAppStore.getState().actions.updateProject({
      dirty: false,
      savedCanonicalText: '{"v":2}',
      savedAt: '2026-10-05T11:00:00Z',
    });
    await vi.advanceTimersByTimeAsync(5 * AUTOSAVE_INTERVAL_MS);
    expect(recoverySave).toHaveBeenCalledTimes(2);
  });

  it('does not write the same text twice, but writes it again after a save', async () => {
    open({ dirty: true, canonicalText: '{"v":1}' });
    start();
    await vi.advanceTimersByTimeAsync(3 * AUTOSAVE_INTERVAL_MS);
    expect(recoverySave).toHaveBeenCalledTimes(1);

    // A save deletes the backend's snapshot; edits made during it keep the project dirty.
    useAppStore.getState().actions.updateProject({
      savedCanonicalText: '{"v":0}',
      savedAt: '2026-10-05T11:00:00Z',
    });
    await vi.advanceTimersByTimeAsync(AUTOSAVE_INTERVAL_MS);
    expect(recoverySave).toHaveBeenCalledTimes(2);
    expect(recoverySave).toHaveBeenLastCalledWith({ handle: HANDLE_A, document: '{"v":1}' });
  });

  it('starts again for another project, with its own handle', async () => {
    open({ dirty: true, canonicalText: '{"a":1}' });
    start();
    await vi.advanceTimersByTimeAsync(AUTOSAVE_INTERVAL_MS / 2);
    useAppStore
      .getState()
      .actions.setProject(
        projectFixture({ handle: HANDLE_B, dirty: true, canonicalText: '{"b":1}' }),
      );
    await vi.advanceTimersByTimeAsync(AUTOSAVE_INTERVAL_MS / 2);
    expect(recoverySave).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(AUTOSAVE_INTERVAL_MS / 2);
    expect(recoverySave).toHaveBeenCalledTimes(1);
    expect(recoverySave).toHaveBeenLastCalledWith({ handle: HANDLE_B, document: '{"b":1}' });

    // No project: nothing more.
    useAppStore.getState().actions.setProject(null);
    await vi.advanceTimersByTimeAsync(3 * AUTOSAVE_INTERVAL_MS);
    expect(recoverySave).toHaveBeenCalledTimes(1);
  });

  it('stops when it is stopped', async () => {
    open({ dirty: true });
    const running = start();
    running.stop();
    running.stop();
    blur();
    await vi.advanceTimersByTimeAsync(3 * AUTOSAVE_INTERVAL_MS);
    await running.snapshotNow();
    expect(recoverySave).not.toHaveBeenCalled();
  });

  it('refuses an interval that is not a positive number', () => {
    expect(() => start(0)).toThrow(RangeError);
    expect(() => start(Number.NaN)).toThrow(RangeError);
  });
});

describe('autosave when the window loses focus', () => {
  it('writes a snapshot at once while there are unsaved changes, and none otherwise', async () => {
    open();
    start();
    blur();
    await vi.advanceTimersByTimeAsync(0);
    expect(recoverySave).not.toHaveBeenCalled();

    edit('{"v":1}');
    blur();
    await vi.advanceTimersByTimeAsync(0);
    expect(recoverySave).toHaveBeenCalledTimes(1);
    expect(recoverySave).toHaveBeenLastCalledWith({ handle: HANDLE_A, document: '{"v":1}' });

    // Nothing new since: the timer's tick has nothing to write.
    await vi.advanceTimersByTimeAsync(AUTOSAVE_INTERVAL_MS);
    expect(recoverySave).toHaveBeenCalledTimes(1);
  });

  it('sends one snapshot at a time, the latest text after the one in progress', async () => {
    let finish: () => void = () => undefined;
    recoverySave.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = () => {
            resolve({});
          };
        }),
    );
    open();
    const running = start();
    edit('{"v":1}');
    blur();
    edit('{"v":2}');
    blur();
    edit('{"v":3}');
    const queued = running.snapshotNow();
    await vi.advanceTimersByTimeAsync(0);
    expect(recoverySave).toHaveBeenCalledTimes(1);

    finish();
    await queued;
    expect(recoverySave).toHaveBeenCalledTimes(2);
    expect(recoverySave).toHaveBeenLastCalledWith({ handle: HANDLE_A, document: '{"v":3}' });
  });
});

describe('autosave failures', () => {
  it('logs a run of the same failure once and tries again at the next tick', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    recoverySave.mockRejectedValue(ipcFailure({ code: 'io', kind: 'other' }, 'recovery_save'));
    open({ dirty: true });
    start();
    await vi.advanceTimersByTimeAsync(3 * AUTOSAVE_INTERVAL_MS);
    expect(recoverySave).toHaveBeenCalledTimes(3);
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn).toHaveBeenCalledWith('Autosave could not write a recovery snapshot', 'io');

    recoverySave.mockResolvedValue({});
    await vi.advanceTimersByTimeAsync(AUTOSAVE_INTERVAL_MS);
    expect(recoverySave).toHaveBeenCalledTimes(4);
    // Written now: the same text is not sent again.
    await vi.advanceTimersByTimeAsync(AUTOSAVE_INTERVAL_MS);
    expect(recoverySave).toHaveBeenCalledTimes(4);
    // Log messages never carry the project's content.
    for (const argument of warn.mock.calls.flat()) {
      expect(String(argument)).not.toContain('"v"');
    }
  });

  it('says nothing when the project was closed meanwhile', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    recoverySave.mockRejectedValue(ipcFailure({ code: 'unknownHandle' }, 'recovery_save'));
    open({ dirty: true });
    start();
    await vi.advanceTimersByTimeAsync(AUTOSAVE_INTERVAL_MS);
    expect(recoverySave).toHaveBeenCalledTimes(1);
    expect(warn).not.toHaveBeenCalled();
  });

  it('does not send a document larger than a project may be', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    open({ dirty: true, canonicalText: 'x'.repeat(MAX_DOCUMENT_BYTES + 1) });
    start();
    await vi.advanceTimersByTimeAsync(2 * AUTOSAVE_INTERVAL_MS);
    expect(recoverySave).not.toHaveBeenCalled();
    expect(warn).toHaveBeenCalledTimes(1);
  });
});

describe('exceedsDocumentLimit', () => {
  it('measures UTF-8 bytes, encoding only when the length alone cannot tell', () => {
    expect(exceedsDocumentLimit('')).toBe(false);
    expect(exceedsDocumentLimit('x'.repeat(MAX_DOCUMENT_BYTES))).toBe(false);
    expect(exceedsDocumentLimit('x'.repeat(MAX_DOCUMENT_BYTES + 1))).toBe(true);
    // Three bytes each in UTF-8: half the limit in characters is over it in bytes.
    const half = Math.ceil(MAX_DOCUMENT_BYTES / 2);
    expect(exceedsDocumentLimit('€'.repeat(half))).toBe(true);
    expect(exceedsDocumentLimit('€'.repeat(Math.floor(MAX_DOCUMENT_BYTES / 3)))).toBe(false);
  });
});
