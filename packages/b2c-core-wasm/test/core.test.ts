// The wrapper around one instance's glue, with a fake glue: JSON parsing, error envelopes, traps
// and input bounds. Runs without a build.

import { describe, expect, it, vi } from 'vitest';

import { createCore, type GlueFunctions, MAX_DOCUMENT_BYTES } from '../src/core';
import { CoreError, CoreTrap } from '../src/errors';

const VERSION = '{"app":"0.1.0","catalog":"1.0.0","formatVersion":1,"sourceMapVersion":1}';

function fakeGlue(overrides: Partial<GlueFunctions> = {}): GlueFunctions {
  return {
    version: vi.fn(() => VERSION),
    load: vi.fn(() => '{"ok":false,"diagnostics":[]}'),
    canonical: vi.fn(() => '{"ok":false,"diagnostics":[]}'),
    preview: vi.fn(() => '{"stage":"load","diagnostics":[],"files":[]}'),
    symbols_in_scope: vi.fn(() => '[]'),
    conversion_table: vi.fn(() => '[{"from":"int","to":"double","conversion":"widening"}]'),
    clipboard_make: vi.fn(() => '{"ok":true,"payload":"{}","diagnostics":[]}'),
    paste_prepare: vi.fn(() => '{"ok":false,"unresolved":[],"diagnostics":[]}'),
    ...overrides,
  };
}

describe('createCore', () => {
  it('parses results', () => {
    const handle = createCore(fakeGlue());
    const { core } = handle;
    expect(core.version()).toEqual({
      app: '0.1.0',
      catalog: '1.0.0',
      formatVersion: 1,
      sourceMapVersion: 1,
    });
    expect(core.load(new Uint8Array([1, 2]))).toEqual({ ok: false, diagnostics: [] });
    expect(handle.trapped).toBe(false);
  });

  it('sends only the known preview option', () => {
    const glue = fakeGlue();
    const { core } = createCore(glue);
    const options = { indentWidth: 2, tabs: true, __proto__: { polluted: true } } as const;
    core.preview('{}', options);
    expect(glue.preview).toHaveBeenCalledWith('{}', '{"indentWidth":2}');
  });

  it('turns error envelopes into CoreError', () => {
    const handle = createCore(
      fakeGlue({
        preview: () => '{"error":{"kind":"invalidOptions","message":"expected 2 or 4"}}',
        canonical: () => '{"error":{"kind":"encode","message":"boom"}}',
        load: () => '{"error":{"kind":"somethingNew","message":7}}',
      }),
    );
    const { core } = handle;
    const refused = (run: () => unknown) => {
      try {
        run();
      } catch (error) {
        return error;
      }
      throw new Error('expected a CoreError');
    };
    const invalid = refused(() => core.preview('{}', { indentWidth: 4 }));
    expect(invalid).toBeInstanceOf(CoreError);
    expect(invalid).toMatchObject({ kind: 'invalidOptions', message: 'preview: expected 2 or 4' });
    expect(refused(() => core.canonical('{}'))).toMatchObject({ kind: 'encode' });
    expect(refused(() => core.load(new Uint8Array()))).toMatchObject({
      kind: 'protocol',
      message: 'load: unknown error',
    });
    // A refused call does not stop the instance.
    expect(handle.trapped).toBe(false);
    expect(core.version().app).toBe('0.1.0');
  });

  it.each([
    ['text that is not JSON', 'not json'],
    ['an array', '[]'],
    ['null', 'null'],
    ['a number', '42'],
    ['an error that is not an object', '{"error":"x"}'],
  ])('reports %s as a protocol error', (_, response) => {
    const { core } = createCore(fakeGlue({ version: () => response }));
    expect(() => core.version()).toThrow(CoreError);
    try {
      core.version();
    } catch (error) {
      expect((error as CoreError).kind).toBe('protocol');
      expect((error as CoreError).name).toBe('CoreError');
    }
  });

  it('stops for good after a trap', () => {
    const trap = new WebAssembly.RuntimeError('unreachable');
    const load = vi.fn(() => {
      throw trap;
    });
    const glue = fakeGlue({ load });
    const handle = createCore(glue);
    let thrown: unknown;
    try {
      handle.core.load(new Uint8Array([0]));
    } catch (error) {
      thrown = error;
    }
    expect(thrown).toBeInstanceOf(CoreTrap);
    expect((thrown as CoreTrap).name).toBe('CoreTrap');
    expect((thrown as CoreTrap).cause).toBe(trap);
    expect((thrown as CoreTrap).message).toContain('unreachable');
    expect(handle.trapped).toBe(true);
    // No later call reaches the instance again.
    expect(() => handle.core.version()).toThrow(CoreTrap);
    expect(() => handle.core.preview('{}', { indentWidth: 4 })).toThrow(/initCore/);
    expect(glue.version).not.toHaveBeenCalled();
    expect(glue.preview).not.toHaveBeenCalled();
    expect(load).toHaveBeenCalledTimes(1);
  });

  it('treats any exception from inside a call as a trap', () => {
    const handle = createCore(
      fakeGlue({
        canonical: () => {
          throw new RangeError('Maximum call stack size exceeded');
        },
      }),
    );
    expect(() => handle.core.canonical('{}')).toThrow(CoreTrap);
    expect(handle.trapped).toBe(true);
  });

  it('passes scope and clipboard arguments as the core expects them', () => {
    const glue = fakeGlue();
    const { core } = createCore(glue);
    expect(core.symbolsInScope('b005', null)).toEqual([]);
    expect(core.symbolsInScope('b010', 'BODY')).toEqual([]);
    expect(glue.symbols_in_scope).toHaveBeenNthCalledWith(1, 'b005', null);
    expect(glue.symbols_in_scope).toHaveBeenNthCalledWith(2, 'b010', 'BODY');
    expect(core.conversionTable()).toEqual([{ from: 'int', to: 'double', conversion: 'widening' }]);
    expect(core.clipboardMake('{"doc":1}', ['b1', 'b2'])).toEqual({
      ok: true,
      payload: '{}',
      diagnostics: [],
    });
    expect(glue.clipboard_make).toHaveBeenCalledWith('{"doc":1}', '["b1","b2"]');
    // Only the known keys of the target cross, whatever else the object carries.
    const target = {
      module: 'mod_main',
      block: null,
      input: null,
      extra: '<script>',
      __proto__: { polluted: true },
    } as const;
    expect(core.pastePrepare('payload', 'doc', target, 'ab'.repeat(32))).toEqual({
      ok: false,
      unresolved: [],
      diagnostics: [],
    });
    expect(glue.paste_prepare).toHaveBeenCalledWith(
      'payload',
      'doc',
      '{"module":"mod_main","block":null,"input":null}',
      'ab'.repeat(32),
    );
  });

  it('turns error envelopes of list results into CoreError', () => {
    const { core } = createCore(
      fakeGlue({
        symbols_in_scope: () => '{"error":{"kind":"internal","message":"busy"}}',
        conversion_table: () => '{"ok":true}',
        clipboard_make: () => '{"error":{"kind":"invalidArguments","message":"bad ids"}}',
        paste_prepare: () => '[]',
      }),
    );
    expect(() => core.symbolsInScope('b', null)).toThrow(
      expect.objectContaining({ kind: 'internal', message: 'symbolsInScope: busy' }) as Error,
    );
    expect(() => core.conversionTable()).toThrow(
      expect.objectContaining({
        kind: 'protocol',
        message: 'conversionTable: the core returned something other than a list',
      }) as Error,
    );
    expect(() => core.clipboardMake('{}', [])).toThrow(
      expect.objectContaining({ kind: 'invalidArguments' }) as Error,
    );
    expect(() => core.pastePrepare('', '', { module: 'm', block: null, input: null }, '')).toThrow(
      expect.objectContaining({ kind: 'protocol' }) as Error,
    );
    for (const response of ['not json', '"text"', '7', 'null']) {
      const { core: other } = createCore(fakeGlue({ symbols_in_scope: () => response }));
      expect(() => other.symbolsInScope('b', null)).toThrow(CoreError);
    }
  });

  it('keeps oversized input bounded', () => {
    const glue = fakeGlue();
    const { core } = createCore(glue);
    const bytes = new Uint8Array(MAX_DOCUMENT_BYTES + 100);
    core.load(bytes);
    const sentBytes = vi.mocked(glue.load).mock.calls[0]?.[0];
    expect(sentBytes?.length).toBe(MAX_DOCUMENT_BYTES + 1);

    const text = 'x'.repeat(MAX_DOCUMENT_BYTES + 100);
    core.canonical(text);
    core.preview(text, { indentWidth: 4 });
    expect(vi.mocked(glue.canonical).mock.calls[0]?.[0].length).toBe(MAX_DOCUMENT_BYTES + 1);
    expect(vi.mocked(glue.preview).mock.calls[0]?.[0].length).toBe(MAX_DOCUMENT_BYTES + 1);

    core.pastePrepare(text, text, { module: 'm', block: null, input: null }, text);
    const [payload, document, , seed] = vi.mocked(glue.paste_prepare).mock.calls[0] ?? [];
    expect([payload?.length, document?.length, seed?.length]).toEqual([
      MAX_DOCUMENT_BYTES + 1,
      MAX_DOCUMENT_BYTES + 1,
      MAX_DOCUMENT_BYTES + 1,
    ]);
    core.clipboardMake(text, []);
    expect(vi.mocked(glue.clipboard_make).mock.calls[0]?.[0].length).toBe(MAX_DOCUMENT_BYTES + 1);
    core.symbolsInScope(text, text);
    const [block, input] = vi.mocked(glue.symbols_in_scope).mock.calls[0] ?? [];
    expect([block?.length, input?.length]).toEqual([
      MAX_DOCUMENT_BYTES + 1,
      MAX_DOCUMENT_BYTES + 1,
    ]);

    // Input within the limit is passed unchanged.
    const small = new Uint8Array([1, 2, 3]);
    core.load(small);
    expect(vi.mocked(glue.load).mock.calls[1]?.[0]).toBe(small);
  });
});
