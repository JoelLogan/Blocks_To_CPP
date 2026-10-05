/**
 * Test helpers for the project feature: the bundled templates, a feature context over a fake IPC
 * client, and answering the in-window dialogs. Only tests import this module.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import {
  type Diagnostic,
  type Handle,
  type IpcError,
  IpcCallError,
  type ProjectOpened,
  type RecentEntry,
  type Trust,
} from '@blocks2cpp/ipc-types';
import { vi } from 'vitest';

import { createCommandRegistry } from '../../app/commands';
import { getCore, setCore } from '../../app/core';
import { createDialogQueue, type DialogQueue, type DialogRequest } from '../../app/dialogs/service';
import type { EditorHandle } from '../../app/editor-types';
import { createAppEventBus } from '../../app/events';
import type { FeatureContext } from '../../app/features';
import { createScreenRegistry } from '../../app/screens';
import { resetAppStore, useAppStore } from '../../app/store';
import { appInfoFixture, createFakeIpc, type FakeIpc } from '../../app/testing/fixtures';
import { installProjectFeature, type InstalledProjectFeature } from './feature';
import { createStartPageSections, type StartPageSections } from './sections';

/** The backend's bundled templates (crates/b2c-app/templates), by file name. */
const TEMPLATE_FILES: Readonly<Record<string, string>> = import.meta.glob<string>(
  '../../../../../crates/b2c-app/templates/*.b2c',
  { query: '?raw', import: 'default', eager: true },
);

function templateFile(name: string): string {
  const text = TEMPLATE_FILES[`../../../../../crates/b2c-app/templates/${name}`];
  if (text === undefined) {
    throw new Error(`the template ${name} is missing`);
  }
  return text;
}

/** The *Empty* template, as `project_new` sends it. */
export const EMPTY_TEXT = templateFile('empty.b2c');

/** The *Hello World* template, as `project_new` sends it. */
export const HELLO_TEXT = templateFile('hello_world.b2c');

/** Handles the fake backend gives out. */
export const HANDLE_A: Handle = `ph_${'a'.repeat(32)}`;
export const HANDLE_B: Handle = `ph_${'b'.repeat(32)}`;
export const HANDLE_C: Handle = `ph_${'c'.repeat(32)}`;

/** A trusted project's trust. */
export const TRUSTED: Trust = {
  state: 'trusted',
  source: 'project',
  restrictedReason: null,
  markOfTheWeb: false,
};

/** A new project's trust. */
export const CREATED_HERE: Trust = { ...TRUSTED, source: 'createdHere' };

/** An IPC failure of `command` with `error`. */
export function ipcFailure(error: IpcError): IpcCallError {
  return new IpcCallError('project_open_dialog', error);
}

/** A loader diagnostic. */
export function loaderDiagnostic(code: string, message: string): Diagnostic {
  return {
    code,
    severity: 'error',
    message,
    primary: { part: { kind: 'whole' } },
    source: 'loader',
  };
}

/** What `project_open_dialog` or `project_open_recent` answers for a file. */
export function opened(document: string, overrides: Partial<ProjectOpened> = {}): ProjectOpened {
  return {
    handle: HANDLE_B,
    document,
    trust: TRUSTED,
    fileName: 'game.b2c',
    migratedFrom: null,
    ...overrides,
  };
}

/** A recent-list entry. */
export function recentEntry(index: number, overrides: Partial<RecentEntry> = {}): RecentEntry {
  return {
    recentId: `rc_${index.toString(16).padStart(32, '0')}`,
    projectName: `Project ${String(index)}`,
    displayPath: `/home/ada/projects/project-${String(index)}.b2c`,
    lastOpenedAt: '2026-10-05T10:42:00Z',
    ...overrides,
  };
}

/** A feature context over a fake backend, with the project feature installed. */
export interface Harness {
  readonly ctx: FeatureContext;
  readonly ipc: FakeIpc;
  readonly dialogs: DialogQueue;
  readonly sections: StartPageSections;
  readonly feature: InstalledProjectFeature;
  /** Makes `handle` the editor `ctx.editor()` returns. */
  setEditor(handle: EditorHandle | null): void;
  /** Uninstalls the feature, cancels open dialogs and forgets the core. */
  dispose(): void;
}

/**
 * Resets the app's store, publishes `core` and installs the project feature on a fresh context.
 * The fake backend answers the calls the feature makes in the background (`recent_list`,
 * `project_set_dirty`, `project_close`, `trust_get`, `app_quit`) with success; tests script the
 * rest.
 */
export function installHarness(core: CoreWasm | null): Harness {
  resetAppStore();
  useAppStore.getState().actions.setAppInfo(appInfoFixture());
  setCore(core);
  const ipc = createFakeIpc();
  ipc.recentList.mockResolvedValue({ entries: [] });
  ipc.projectSetDirty.mockResolvedValue({});
  ipc.projectClose.mockResolvedValue({});
  ipc.trustGet.mockResolvedValue({ trust: TRUSTED });
  ipc.appQuit.mockResolvedValue({});
  ipc.recentRemove.mockResolvedValue({});
  const dialogs = createDialogQueue();
  const sections = createStartPageSections();
  let editor: EditorHandle | null = null;
  const ctx: FeatureContext = {
    ipc,
    store: useAppStore,
    commands: createCommandRegistry(),
    screens: createScreenRegistry(),
    dialogs,
    events: createAppEventBus(),
    core: getCore,
    editor: () => editor,
  };
  const feature = installProjectFeature(ctx, { sections });
  return {
    ctx,
    ipc,
    dialogs,
    sections,
    feature,
    setEditor(handle) {
      editor = handle;
    },
    dispose() {
      feature.uninstall();
      dialogs.cancelAll();
      setCore(null);
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

/** Waits for a choice dialog and answers it with `id`; returns the request. */
export async function answerChoice(dialogs: DialogQueue, id: string): Promise<DialogRequest> {
  const request = await nextDialog(dialogs);
  if (request.kind !== 'choice') {
    throw new Error(`expected a choice dialog, got ${request.kind}`);
  }
  request.settle(id);
  return request;
}

/** Waits for a confirmation and answers it; returns the request. */
export async function answerConfirm(dialogs: DialogQueue, yes: boolean): Promise<DialogRequest> {
  const request = await nextDialog(dialogs);
  if (request.kind !== 'confirm') {
    throw new Error(`expected a confirmation, got ${request.kind}`);
  }
  request.settle(yes);
  return request;
}

/** Waits for an alert and closes it; returns the request. */
export async function closeAlert(dialogs: DialogQueue): Promise<DialogRequest> {
  const request = await nextDialog(dialogs);
  if (request.kind !== 'alert') {
    throw new Error(`expected an alert, got ${request.kind}`);
  }
  request.settle();
  return request;
}

/** Whether the built compiler core is available (CI sets `B2C_REQUIRE_WASM` and builds it). */
export const CORE_BUILT =
  Object.keys(import.meta.glob('../../../../../packages/b2c-core-wasm/pkg/b2c_core_wasm_bg.wasm'))
    .length > 0 || String(import.meta.env['B2C_REQUIRE_WASM'] ?? '') !== '';

let shared: Promise<CoreWasm> | null = null;

/** The real compiler core (started once per test file). */
export function realCore(): Promise<CoreWasm> {
  shared ??= import('@blocks2cpp/b2c-core-wasm').then(({ initCore }) => initCore());
  return shared;
}
