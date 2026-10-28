/**
 * The editor's worker preview service over a scripted worker: the worker's core version is checked
 * against the main instance's as it was when the client was created, so a main instance that traps
 * later (and is not restarted while the worker previews) never makes a healthy worker refused.
 */
import {
  CoreTrap,
  type CoreVersion,
  type CoreWasm,
  type UpdateResult,
} from '@blocks2cpp/b2c-core-wasm';
import type { WorkerLike, WorkerRequest } from '@blocks2cpp/b2c-core-wasm/worker';
import { describe, expect, it, vi } from 'vitest';

import { createCoreHost } from '../coreHost';
import { appWorkerPreviewService, sameVersion } from './spawn';

const VERSION: CoreVersion = {
  app: '0.1.0',
  catalog: '1.3.0',
  formatVersion: 2,
  sourceMapVersion: 1,
};

/** `\0asm` version 1: the smallest valid module. */
const MODULE = new WebAssembly.Module(new Uint8Array([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0]));

/** A worker whose replies the test writes. */
class ScriptedWorker implements WorkerLike {
  onmessage: ((event: { readonly data: unknown }) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  onmessageerror: ((event: unknown) => void) | null = null;
  readonly sent: WorkerRequest[] = [];
  terminated = false;

  postMessage(message: WorkerRequest): void {
    this.sent.push(message);
  }

  terminate(): void {
    this.terminated = true;
  }

  started(version: CoreVersion): void {
    this.onmessage?.({ data: { p: 1, kind: 'started', versionJson: JSON.stringify(version) } });
  }
}

/** A main-thread core that can be made to trap on every call, as a trapped instance does. */
function mainCore(): CoreWasm & { trap: () => void } {
  let trapped = false;
  const guard = (): void => {
    if (trapped) {
      throw new CoreTrap('version: the compiler core stopped after an earlier internal error');
    }
  };
  return {
    trap: () => {
      trapped = true;
    },
    version: () => {
      guard();
      return VERSION;
    },
    update: () => {
      guard();
      return { stage: 'generate' } as unknown as UpdateResult;
    },
  } as unknown as CoreWasm & { trap: () => void };
}

async function flush(): Promise<void> {
  for (let turn = 0; turn < 5; turn += 1) {
    await Promise.resolve();
  }
}

function setUp() {
  const core = mainCore();
  let current: CoreWasm | null = core;
  const host = createCoreHost({
    initCore: () => Promise.resolve(core),
    resetCore: vi.fn(),
    getCore: () => current,
    setCore: (next) => {
      current = next;
    },
  });
  const workers: ScriptedWorker[] = [];
  const service = appWorkerPreviewService(host, {
    spawn: () => {
      const worker = new ScriptedWorker();
      workers.push(worker);
      return worker;
    },
    module: () => Promise.resolve(MODULE),
  });
  return {
    core,
    workers,
    service,
    clearCore: () => {
      current = null;
    },
  };
}

describe('the editor’s worker preview service', () => {
  it('accepts a worker of the main instance’s version even after that instance trapped', async () => {
    const errors = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { core, workers, service } = setUp();
    // The first update runs on the main instance and creates the client.
    await service.update('{}', { indentWidth: 4 });
    await flush();
    expect(workers).toHaveLength(1);
    core.trap();
    workers[0]?.started(VERSION);
    expect(service.client()?.state).toBe('ready');
    expect(service.thread()).toBe('worker');
    expect(workers).toHaveLength(1);
    service.dispose();
    errors.mockRestore();
  });

  it('accepts it while the host has no main instance (a restart in progress)', async () => {
    const { clearCore, workers, service } = setUp();
    await service.update('{}', { indentWidth: 4 });
    await flush();
    clearCore();
    workers[0]?.started(VERSION);
    expect(service.client()?.state).toBe('ready');
    service.dispose();
  });

  it('refuses a worker of another version', async () => {
    const errors = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { workers, service } = setUp();
    await service.update('{}', { indentWidth: 4 });
    await flush();
    workers[0]?.started({ ...VERSION, app: '9.9.9' });
    expect(workers[0]?.terminated).toBe(true);
    expect(workers).toHaveLength(2);
    service.dispose();
    errors.mockRestore();
  });

  it('compares every field of a version', () => {
    expect(sameVersion(VERSION, { ...VERSION })).toBe(true);
    for (const changed of [
      { ...VERSION, app: '0.1.1' },
      { ...VERSION, catalog: '1.3.1' },
      { ...VERSION, formatVersion: 3 },
      { ...VERSION, sourceMapVersion: 2 },
    ]) {
      expect(sameVersion(VERSION, changed)).toBe(false);
    }
  });
});
