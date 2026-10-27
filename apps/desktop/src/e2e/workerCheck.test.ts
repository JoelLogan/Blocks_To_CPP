/** The preview worker's check of the test hook, over a fake worker client and core. */
import type { CoreWasm, UpdateResult } from '@blocks2cpp/b2c-core-wasm';
import type { CoreWorkerClient, WorkerLike } from '@blocks2cpp/b2c-core-wasm/worker';
import { describe, expect, it, vi } from 'vitest';

import { MAX_WORKER_CHECK_ROUNDS, startWorkerCheck, type WorkerCheckDeps } from './workerCheck';

const RESULT = { stage: 'generate', contentHash: 'a'.repeat(64) } as unknown as UpdateResult;

function fakeCore(result: UpdateResult = RESULT): CoreWasm {
  return {
    version: () => ({ app: '0.1.0', catalog: '1.3.0', formatVersion: 2, sourceMapVersion: 1 }),
    update: vi.fn(() => result),
  } as unknown as CoreWasm;
}

function fakeClient(result: UpdateResult = RESULT): CoreWorkerClient & { disposed: boolean } {
  const client = {
    state: 'ready' as const,
    everStarted: true,
    disposed: false,
    ready: () =>
      Promise.resolve({ app: '0.1.0', catalog: '1.3.0', formatVersion: 2, sourceMapVersion: 1 }),
    update: () =>
      Promise.resolve({ result, scopeJson: null, startedAt: 1, updateMs: 2, indexMs: 0 }),
    probe: () =>
      Promise.resolve({ eval: 'blocked' as const, request: 'failed' as const, directives: [] }),
    kill: () => undefined,
    dispose: () => {
      client.disposed = true;
    },
  };
  return client;
}

function deps(overrides: Partial<WorkerCheckDeps> = {}): WorkerCheckDeps {
  let violations = 0;
  return {
    documentJson: () => '{"doc":1}',
    core: () => fakeCore(),
    options: () => ({ indentWidth: 4 }),
    client: () => fakeClient(),
    spawnUnwrapped: () => {
      violations += 1;
      return { terminate: vi.fn() } as unknown as WorkerLike;
    },
    violations: () => violations,
    pageRequest: () => Promise.resolve('allowed'),
    ...overrides,
  };
}

async function finished(check: { result(): unknown }): Promise<unknown> {
  await vi.waitFor(() => {
    expect(check.result()).not.toBeNull();
  });
  return check.result();
}

describe('the worker check', () => {
  it('measures the worker against the main thread and reports the probe and its control', async () => {
    let client: ReturnType<typeof fakeClient> | null = null;
    const check = startWorkerCheck(
      deps({
        client: () => {
          client = fakeClient();
          return client;
        },
      }),
      3,
    );
    expect(check.result()).toBeNull();
    const result = (await finished(check)) as Record<string, unknown>;
    expect(result).toMatchObject({
      ok: true,
      error: null,
      sameAsMain: true,
      contentHash: 'a'.repeat(64),
      requestChars: '{"doc":1}'.length,
      probe: { eval: 'blocked', request: 'failed', directives: [] },
      pageRequest: 'allowed',
      unwrappedViolations: 1,
      policyViolations: 0,
    });
    expect(result['roundTripMs']).toHaveLength(3);
    expect(result['workerUpdateMs']).toEqual([2, 2, 2]);
    expect(result['mainUpdateMs']).toHaveLength(3);
    expect((client as ReturnType<typeof fakeClient> | null)?.disposed).toBe(true);
  });

  it('tells when the worker’s answer differs from the main thread’s', async () => {
    const other = { ...RESULT, stage: 'analyze' } as UpdateResult;
    const result = await finished(startWorkerCheck(deps({ core: () => fakeCore(other) }), 1));
    expect(result).toMatchObject({ ok: true, sameAsMain: false });
  });

  it('reports a failure by its name, and needs an open project', async () => {
    const failing = startWorkerCheck(
      deps({
        client: () => ({
          ...fakeClient(),
          ready: () => Promise.reject(new TypeError('no worker here')),
        }),
      }),
      1,
    );
    expect(await finished(failing)).toEqual({ ok: false, error: 'TypeError: no worker here' });
    const closed = startWorkerCheck(deps({ documentJson: () => null }), 1);
    expect(await finished(closed)).toMatchObject({ ok: false });
  });

  it('takes 1 to 50 rounds', () => {
    for (const rounds of [0, -1, 1.5, MAX_WORKER_CHECK_ROUNDS + 1, Number.NaN]) {
      expect(() => startWorkerCheck(deps(), rounds)).toThrow(TypeError);
    }
  });
});
