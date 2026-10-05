/**
 * Test helpers for the shell and the features: a scriptable fake of the IPC client and small
 * fixtures of the backend's types. Only tests import this module; it is never part of the app.
 */
import type { BdmBuildConfiguration, BdmDocument, PreviewResult } from '@blocks2cpp/b2c-core-wasm';
import {
  type AppEvent,
  type AppInfo,
  createIpcClient,
  type Diagnostic,
  IPC_VERSION,
  IpcCallError,
  type IpcClient,
  type Settings,
  type Toolchain,
} from '@blocks2cpp/ipc-types';
import { type Mock, vi } from 'vitest';

import type { ProjectState } from '../store/state';

/** The client's method names, from the generated client itself. */
const METHOD_NAMES = Object.keys(
  createIpcClient({ invoke: () => Promise.resolve(undefined), channel: () => undefined }),
) as (keyof IpcClient)[];

type MockedClient = { [K in keyof IpcClient]: Mock<IpcClient[K]> };

/** A fake IPC client: every method is a `vi.fn`, and `pushAppEvent` plays the backend's events. */
export interface FakeIpc extends MockedClient {
  /** Sends `event` (or any message) on the channel given to the last `appSubscribe`. */
  pushAppEvent(event: unknown): void;
}

/** What `app_info` reports by default. */
export function appInfoFixture(overrides: Partial<AppInfo> = {}): AppInfo {
  return {
    appVersion: '0.1.0',
    ipcVersion: IPC_VERSION,
    platform: 'linux',
    catalogVersion: '1.0.0',
    ...overrides,
  };
}

/** The default settings (05 §5.9). */
export function settingsFixture(overrides: Partial<Settings> = {}): Settings {
  return {
    formatVersion: 1,
    codeStyle: { indentWidth: 4 },
    run: { onErrors: 'disableRun' },
    console: { scrollbackLines: 10000 },
    toolchain: { selectedId: null },
    newProject: { standard: 'c++20' },
    buildCache: { maxBytes: 2 * 1024 * 1024 * 1024 },
    ...overrides,
  };
}

/** A usable, selected g++. */
export function toolchainFixture(overrides: Partial<Toolchain> = {}): Toolchain {
  return {
    id: 'tc_0123456789abcdef',
    version: '15.2.0',
    target: 'x86_64-w64-mingw32',
    flavor: 'MSYS2 UCRT64',
    displayPath: 'C:\\msys64\\ucrt64\\bin\\g++.exe',
    source: 'path',
    usable: true,
    selected: true,
    capabilities: {
      standards: ['c++17', 'c++20'],
      stdFormat: true,
      sanitizers: false,
      sarif: true,
    },
    problems: [],
    ...overrides,
  };
}

/** A diagnostic of the live analysis. */
export function diagnosticFixture(overrides: Partial<Diagnostic> = {}): Diagnostic {
  return {
    code: 'B2C-E0201',
    severity: 'error',
    message: 'This uses a variable that does not exist here.',
    primary: { module: 'mod_main', block: 'b007', part: { kind: 'whole' } },
    source: 'analyser',
    ...overrides,
  };
}

/** A preview whose only content is `diagnostics`. */
export function previewFixture(diagnostics: Diagnostic[] = []): PreviewResult {
  return {
    stage: diagnostics.some((diagnostic) => diagnostic.severity === 'error')
      ? 'analyze'
      : 'generate',
    diagnostics,
    files: [],
    sourceMap: null,
    buildable: diagnostics.length === 0,
    placeholders: 0,
    contentHash: 'a'.repeat(64),
    blockTypes: {},
    symbols: [],
  };
}

/** A small loaded document. */
export function documentFixture(name = 'Guessing Game'): BdmDocument {
  const configuration: BdmBuildConfiguration = {
    optimization: 'none',
    debugInfo: true,
    sanitizers: [],
    warnings: 'helpful',
    hardening: true,
  };
  return {
    format: 'blocks2cpp/project',
    formatVersion: 1,
    generator: { app: '0.1.0', catalog: '1.0.0' },
    project: {
      id: 'prj_fixture',
      name,
      language: { standard: 'c++20' },
      options: {
        showAdvanced: false,
        manualMemory: false,
        preferPlainStd: false,
        formattingStyle: 'stream',
        checkedIndexing: true,
      },
      build: { configurations: { debug: configuration, release: configuration } },
      run: { workingDirectory: 'project' },
    },
    modules: [{ id: 'mod_main', name: 'main', workspace: { blocks: [] } }],
  };
}

/** An open, saved, trusted project. */
export function projectFixture(overrides: Partial<ProjectState> = {}): ProjectState {
  return {
    handle: 'ph_0123456789abcdef0123456789abcdef',
    fileName: 'guessing-game.b2c',
    document: documentFixture(),
    canonicalText: '{}',
    contentHash: 'a'.repeat(64),
    savedCanonicalText: '{}',
    savedAt: '2026-10-05T10:42:00Z',
    dirty: false,
    trust: { state: 'trusted', source: 'project', restrictedReason: null, markOfTheWeb: false },
    activeModuleId: 'mod_main',
    migratedFrom: null,
    ...overrides,
  };
}

/**
 * A fake client. By default `appInfo`, `appSubscribe`, `settingsGet` and `toolchainList` answer
 * like a healthy backend with no toolchain; every other method rejects with an `internal` error
 * until a test scripts it.
 */
export function createFakeIpc(): FakeIpc {
  let onAppEvent: ((event: AppEvent) => void) | null = null;
  const fake: Record<string, Mock> = {};
  for (const name of METHOD_NAMES) {
    fake[name] = vi.fn(() => Promise.reject(new IpcCallError('app_info', { code: 'internal' })));
  }
  const client = fake as unknown as FakeIpc;
  client.appInfo.mockImplementation(() => Promise.resolve(appInfoFixture()));
  client.appSubscribe.mockImplementation((handler) => {
    onAppEvent = handler;
    return Promise.resolve({});
  });
  client.settingsGet.mockImplementation(() =>
    Promise.resolve({ settings: settingsFixture(), notices: [] }),
  );
  client.toolchainList.mockImplementation(() =>
    Promise.resolve({ toolchains: [], discovering: false }),
  );
  client.pushAppEvent = (event) => {
    if (onAppEvent === null) {
      throw new Error('nothing subscribed to app events');
    }
    onAppEvent(event as AppEvent);
  };
  return client;
}
