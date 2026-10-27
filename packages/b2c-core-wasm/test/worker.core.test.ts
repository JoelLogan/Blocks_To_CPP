// The preview worker's client and host with the real compiler core, built by
// `pnpm --filter @blocks2cpp/b2c-core-wasm build`, joined by Node's MessageChannel: the page's
// compiled `Module` is structured-cloned to the host, which instantiates it with its own glue,
// and every example previews in it exactly as on the main thread. The last test runs the host
// with `Object.prototype` frozen, as the worker's entry does. Without a build these tests are
// skipped, unless B2C_REQUIRE_WASM is set (as in CI), in which case a missing build fails them.

import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { MessageChannel, type MessagePort } from 'node:worker_threads';

import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import type { CoreWasm } from '../src/api';
import { initCoreFromBytes } from '../src/index';
import {
  createCoreWorkerClient,
  type CoreWorkerClient,
  serveCoreWorker,
  type WorkerLike,
} from '../src/worker';

const repo = (path: string) => fileURLToPath(new URL(`../../../${path}`, import.meta.url));
const builtModule = fileURLToPath(new URL('../pkg/b2c_core_wasm_bg.wasm', import.meta.url));
const built = existsSync(builtModule);
const required = (process.env['B2C_REQUIRE_WASM'] ?? '') !== '';

if (!built && !required) {
  console.warn(
    'test/worker.core.test.ts skipped: build the module first (pnpm --filter @blocks2cpp/b2c-core-wasm build)',
  );
}

const examplesDir = repo('examples');
const exampleNames = readdirSync(examplesDir)
  .filter((name) => name.endsWith('.b2c'))
  .map((name) => name.slice(0, -'.b2c'.length))
  .sort();

const ports: MessagePort[] = [];

/** A worker that is the host behind one end of a MessageChannel. */
function channelWorker(): WorkerLike {
  const { port1, port2 } = new MessageChannel();
  ports.push(port1, port2);
  serveCoreWorker({
    postMessage: (message) => {
      port2.postMessage(message);
    },
    set onmessage(handler: ((event: { readonly data: unknown }) => void) | null) {
      port2.onmessage = handler as ((event: unknown) => void) | null;
    },
    get onmessage() {
      return null;
    },
  });
  const worker: WorkerLike = {
    postMessage: (message) => {
      port1.postMessage(message);
    },
    onmessage: null,
    onerror: null,
    onmessageerror: null,
    terminate: () => {
      port1.close();
      port2.close();
    },
  };
  port1.onmessage = (event: { data: unknown }) => worker.onmessage?.(event);
  return worker;
}

afterAll(() => {
  for (const port of ports) {
    port.close();
  }
});

describe.skipIf(!built && !required)('the preview worker with the real core', () => {
  let core: CoreWasm;
  let module: WebAssembly.Module;
  let client: CoreWorkerClient;

  beforeAll(async () => {
    const bytes = new Uint8Array(readFileSync(builtModule));
    core = await initCoreFromBytes(bytes);
    module = await WebAssembly.compile(bytes);
    client = createCoreWorkerClient({
      spawn: channelWorker,
      module: () => Promise.resolve(module),
      acceptVersion: (version) => JSON.stringify(version) === JSON.stringify(core.version()),
    });
  });

  afterAll(() => {
    client.dispose();
  });

  it('starts with the main instance’s version', async () => {
    await expect(client.ready()).resolves.toEqual(core.version());
  });

  it.each(exampleNames)('previews %s as the main thread does', async (name) => {
    const loaded = core.load(new Uint8Array(readFileSync(`${examplesDir}/${name}.b2c`)));
    expect(loaded.ok).toBe(true);
    if (!loaded.ok) {
      return;
    }
    const json = JSON.stringify(loaded.document);
    for (const indentWidth of [2, 4] as const) {
      const answer = await client.update(json, { indentWidth });
      expect(answer?.result).toEqual(core.update(json, { indentWidth }));
      expect(answer?.scopeJson).toBeNull();
      expect(answer?.updateMs).toBeGreaterThanOrEqual(0);
    }
  });

  it('answers a document that does not load, and refuses invalid options, as the main thread does', async () => {
    const answer = await client.update('{"not":"a project"}', { indentWidth: 4 });
    expect(answer?.result).toEqual(core.update('{"not":"a project"}', { indentWidth: 4 }));
    expect(answer?.result.stage).toBe('load');
    const error: unknown = await client
      .update('{}', { indentWidth: 3 as 4 })
      .catch((caught: unknown) => caught);
    expect(error).toMatchObject({ name: 'CoreError', kind: 'invalidOptions' });
    expect(client.state).toBe('ready');
  });

  it('serves with Object.prototype frozen, as the worker’s entry runs it', async () => {
    // Last: nothing in this file runs after it.
    Object.freeze(Object.prototype);
    const frozen = createCoreWorkerClient({
      spawn: channelWorker,
      module: () => Promise.resolve(module),
    });
    try {
      const json = JSON.stringify(
        (
          core.load(new Uint8Array(readFileSync(`${examplesDir}/guessing_game.b2c`))) as {
            document: unknown;
          }
        ).document,
      );
      const answer = await frozen.update(json, { indentWidth: 4 });
      expect(answer?.result.stage).toBe('generate');
      expect(answer?.result).toEqual(core.update(json, { indentWidth: 4 }));
    } finally {
      frozen.dispose();
    }
  });
});
