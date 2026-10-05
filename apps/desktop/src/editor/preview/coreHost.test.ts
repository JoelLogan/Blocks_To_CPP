/** Starting, publishing and replacing the compiler core. */
import { CoreError, type CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { getCore, setCore } from '../../app/core';
import { appCoreHost, createCoreHost, liveCore } from './coreHost';
import { mainThreadPreviewService } from './service';

/** A core whose every method answers with its own name and arguments. */
function namedCore(name: string): CoreWasm {
  const answer =
    (method: string) =>
    (...args: unknown[]): never =>
      ({ name, method, args }) as never;
  return {
    version: answer('version'),
    load: answer('load'),
    canonical: answer('canonical'),
    preview: answer('preview'),
    symbolsInScope: answer('symbolsInScope'),
    conversionTable: answer('conversionTable'),
    clipboardMake: answer('clipboardMake'),
    pastePrepare: answer('pastePrepare'),
  };
}

afterEach(() => {
  setCore(null);
});

describe('the core host', () => {
  it('starts the core once, publishes it, and returns the running one', async () => {
    const core = namedCore('first');
    const initCore = vi.fn(() => Promise.resolve(core));
    const host = createCoreHost({ initCore, resetCore: vi.fn(), getCore, setCore });
    expect(host.current()).toBeNull();
    const [a, b] = await Promise.all([host.start(), host.start()]);
    expect(a).toBe(core);
    expect(b).toBe(core);
    expect(initCore).toHaveBeenCalledTimes(1);
    expect(getCore()).toBe(core);
    await host.start();
    expect(initCore).toHaveBeenCalledTimes(1);
  });

  it('tries again after a failed start', async () => {
    const core = namedCore('second try');
    const initCore = vi
      .fn<() => Promise<CoreWasm>>()
      .mockRejectedValueOnce(new CoreError('init', 'no module'))
      .mockResolvedValueOnce(core);
    const host = createCoreHost({ initCore, resetCore: vi.fn(), getCore, setCore });
    await expect(host.start()).rejects.toThrow(CoreError);
    expect(getCore()).toBeNull();
    await expect(host.start()).resolves.toBe(core);
  });

  it('replaces the core on restart', async () => {
    const cores = [namedCore('first'), namedCore('second')];
    const resetCore = vi.fn();
    const host = createCoreHost({
      initCore: () => Promise.resolve(cores.shift() ?? namedCore('more')),
      resetCore,
      getCore,
      setCore,
    });
    const first = await host.start();
    const second = await host.restart();
    expect(resetCore).toHaveBeenCalledTimes(1);
    expect(second).not.toBe(first);
    expect(getCore()).toBe(second);
  });

  it('shares one app host', () => {
    expect(appCoreHost()).toBe(appCoreHost());
  });
});

describe('the live core', () => {
  it('forwards every call to the running instance, also after it was replaced', () => {
    let current: CoreWasm | null = null;
    const host = {
      current: () => current,
      start: () => Promise.reject(new Error('unused')),
      restart: () => Promise.reject(new Error('unused')),
    };
    const live = liveCore(host);
    expect(() => live.version()).toThrow(CoreError);

    current = namedCore('one');
    const bytes = new Uint8Array([1]);
    const calls = [
      live.version(),
      live.load(bytes),
      live.canonical('{}'),
      live.preview('{}', { indentWidth: 2 }),
      live.symbolsInScope('b1', null),
      live.conversionTable(),
      live.clipboardMake('{}', ['b1']),
      live.pastePrepare('x', '{}', { module: 'm', block: null, input: null }, 'seed'),
    ] as unknown as { name: string; method: string }[];
    expect(calls.map((call) => call.method)).toEqual([
      'version',
      'load',
      'canonical',
      'preview',
      'symbolsInScope',
      'conversionTable',
      'clipboardMake',
      'pastePrepare',
    ]);
    current = namedCore('two');
    expect((live.version() as unknown as { name: string }).name).toBe('two');
  });
});

describe('the main-thread preview service', () => {
  it('starts the core if needed and previews with it', async () => {
    const core = namedCore('previewer');
    const host = createCoreHost({
      initCore: () => Promise.resolve(core),
      resetCore: vi.fn(),
      getCore,
      setCore,
    });
    const result = (await mainThreadPreviewService(host).preview('{"a":1}', {
      indentWidth: 4,
    })) as unknown as { method: string; args: unknown[] };
    expect(result.method).toBe('preview');
    expect(result.args).toEqual(['{"a":1}', { indentWidth: 4 }]);
  });
});
