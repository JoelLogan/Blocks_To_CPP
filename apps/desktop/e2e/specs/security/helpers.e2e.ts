/**
 * Unit tests of the security specs' own helpers (lib/): they need no app, and run with the
 * security specs (`pnpm --filter @blocks2cpp/desktop run e2e specs/security`). A helper that
 * classified a refused call as dropped, missed a compiler's name or skipped a file of the
 * malicious-project suite would let an accepted abuse pass unnoticed, so each rule is pinned
 * here.
 */
import { describe, expect, it } from 'vitest';

import {
  bypassCases,
  documentCases,
  FOREIGN_COMMANDS,
  forgedIdCases,
  hookMutations,
  limitCases,
  malformedIds,
  MAX_INPUT_BYTES,
  protoCases,
  readSamples,
  type Samples,
  withProtoKey,
  zeroBytesBase64,
} from './lib/abuse';
import { isCompilerProgram, parseTasklist } from './lib/compilers';
import {
  describeExpected,
  describeOutcome,
  errorCode,
  type IpcOutcome,
  mismatch,
  orderedMessages,
  outcomeOf,
} from './lib/ipc';
import { injectionProblems, type PageState, shownText } from './lib/page';
import { parseSuiteTable, readSuite, SuiteTableError, tableCells } from './lib/projects';

describe('compiler names', () => {
  it('match the programs of a C++ build, with target prefixes, versions and .exe', () => {
    for (const program of [
      'g++',
      '/usr/bin/g++',
      '/usr/bin/x86_64-linux-gnu-g++-13',
      'c++',
      'gcc-14',
      '/usr/libexec/gcc/x86_64-linux-gnu/13/cc1plus',
      'cc1',
      'collect2',
      'lto-wrapper',
      'lto1',
      'as',
      'x86_64-linux-gnu-as',
      'ld',
      'ld.bfd',
      'ld.gold',
      'g++.exe',
      'C:\\msys64\\ucrt64\\bin\\x86_64-w64-mingw32-g++.exe',
      'CC1PLUS.EXE',
      'ld.exe',
    ]) {
      expect(isCompilerProgram(program), program).toBe(true);
    }
  });

  it('do not match the app, the webview, the drivers or the program', () => {
    for (const program of [
      '/tmp/b2c-e2e/blocks2cpp-desktop',
      'WebKitWebProcess',
      'WebKitNetworkProcess',
      'WebKitWebDriver',
      'tauri-driver',
      'msedgewebview2.exe',
      'msedgedriver.exe',
      'main',
      'main.exe',
      'gas',
      'bash',
      'systemd-run',
      'glass',
      '',
    ]) {
      expect(isCompilerProgram(program), program).toBe(false);
    }
  });

  it('are read from tasklist output', () => {
    const text = [
      '"System Idle Process","0","Services","0","8 K"',
      '"g++.exe","4120","Console","1","12,345 K"',
      '"cc1plus.exe","4121","Console","1","98,765 K"',
      'INFO: something else',
      '',
    ].join('\r\n');
    expect(parseTasklist(text)).toEqual([
      { program: 'System Idle Process', pid: 0 },
      { program: 'g++.exe', pid: 4120 },
      { program: 'cc1plus.exe', pid: 4121 },
    ]);
  });
});

describe('the malicious-project table', () => {
  it('is read cell by cell, with escaped pipes kept in their cell', () => {
    expect(tableCells('| `a.b2c` | T1 | x \\| y | accepted | clean |  |')).toEqual([
      '`a.b2c`',
      'T1',
      'x \\| y',
      'accepted',
      'clean',
      '',
    ]);
    const table = [
      '| File | Threat | Attack | Loader | Resolve | Later stages |',
      '|------|--------|--------|--------|---------|--------------|',
      '| `ok.b2c` | T3 | text | accepted | clean | |',
      '| `bad.b2c` | T5 | text | `B2C-E0110`, `B2C-E0199` | — | |',
      '| Limit | Code | Test |',
    ].join('\n');
    expect(parseSuiteTable(table)).toEqual([
      { file: 'ok.b2c', loader: 'accepted' },
      { file: 'bad.b2c', loader: ['B2C-E0110', 'B2C-E0199'] },
    ]);
  });

  it('refuses a loader cell that is neither accepted nor codes', () => {
    expect(() => parseSuiteTable('| `x.b2c` | T1 | a | rejected | — | |')).toThrow(SuiteTableError);
    expect(() => parseSuiteTable('| `x.b2c` | T1 | a | `B2C-E0110` and more | — | |')).toThrow(
      SuiteTableError,
    );
  });

  it('lists every file of tests/security/projects exactly once', () => {
    const suite = readSuite();
    expect(suite.length).toBeGreaterThan(80);
    expect(suite.some((entry) => entry.loader === 'accepted')).toBe(true);
    expect(suite.some((entry) => entry.loader !== 'accepted')).toBe(true);
  });
});

describe('call outcomes', () => {
  const answered: IpcOutcome = { kind: 'answered', value: { ok: true } };
  const refused: IpcOutcome = { kind: 'refused', error: { code: 'restricted' } };
  const tauriRefused: IpcOutcome = { kind: 'refused', error: 'event.listen not allowed' };
  const dropped: IpcOutcome = { kind: 'dropped' };
  const threw: IpcOutcome = { kind: 'threw', error: 'TypeError' };

  it('come from the page’s report; one still pending was dropped', () => {
    expect(outcomeOf({ name: 'a', state: 'pending' })).toEqual(dropped);
    expect(outcomeOf({ name: 'a', state: 'answered', value: 1 })).toEqual({
      kind: 'answered',
      value: 1,
    });
    expect(outcomeOf({ name: 'a', state: 'refused', error: { code: 'x' } })).toEqual({
      kind: 'refused',
      error: { code: 'x' },
    });
    expect(outcomeOf({ name: 'a', state: 'threw', error: 'boom' })).toEqual({
      kind: 'threw',
      error: 'boom',
    });
  });

  it('give the IPC error code only for a typed refusal', () => {
    expect(errorCode(refused)).toBe('restricted');
    expect(errorCode(tauriRefused)).toBeNull();
    expect(errorCode(answered)).toBeNull();
    expect(errorCode(dropped)).toBeNull();
    expect(errorCode({ kind: 'refused', error: { code: 7 } })).toBeNull();
  });

  it('match only what is expected; an answer is never an expected abuse outcome', () => {
    expect(mismatch(dropped, 'dropped')).toBeNull();
    expect(mismatch(refused, 'dropped')).toContain('expected dropped');
    expect(mismatch(answered, 'dropped')).toContain('answered');
    expect(mismatch(threw, 'dropped')).not.toBeNull();
    expect(mismatch(refused, { code: 'restricted' })).toBeNull();
    expect(mismatch(refused, { code: 'unknownHandle' })).not.toBeNull();
    expect(mismatch(dropped, { code: 'restricted' })).not.toBeNull();
    expect(mismatch(tauriRefused, 'refused')).toBeNull();
    expect(mismatch(refused, 'refused')).not.toBeNull();
    for (const outcome of [refused, tauriRefused, dropped, threw]) {
      expect(mismatch(outcome, 'notAccepted')).toBeNull();
    }
    expect(mismatch(answered, 'notAccepted')).not.toBeNull();
    expect(describeExpected({ code: 'x' })).toBe('refused with x');
    expect(describeOutcome({ kind: 'answered', value: 'y'.repeat(400) }).length).toBeLessThan(330);
  });

  it('put channel messages in Tauri’s order and drop the end marker', () => {
    expect(
      orderedMessages([
        { index: 1, message: 'b' },
        { index: 2, end: true },
        { index: 0, message: 'a' },
        'not a message',
      ]),
    ).toEqual(['a', 'b']);
  });
});

describe('abuse cases', () => {
  const samples: Samples = readSamples();
  const commands = Object.keys(samples);

  it('cover every command, each case named once per batch', () => {
    expect(commands.length).toBeGreaterThanOrEqual(36);
    for (const cases of [
      hookMutations(samples),
      protoCases(),
      limitCases(samples),
      forgedIdCases(samples),
      documentCases(samples),
      bypassCases(samples),
    ]) {
      const names = cases.map((entry) => entry.name);
      expect(new Set(names).size).toBe(names.length);
    }
    const mutated = new Set(hookMutations(samples).map((entry) => entry.cmd));
    expect([...mutated].sort()).toEqual([...commands].sort());
  });

  it('send nothing that the hook should pass and that would act (every mutation is dropped)', () => {
    expect(hookMutations(samples).every((entry) => entry.expected === 'dropped')).toBe(true);
    // A message around the hook with an empty body runs a command that takes no arguments:
    // none is sent (app_quit would end the app).
    for (const entry of bypassCases(samples)) {
      if (entry.route === 'aroundHookEmpty' && entry.cmd in samples) {
        expect(Object.keys(samples[entry.cmd] ?? {}), entry.cmd).not.toHaveLength(0);
      }
      expect(entry.expected).toBe('notAccepted');
    }
    for (const cmd of FOREIGN_COMMANDS) {
      expect(commands).not.toContain(cmd);
    }
  });

  it('put the limits at their edges', () => {
    expect(zeroBytesBase64(0)).toBe('');
    expect(zeroBytesBase64(1)).toBe('AA==');
    expect(zeroBytesBase64(2)).toBe('AAA=');
    expect(zeroBytesBase64(3)).toBe('AAAA');
    expect(Buffer.from(zeroBytesBase64(MAX_INPUT_BYTES), 'base64')).toHaveLength(MAX_INPUT_BYTES);
    expect(zeroBytesBase64(MAX_INPUT_BYTES)).toHaveLength(87_384);
    expect(zeroBytesBase64(MAX_INPUT_BYTES + 1)).toHaveLength(87_384);
    const malformed = malformedIds('ph_', 32);
    expect(malformed.every((id) => !/^ph_[0-9a-f]{32}$/.test(id))).toBe(true);
  });

  it('build __proto__ keys as JSON text', () => {
    expect(JSON.parse(withProtoKey({}))).toHaveProperty('__proto__');
    const parsed = JSON.parse(withProtoKey({ handle: 'x' })) as Record<string, unknown>;
    expect(Object.keys(parsed)).toEqual(['__proto__', 'handle']);
  });
});

describe('page checks', () => {
  const clean: PageState = {
    canary: null,
    injected: 0,
    frames: 1,
    href: 'tauri://localhost',
    sentinel: 's',
  };

  it('report script that ran and markup that became elements', () => {
    expect(injectionProblems(clean)).toEqual([]);
    expect(injectionProblems({ ...clean, canary: 'name' })).toHaveLength(1);
    expect(injectionProblems({ ...clean, injected: 2 })).toHaveLength(1);
  });

  it('read text as it is shown', () => {
    expect(shownText('\u00a0a\u00a0 b\n c ')).toBe('a b c');
  });
});
