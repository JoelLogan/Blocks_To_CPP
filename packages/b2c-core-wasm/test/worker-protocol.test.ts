// The preview worker's protocol checks (src/worker/protocol.ts): every message of version 1 is
// accepted as it is, and anything else (another version, an unknown kind, a missing, extra or
// mistyped field, something that is not a plain object) is refused. Runs without a build.

import { describe, expect, it } from 'vitest';

import {
  MAX_REPLY_MESSAGE,
  parseReply,
  parseRequest,
  WORKER_PROTOCOL,
} from '../src/worker/protocol';

/** `\0asm` version 1: the smallest valid module. */
const MODULE = new WebAssembly.Module(new Uint8Array([0, 0x61, 0x73, 0x6d, 1, 0, 0, 0]));

const REQUESTS: readonly Record<string, unknown>[] = [
  { p: 1, kind: 'start', module: MODULE },
  { p: 1, kind: 'update', id: 1, documentJson: '{}', optionsJson: '{"indentWidth":4}' },
  { p: 1, kind: 'probe', id: 7 },
];

const REPLIES: readonly Record<string, unknown>[] = [
  { p: 1, kind: 'started', versionJson: '{}' },
  { p: 1, kind: 'startFailed', message: 'CompileError: no' },
  {
    p: 1,
    kind: 'updated',
    id: 3,
    updateJson: '{"stage":"load"}',
    scopeJson: null,
    startedAt: 1_700_000_000_000.5,
    updateMs: 42.5,
    indexMs: 0,
  },
  {
    p: 1,
    kind: 'updated',
    id: 3,
    updateJson: '{}',
    scopeJson: '{"v":1}',
    startedAt: 0,
    updateMs: 0,
    indexMs: 1,
  },
  { p: 1, kind: 'trapped', id: 2, message: 'stopped' },
  {
    p: 1,
    kind: 'probed',
    id: 4,
    probe: { eval: 'blocked', request: 'blocked', directives: ['connect-src', 'script-src'] },
  },
];

/** Each message with one field changed to `value`, or removed (`undefined`). */
function variants(
  message: Record<string, unknown>,
  values: readonly unknown[],
): Record<string, unknown>[] {
  const out: Record<string, unknown>[] = [];
  for (const key of Object.keys(message)) {
    if (key === 'kind') {
      continue;
    }
    for (const value of values) {
      const changed: Record<string, unknown> = { ...message };
      if (value === undefined) {
        Reflect.deleteProperty(changed, key);
      } else {
        changed[key] = value;
      }
      out.push(changed);
    }
  }
  return out;
}

/** Values no field of the protocol takes, whatever its type. */
const WRONG = [undefined, true, -1, 0, 1.5, Number.NaN, Infinity, 'x'.repeat(600), [], {}, () => 1];

describe('parseRequest', () => {
  it('accepts every request of version 1 as it is, also after a structured clone', () => {
    for (const request of REQUESTS) {
      expect(parseRequest(request)).toBe(request);
      expect(parseRequest(structuredClone(request))).not.toBeNull();
    }
  });

  it('refuses another version, an unknown kind and things that are not messages', () => {
    for (const request of REQUESTS) {
      expect(parseRequest({ ...request, p: 2 })).toBeNull();
      expect(parseRequest({ ...request, p: '1' })).toBeNull();
      expect(parseRequest({ ...request, kind: 'Update' })).toBeNull();
      expect(parseRequest({ ...request, extra: 1 })).toBeNull();
    }
    for (const garbage of [null, undefined, 1, 'start', [], [1], new Map(), new Date()]) {
      expect(parseRequest(garbage)).toBeNull();
    }
    expect(parseRequest({ p: WORKER_PROTOCOL, kind: 'started', versionJson: '{}' })).toBeNull();
  });

  it('refuses a missing or mistyped field', () => {
    for (const request of REQUESTS) {
      for (const changed of variants(request, WRONG)) {
        const valid =
          (changed['kind'] === 'update' &&
            ['documentJson', 'optionsJson'].some(
              (key) => typeof changed[key] === 'string' && changed[key] !== request[key],
            )) ||
          false;
        // A string field may hold any string (the core checks the text).
        expect(parseRequest(changed) === null, JSON.stringify(Object.keys(changed))).toBe(!valid);
      }
    }
  });

  it('refuses objects that are not plain', () => {
    class Start {
      readonly p = 1;
      readonly kind = 'start';
      readonly module = MODULE;
    }
    expect(parseRequest(new Start())).toBeNull();
    const inherited = Object.create({ p: 1, kind: 'probe', id: 1 }) as object;
    expect(parseRequest(inherited)).toBeNull();
    const bare = Object.assign(Object.create(null) as object, { p: 1, kind: 'probe', id: 1 });
    expect(parseRequest(bare)).not.toBeNull();
  });

  it('takes only a WebAssembly.Module to start', () => {
    expect(parseRequest({ p: 1, kind: 'start', module: new Uint8Array(8) })).toBeNull();
    expect(parseRequest({ p: 1, kind: 'start', module: {} })).toBeNull();
  });
});

describe('parseReply', () => {
  it('accepts every reply of version 1 as it is, also after a structured clone', () => {
    for (const reply of REPLIES) {
      expect(parseReply(reply)).toBe(reply);
      expect(parseReply(structuredClone(reply))).not.toBeNull();
    }
  });

  it('refuses another version, an unknown kind and things that are not messages', () => {
    for (const reply of REPLIES) {
      expect(parseReply({ ...reply, p: 0 })).toBeNull();
      expect(parseReply({ ...reply, kind: 'update' })).toBeNull();
      expect(parseReply({ ...reply, extra: null })).toBeNull();
    }
    for (const garbage of [null, 0, '', 'started', [{ p: 1 }], new Error('x')]) {
      expect(parseReply(garbage)).toBeNull();
    }
  });

  it('refuses a missing or mistyped field', () => {
    for (const reply of REPLIES) {
      for (const changed of variants(reply, WRONG)) {
        const parsed = parseReply(changed);
        // The only changes that stay valid: another string where any string goes, another
        // time or ID that is still one.
        const stillValid = Object.entries(changed).every(([key, value]) => {
          const original = reply[key];
          if (value === original) {
            return true;
          }
          if (key === 'versionJson' || key === 'updateJson') {
            return typeof value === 'string';
          }
          if (key === 'scopeJson') {
            return value === null || typeof value === 'string';
          }
          if (key === 'message') {
            return typeof value === 'string' && value.length <= MAX_REPLY_MESSAGE;
          }
          if (key === 'startedAt' || key === 'updateMs' || key === 'indexMs') {
            return typeof value === 'number' && Number.isFinite(value) && value >= 0;
          }
          if (key === 'id') {
            return typeof value === 'number' && Number.isSafeInteger(value) && value > 0;
          }
          return false;
        });
        const complete = Object.keys(changed).length === Object.keys(reply).length;
        expect(parsed !== null, JSON.stringify(changed)).toBe(stillValid && complete);
      }
    }
  });

  it('checks the probe', () => {
    const probed = (probe: unknown) => ({ p: 1, kind: 'probed', id: 1, probe });
    expect(
      parseReply(probed({ eval: 'allowed', request: 'failed', directives: [] })),
    ).not.toBeNull();
    for (const probe of [
      null,
      { eval: 'maybe', request: 'blocked', directives: [] },
      { eval: 'blocked', request: 'refused', directives: [] },
      { eval: 'blocked', request: 'blocked', directives: ['Script Src'] },
      { eval: 'blocked', request: 'blocked', directives: ['a'.repeat(65)] },
      { eval: 'blocked', request: 'blocked', directives: Array<string>(33).fill('script-src') },
      { eval: 'blocked', request: 'blocked', directives: 'script-src' },
      { eval: 'blocked', request: 'blocked', directives: [], sample: 'text' },
    ]) {
      expect(parseReply(probed(probe)), JSON.stringify(probe)).toBeNull();
    }
  });
});
