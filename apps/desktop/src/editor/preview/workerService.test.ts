/**
 * The worker preview service (ADR-0016, docs/plans/m3-contracts/worker.md §6) over a fake worker
 * client and a fake main-thread core: the main instance before the worker's first `started` and
 * after it has failed, the worker otherwise, and `recover()` never restarting a worker.
 */
import {
  CoreError,
  CoreTrap,
  type CoreVersion,
  type CoreWasm,
  type UpdateResult,
} from '@blocks2cpp/b2c-core-wasm';
import type { CoreWorkerClient, WorkerUpdate } from '@blocks2cpp/b2c-core-wasm/worker';
import { describe, expect, it, vi } from 'vitest';

import { createCoreHost } from './coreHost';
import { previewWorkerEnabled, workerPreviewService } from './workerService';

const VERSION: CoreVersion = {
  app: '0.1.0',
  catalog: '1.3.0',
  formatVersion: 2,
  sourceMapVersion: 1,
};

/** A main-thread core whose updates say where they ran. */
function mainCore(): CoreWasm & { readonly updates: string[] } {
  const updates: string[] = [];
  return {
    updates,
    version: () => VERSION,
    update: (json: string) => {
      updates.push(json);
      if (json === 'TRAP') {
        throw new CoreTrap('stopped');
      }
      return { stage: 'generate', ran: 'main', json } as unknown as UpdateResult;
    },
  } as unknown as CoreWasm & { readonly updates: string[] };
}

/** A host over `core` that counts restarts. */
function hostOf(core: CoreWasm) {
  let current: CoreWasm | null = core;
  const resetCore = vi.fn();
  const host = createCoreHost({
    initCore: () => Promise.resolve(core),
    resetCore,
    getCore: () => current,
    setCore: (next) => {
      current = next;
    },
  });
  return { host, resetCore };
}

/** A worker client the test drives. */
class FakeClient implements CoreWorkerClient {
  state: CoreWorkerClient['state'] = 'starting';
  everStarted = false;
  readonly updates: string[] = [];
  readiness: Promise<CoreVersion> = Promise.resolve(VERSION);
  answer: (json: string) => Promise<WorkerUpdate | null> = (json) =>
    Promise.resolve({
      result: { stage: 'generate', ran: 'worker', json } as unknown as UpdateResult,
      scopeJson: null,
      startedAt: 5,
      updateMs: 7,
      indexMs: 0,
    });
  disposed = false;

  start(): void {
    this.state = 'ready';
    this.everStarted = true;
  }

  ready(): Promise<CoreVersion> {
    return this.readiness;
  }

  update(json: string): Promise<WorkerUpdate | null> {
    this.updates.push(json);
    return this.answer(json);
  }

  kill(): void {
    this.state = 'starting';
  }

  probe(): never {
    throw new Error('no probe');
  }

  dispose(): void {
    this.disposed = true;
  }
}

function setUp() {
  const core = mainCore();
  const { host, resetCore } = hostOf(core);
  const client = new FakeClient();
  const createClient = vi.fn(() => client);
  const log = vi.fn<(message: string) => void>();
  const service = workerPreviewService({ host, createClient, log });
  return { core, host, resetCore, client, createClient, log, service };
}

const OPTIONS = { indentWidth: 4 } as const;

describe('the worker preview service', () => {
  it('is off unless the build was made with B2C_PREVIEW_WORKER=1', () => {
    expect(previewWorkerEnabled()).toBe(false);
  });

  it('runs on the main instance until the worker has started, starting it after the first update', async () => {
    const { core, client, createClient, service } = setUp();
    expect(service.thread()).toBe('main');
    expect(service.client()).toBeNull();
    const first = await service.update('one', OPTIONS);
    expect(first?.result).toMatchObject({ ran: 'main' });
    expect(first?.scopeJson).toBeNull();
    expect(createClient).toHaveBeenCalledTimes(1);
    expect(service.client()).toBe(client);
    // Still starting: the main instance again, and no second client.
    expect((await service.update('two', OPTIONS))?.result).toMatchObject({ ran: 'main' });
    expect(createClient).toHaveBeenCalledTimes(1);
    expect(core.updates).toEqual(['one', 'two']);
  });

  it('runs in the worker once it has started, with the worker’s timing', async () => {
    const { core, client, service } = setUp();
    await service.update('one', OPTIONS);
    client.start();
    expect(service.thread()).toBe('worker');
    const answer = await service.update('two', OPTIONS);
    expect(answer).toEqual({
      result: { stage: 'generate', ran: 'worker', json: 'two' },
      scopeJson: null,
      timing: { startedAt: 5, updateMs: 7, indexMs: 0 },
    });
    expect(client.updates).toEqual(['two']);
    expect(core.updates).toEqual(['one']);
    // While the client replaces its worker, requests still go to it.
    client.kill();
    expect(service.thread()).toBe('worker');
  });

  it('passes on a superseded request as null', async () => {
    const { client, service } = setUp();
    await service.update('one', OPTIONS);
    client.start();
    client.answer = () => Promise.resolve(null);
    await expect(service.update('two', OPTIONS)).resolves.toBeNull();
  });

  it('falls back to the main instance for good when the worker fails, logging once', async () => {
    const { core, client, log, service } = setUp();
    await service.update('one', OPTIONS);
    client.start();
    client.answer = () => {
      client.state = 'failed';
      return Promise.reject(new CoreError('init', 'The preview worker could not be started.'));
    };
    expect((await service.update('two', OPTIONS))?.result).toMatchObject({ ran: 'main' });
    expect(service.thread()).toBe('main');
    expect((await service.update('three', OPTIONS))?.result).toMatchObject({ ran: 'main' });
    expect(core.updates).toEqual(['one', 'two', 'three']);
    expect(log).toHaveBeenCalledTimes(1);
  });

  it('logs the fallback when the worker fails before it ever started', async () => {
    const { client, log, service } = setUp();
    await service.update('one', OPTIONS);
    client.state = 'failed';
    await service.update('two', OPTIONS);
    await service.update('three', OPTIONS);
    expect(log).toHaveBeenCalledTimes(1);
  });

  it('passes on an error envelope and a trap from the worker', async () => {
    const { client, service } = setUp();
    await service.update('one', OPTIONS);
    client.start();
    client.answer = () => Promise.reject(new CoreError('invalidOptions', 'no'));
    await expect(service.update('two', OPTIONS)).rejects.toMatchObject({ kind: 'invalidOptions' });
    client.answer = () => Promise.reject(new CoreTrap('stopped'));
    await expect(service.update('three', OPTIONS)).rejects.toBeInstanceOf(CoreTrap);
  });

  it('recovers from a worker trap by waiting for the worker, never restarting it or the host', async () => {
    const { client, resetCore, service } = setUp();
    await service.update('one', OPTIONS);
    client.start();
    client.answer = () => Promise.reject(new CoreTrap('stopped'));
    await service.update('two', OPTIONS).catch(() => undefined);
    const ready = vi.spyOn(client, 'ready');
    const kill = vi.spyOn(client, 'kill');
    await service.recover();
    expect(ready).toHaveBeenCalledTimes(1);
    expect(kill).not.toHaveBeenCalled();
    expect(resetCore).not.toHaveBeenCalled();
    // A worker that fails while recovering leaves the retry to the main instance.
    client.readiness = Promise.reject(new CoreError('init', 'failed'));
    await expect(service.recover()).resolves.toBeUndefined();
  });

  it('recovers from a main-thread trap by restarting the host', async () => {
    const { resetCore, service } = setUp();
    await expect(service.update('TRAP', OPTIONS)).rejects.toBeInstanceOf(CoreTrap);
    await service.recover();
    expect(resetCore).toHaveBeenCalledTimes(1);
  });

  it('disposes its client, and starts none after', async () => {
    const { client, createClient, service } = setUp();
    service.dispose();
    await service.update('one', OPTIONS);
    expect(createClient).not.toHaveBeenCalled();
    const second = setUp();
    await second.service.update('one', OPTIONS);
    second.service.dispose();
    expect(second.client.disposed).toBe(true);
    expect(client.disposed).toBe(false);
  });
});
