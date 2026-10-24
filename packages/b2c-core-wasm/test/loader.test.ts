// Starting, caching and replacing instances, with the generated modules mocked: the embedded
// bytes are a minimal valid WebAssembly module and the glue is a fake that counts instances.
// Runs without a build.

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { CoreWasm } from '../src/api';

/** `\0asm` version 1: the smallest valid module. */
const EMPTY_MODULE_BASE64 = 'AGFzbQEAAAA=';

const state = vi.hoisted(() => ({
  base64: '',
  instances: 0,
  initFails: false,
  trapNext: false,
}));

vi.mock('#pkg-bytes', () => ({
  get WASM_BASE64() {
    return state.base64;
  },
}));

vi.mock('#glue', () => ({
  createGlue: () => {
    state.instances += 1;
    const instance = state.instances;
    let ready = false;
    const call = (result: unknown) => {
      if (!ready) {
        throw new Error('the glue was used before init');
      }
      if (state.trapNext) {
        state.trapNext = false;
        throw new WebAssembly.RuntimeError('unreachable');
      }
      return JSON.stringify(result);
    };
    return {
      init: ({ module_or_path }: { module_or_path: unknown }) => {
        if (!(module_or_path instanceof WebAssembly.Module)) {
          return Promise.reject(new TypeError('expected a compiled module'));
        }
        if (state.initFails) {
          return Promise.reject(new Error('instantiation failed'));
        }
        ready = true;
        return Promise.resolve({});
      },
      version: () =>
        call({
          app: `instance ${String(instance)}`,
          catalog: '1.0.0',
          formatVersion: 1,
          sourceMapVersion: 1,
        }),
      load: () => call({ ok: false, diagnostics: [] }),
      canonical: () => call({ ok: false, diagnostics: [] }),
      preview: () => call({ stage: 'load', diagnostics: [], files: [] }),
    };
  },
}));

type Loader = typeof import('../src/loader') & typeof import('../src/errors');

/** The loader with fresh module state, and the error classes of the same module graph. */
async function freshLoader(): Promise<Loader> {
  vi.resetModules();
  return { ...(await import('../src/loader')), ...(await import('../src/errors')) };
}

async function rejection(promise: Promise<unknown>): Promise<unknown> {
  try {
    await promise;
  } catch (error) {
    return error;
  }
  throw new Error('expected the promise to reject');
}

const app = (core: CoreWasm) => core.version().app;

describe('initCore', () => {
  beforeEach(() => {
    state.base64 = EMPTY_MODULE_BASE64;
    state.instances = 0;
    state.initFails = false;
    state.trapNext = false;
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('starts one instance and returns it to every caller', async () => {
    const loader = await freshLoader();
    const [first, second] = await Promise.all([loader.initCore(), loader.initCore()]);
    const third = await loader.initCore();
    expect(first).toBe(second);
    expect(first).toBe(third);
    expect(state.instances).toBe(1);
    expect(app(first)).toBe('instance 1');
  });

  it('replaces an instance that trapped, compiling the module only once', async () => {
    const compile = vi.spyOn(WebAssembly, 'compile');
    const loader = await freshLoader();
    const first = await loader.initCore();
    state.trapNext = true;
    expect(() => first.load(new Uint8Array())).toThrow(loader.CoreTrap);
    expect(() => first.version()).toThrow(loader.CoreTrap);

    const second = await loader.initCore();
    expect(second).not.toBe(first);
    expect(app(second)).toBe('instance 2');
    expect(await loader.initCore()).toBe(second);
    expect(compile).toHaveBeenCalledTimes(1);
  });

  it('starts a fresh instance after resetCore', async () => {
    const loader = await freshLoader();
    const first = await loader.initCore();
    loader.resetCore();
    const second = await loader.initCore();
    expect(second).not.toBe(first);
    expect(app(second)).toBe('instance 2');
    // The old instance was not trapped, so it still answers.
    expect(app(first)).toBe('instance 1');
  });

  it('reports a corrupt embedded module and retries on the next call', async () => {
    const loader = await freshLoader();
    state.base64 = 'not base64!';
    const error = await rejection(loader.initCore());
    expect(error).toBeInstanceOf(loader.CoreError);
    expect(error).toMatchObject({ kind: 'init' });
    expect((error as Error).message).toContain('not base64');

    state.base64 = 'AAAAAA=='; // valid base64, not a module
    expect(await rejection(loader.initCore())).toMatchObject({ kind: 'init' });

    state.base64 = EMPTY_MODULE_BASE64;
    expect(app(await loader.initCore())).toBe('instance 1');
  });

  it('reports a failed instantiation and retries on the next call', async () => {
    const loader = await freshLoader();
    state.initFails = true;
    const error = await rejection(loader.initCore());
    expect(error).toMatchObject({ kind: 'init', name: 'CoreError' });
    expect((error as Error).cause).toBeInstanceOf(Error);

    state.initFails = false;
    expect(app(await loader.initCore())).toBe('instance 2');
  });
});

describe('initCoreFromBytes', () => {
  beforeEach(() => {
    state.base64 = EMPTY_MODULE_BASE64;
    state.instances = 0;
    state.initFails = false;
    state.trapNext = false;
  });

  it('starts independent instances that initCore does not cache', async () => {
    const loader = await freshLoader();
    const bytes = new Uint8Array([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]);
    const first = await loader.initCoreFromBytes(bytes);
    const second = await loader.initCoreFromBytes(bytes);
    expect(first).not.toBe(second);
    const shared = await loader.initCore();
    expect(shared).not.toBe(first);
    expect(state.instances).toBe(3);
  });

  it('refuses bytes that are not a module', async () => {
    const loader = await freshLoader();
    const error = await rejection(loader.initCoreFromBytes(new Uint8Array([1, 2, 3])));
    expect(error).toBeInstanceOf(loader.CoreError);
    expect(error).toMatchObject({ kind: 'init' });
  });

  it('reports a failed instantiation', async () => {
    const loader = await freshLoader();
    state.initFails = true;
    const bytes = new Uint8Array([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]);
    expect(await rejection(loader.initCoreFromBytes(bytes))).toMatchObject({ kind: 'init' });
  });
});
