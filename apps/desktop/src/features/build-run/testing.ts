/**
 * Test helpers for the build and run feature: a console whose writes the test controls, a fake
 * backend that records every build and run and lets the test play their channels, and a feature
 * context over the app's store. Only tests import this module.
 */
import type {
  BuildEvent,
  BuildId,
  BuildStartRequest,
  RunEvent,
  RunId,
  RunStartRequest,
} from '@blocks2cpp/ipc-types';
import { type Mock, vi } from 'vitest';

import { createCommandRegistry } from '../../app/commands';
import type { ChoiceOptions, DialogService } from '../../app/dialogs';
import { createAppEventBus } from '../../app/events';
import type { FeatureContext } from '../../app/features';
import { createScreenRegistry } from '../../app/screens';
import { useAppStore } from '../../app/store';
import {
  createFakeIpc,
  type FakeIpc,
  previewFixture,
  projectFixture,
  settingsFixture,
  toolchainFixture,
} from '../../app/testing/fixtures';
import type { ConsoleHandle, TerminalSize } from '../../panels';

const decoder = new TextDecoder();

/** A console handle whose writes resolve at once, or only when the test says so. */
export class FakeConsole implements ConsoleHandle {
  /** Everything written, decoded, in order (output, separators and markers). */
  text = '';
  /** The `writeSkipped` calls. */
  readonly skipped: number[] = [];
  /** How often it was cleared. */
  clears = 0;
  /** How often it got the focus. */
  focuses = 0;
  /** Whether writes resolve at once. */
  autoResolve = true;
  /** Writes waiting for {@link resolveWrites}. */
  readonly pending: (() => void)[] = [];
  terminalSize: TerminalSize = { cols: 100, rows: 30 };
  readonly #data = new Set<(data: string) => void>();
  readonly #resize = new Set<(size: TerminalSize) => void>();

  write(bytes: Uint8Array): Promise<void> {
    this.text += decoder.decode(bytes);
    if (this.autoResolve) {
      return Promise.resolve();
    }
    return new Promise((resolve) => {
      this.pending.push(resolve);
    });
  }

  /** Resolves the oldest `count` waiting writes (all by default), in order. */
  resolveWrites(count = this.pending.length): void {
    for (const resolve of this.pending.splice(0, count)) {
      resolve();
    }
  }

  writeSkipped(lines: number): void {
    this.skipped.push(lines);
    this.text += `[skipped ${String(lines)}]`;
  }

  clear(): void {
    this.clears += 1;
  }

  size(): TerminalSize {
    return this.terminalSize;
  }

  onData(callback: (data: string) => void): () => void {
    this.#data.add(callback);
    return () => {
      this.#data.delete(callback);
    };
  }

  onResize(callback: (size: TerminalSize) => void): () => void {
    this.#resize.add(callback);
    return () => {
      this.#resize.delete(callback);
    };
  }

  focus(): void {
    this.focuses += 1;
  }

  /** The person types `data`. */
  type(data: string): void {
    for (const listener of this.#data) {
      listener(data);
    }
  }

  /** The terminal is fitted to `size`. */
  resizeTo(size: TerminalSize): void {
    this.terminalSize = size;
    for (const listener of this.#resize) {
      listener(size);
    }
  }
}

/** A build the fake backend was asked for. */
export interface RecordedBuild {
  readonly request: BuildStartRequest;
  readonly buildId: BuildId;
  /** Plays a build event on its channel. */
  send(event: BuildEvent | Record<string, unknown>): void;
}

/** A run the fake backend was asked for. */
export interface RecordedRun {
  readonly request: RunStartRequest;
  readonly runId: RunId;
  /** Plays an output batch (text, UTF-8). */
  output(text: string): void;
  /** Plays a run event. */
  send(event: RunEvent | Record<string, unknown>): void;
}

/** A build ID for number `n`. */
export function buildIdOf(n: number): BuildId {
  return `bd_${n.toString(16).padStart(32, '0')}`;
}

/** A run ID for number `n`. */
export function runIdOf(n: number): RunId {
  return `rn_${n.toString(16).padStart(32, '0')}`;
}

/** The fake backend: an IPC client that records builds and runs. */
export interface FakeBackend {
  readonly ipc: FakeIpc;
  readonly builds: RecordedBuild[];
  readonly runs: RecordedRun[];
  /** The `runInput` data, decoded from base64 to text. */
  inputs(): string[];
}

const encoder = new TextEncoder();

/** Decodes standard base64 to bytes. */
export function fromBase64(data: string): Uint8Array {
  const binary = atob(data);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index++) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

/**
 * A fake IPC client whose `buildStart` and `runStart` succeed with fresh IDs and record their
 * channels, and whose acknowledgement, input, resize, stop and cancel calls succeed.
 */
export function createFakeBackend(): FakeBackend {
  const ipc = createFakeIpc();
  const builds: RecordedBuild[] = [];
  const runs: RecordedRun[] = [];
  ipc.buildStart.mockImplementation((request, onEvent) => {
    const buildId = buildIdOf(builds.length + 1);
    builds.push({
      request,
      buildId,
      send: (event) => {
        onEvent(event as BuildEvent);
      },
    });
    return Promise.resolve({ buildId });
  });
  ipc.runStart.mockImplementation((request, onOutput, onEvent) => {
    const runId = runIdOf(runs.length + 1);
    runs.push({
      request,
      runId,
      output: (text) => {
        onOutput(encoder.encode(text).slice().buffer);
      },
      send: (event) => {
        onEvent(event as RunEvent);
      },
    });
    return Promise.resolve({ runId });
  });
  for (const method of [
    ipc.buildCancel,
    ipc.runAck,
    ipc.runInput,
    ipc.runResize,
    ipc.runStop,
  ] as const) {
    method.mockImplementation(() => Promise.resolve({}));
  }
  return {
    ipc,
    builds,
    runs,
    inputs: () =>
      ipc.runInput.mock.calls.map(([request]) => decoder.decode(fromBase64(request.data))),
  };
}

/** Dialogs that answer at once (OK, no, cancel) and record what they were asked. */
export interface FakeDialogs {
  readonly alert: Mock<DialogService['alert']>;
  readonly confirm: Mock<DialogService['confirm']>;
  readonly prompt: Mock<DialogService['prompt']>;
  readonly choose: Mock<(options: ChoiceOptions<string>) => Promise<string>>;
}

/** Creates {@link FakeDialogs}. */
export function fakeDialogs(): FakeDialogs {
  return {
    alert: vi.fn<DialogService['alert']>(() => Promise.resolve()),
    confirm: vi.fn<DialogService['confirm']>(() => Promise.resolve(false)),
    prompt: vi.fn<DialogService['prompt']>(() => Promise.resolve(null)),
    choose: vi.fn((options: ChoiceOptions<string>) => Promise.resolve(options.cancel)),
  };
}

/** A feature context over the app's store with fresh registries and `dialogs`. */
export function featureContext(
  ipc: FakeIpc,
  overrides: Partial<FeatureContext> = {},
  dialogs: FakeDialogs = fakeDialogs(),
): FeatureContext {
  return {
    ipc,
    store: useAppStore,
    commands: createCommandRegistry(),
    screens: createScreenRegistry(),
    dialogs: dialogs as unknown as DialogService,
    events: createAppEventBus(),
    core: () => null,
    editor: () => null,
    ...overrides,
  };
}

/** Opens a trusted project with no errors, a usable g++ and the default settings. */
export function openRunnableProject(): void {
  const { actions } = useAppStore.getState();
  actions.setProject(projectFixture({ canonicalText: '{"doc":1}', contentHash: 'a'.repeat(64) }));
  actions.setToolchains({ list: [toolchainFixture()], discovering: false });
  actions.setSettings({ value: settingsFixture() });
  actions.setAnalysis({ preview: previewFixture([]) });
}

/** Lets promise callbacks and zero-delay timers run (with fake timers in use). */
export async function settle(): Promise<void> {
  for (let round = 0; round < 5; round++) {
    await vi.advanceTimersByTimeAsync(0);
  }
}
