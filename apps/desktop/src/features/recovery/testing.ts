/**
 * Test helpers for the recovery and external-change features: a feature context over a fake
 * backend, fixtures of their IPC types, and answering the in-window dialogs. Only tests import
 * this module; it is never part of the app.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import {
  type CommandName,
  type Handle,
  IpcCallError,
  type IpcError,
  type SnapshotId,
  type SnapshotInfo,
  type Trust,
} from '@blocks2cpp/ipc-types';
import { type Mock, vi } from 'vitest';

import { type CommandRegistry, createCommandRegistry } from '../../app/commands';
import { createDialogQueue, type DialogQueue, type DialogRequest } from '../../app/dialogs/service';
import type { EditorHandle } from '../../app/editor-types';
import { type AppEventBus, createAppEventBus } from '../../app/events';
import type { FeatureContext } from '../../app/features';
import { createScreenRegistry } from '../../app/screens';
import { resetAppStore, useAppStore } from '../../app/store';
import { appInfoFixture, createFakeIpc, type FakeIpc } from '../../app/testing/fixtures';
import type { OpenDocumentArgs, OpenDocumentResult } from '../../editor/load';

/** Handles the fake backend gives out. */
export const HANDLE_A: Handle = `ph_${'a'.repeat(32)}`;
export const HANDLE_B: Handle = `ph_${'b'.repeat(32)}`;

/** A trusted project's trust. */
export const TRUSTED: Trust = {
  state: 'trusted',
  source: 'project',
  restrictedReason: null,
  markOfTheWeb: false,
};

/** A new project's trust. */
export const CREATED_HERE: Trust = { ...TRUSTED, source: 'createdHere' };

/** Restricted Mode after an outside change to trust-relevant content (08 §8.3). */
export const CHANGED_OUTSIDE: Trust = {
  state: 'restricted',
  source: null,
  restrictedReason: 'changedOutside',
  markOfTheWeb: false,
};

/** A snapshot ID made from `n`. */
export function snapshotId(n: number): SnapshotId {
  return `sn_${n.toString(16).padStart(32, '0')}`;
}

/** A snapshot offered for restore. */
export function snapshotInfo(n: number, overrides: Partial<SnapshotInfo> = {}): SnapshotInfo {
  return {
    snapshotId: snapshotId(n),
    projectName: `Project ${String(n)}`,
    savedAt: '2026-10-05T10:42:00Z',
    hasPath: true,
    ...overrides,
  };
}

/** An IPC failure of `command` with `error`. */
export function ipcFailure(error: IpcError, command: CommandName = 'app_info'): IpcCallError {
  return new IpcCallError(command, error);
}

/** The `openDocumentInEditor` stand-in and its mock. */
export type OpenInEditorMock = Mock<
  (ctx: FeatureContext, args: OpenDocumentArgs) => Promise<OpenDocumentResult>
>;

/** A feature context over a fake backend. */
export interface Harness {
  readonly ctx: FeatureContext;
  readonly ipc: FakeIpc;
  readonly dialogs: DialogQueue;
  readonly commands: CommandRegistry;
  readonly events: AppEventBus;
  /** Makes `handle` the editor `ctx.editor()` returns. */
  setEditor(handle: EditorHandle | null): void;
  /** Cancels open dialogs. */
  dispose(): void;
}

/**
 * Resets the app's store and builds a feature context over a fake backend. `recovery_list`
 * answers with no snapshots, and `recovery_save`, `project_close` and `project_set_dirty`
 * succeed; tests script the rest.
 */
export function createHarness(core: CoreWasm | null = null): Harness {
  resetAppStore();
  useAppStore.getState().actions.setAppInfo(appInfoFixture());
  const ipc = createFakeIpc();
  ipc.recoveryList.mockResolvedValue({ snapshots: [] });
  ipc.recoverySave.mockResolvedValue({});
  ipc.projectClose.mockResolvedValue({});
  ipc.projectSetDirty.mockResolvedValue({});
  const dialogs = createDialogQueue();
  const commands = createCommandRegistry();
  const events = createAppEventBus();
  let editor: EditorHandle | null = null;
  const ctx: FeatureContext = {
    ipc,
    store: useAppStore,
    commands,
    screens: createScreenRegistry(),
    dialogs,
    events,
    core: () => core,
    editor: () => editor,
  };
  return {
    ctx,
    ipc,
    dialogs,
    commands,
    events,
    setEditor(handle) {
      editor = handle;
    },
    dispose() {
      dialogs.cancelAll();
    },
  };
}

/** Waits until a dialog is shown and returns it (without answering). */
export async function nextDialog(dialogs: DialogQueue): Promise<DialogRequest> {
  return vi.waitFor(() => {
    const request = dialogs.store.getState().queue[0];
    if (request === undefined) {
      throw new Error('no dialog is shown');
    }
    return request;
  });
}

/** Waits for a choice dialog and returns it (without answering). */
export async function nextChoice(
  dialogs: DialogQueue,
): Promise<Extract<DialogRequest, { kind: 'choice' }>> {
  const request = await nextDialog(dialogs);
  if (request.kind !== 'choice') {
    throw new Error(`expected a choice dialog, got ${request.kind}`);
  }
  return request;
}

/** Waits for a choice dialog and answers it with `id`; returns the request. */
export async function answerChoice(
  dialogs: DialogQueue,
  id: string,
): Promise<Extract<DialogRequest, { kind: 'choice' }>> {
  const request = await nextChoice(dialogs);
  request.settle(id);
  return request;
}

/** Waits for a confirmation and answers it; returns the request. */
export async function answerConfirm(
  dialogs: DialogQueue,
  yes: boolean,
): Promise<Extract<DialogRequest, { kind: 'confirm' }>> {
  const request = await nextDialog(dialogs);
  if (request.kind !== 'confirm') {
    throw new Error(`expected a confirmation, got ${request.kind}`);
  }
  request.settle(yes);
  return request;
}

/** Waits for an alert and closes it; returns the request. */
export async function closeAlert(
  dialogs: DialogQueue,
): Promise<Extract<DialogRequest, { kind: 'alert' }>> {
  const request = await nextDialog(dialogs);
  if (request.kind !== 'alert') {
    throw new Error(`expected an alert, got ${request.kind}`);
  }
  request.settle();
  return request;
}

/** The IDs of a choice dialog's answers, in display order. */
export function choiceIds(request: Extract<DialogRequest, { kind: 'choice' }>): string[] {
  return request.options.choices.map((choice) => choice.id);
}
