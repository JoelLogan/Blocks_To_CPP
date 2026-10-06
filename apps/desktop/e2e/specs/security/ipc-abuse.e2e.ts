/**
 * Nightly E2E security tests 3 and 4 (docs/spec/08-security.md §8.8 and §8.13; threat T8 of
 * §8.12, IPC abuse after XSS): what script injected into the editor could send to the backend.
 *
 * - 3: a command the allowlist does not name is dropped by the isolation hook: the call is never
 *   answered (the hook throws in its frame, so nothing is sent and no promise settles), and the
 *   backend's log never mentions it.
 * - 4: unknown fields, wrong types, `__proto__` keys, oversized documents and program input, an
 *   out-of-range terminal size and malformed IDs are dropped by the hook; well-formed forged IDs
 *   and documents the strict loader refuses reach the backend and are refused with typed errors.
 * - Around the hook: a message sent straight to the IPC endpoint with
 *   `__TAURI_INTERNALS__.postMessage` is refused (a plain JSON body cannot be decrypted; an empty
 *   body carries no arguments, and the capability grants no other command).
 *
 * Every case says exactly what must become of it (lib/abuse.ts); a test fails on any case that
 * was answered or ended otherwise, listing them all.
 */
import { readFileSync } from 'node:fs';

import type { RecentListResponse, RecoveryListResponse } from '@blocks2cpp/ipc-types';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import { launchApp } from '../../support/app';
import {
  bypassCases,
  documentCases,
  FOREIGN_COMMANDS,
  forgedIdCases,
  hookMutations,
  limitCases,
  protoCases,
  readSamples,
} from './lib/abuse';
import { type AbuseCase, IpcProbe } from './lib/ipc';
import { appLog, profileOf } from './lib/projects';

/** The log level the app gets in these tests, so that the log check sees everything. */
const LOG_LEVEL = 'debug';

describe('E2E security 3 and 4: IPC abuse from the webview', () => {
  const samples = readSamples();
  let previousLogLevel: string | undefined;

  beforeAll(() => {
    previousLogLevel = process.env['B2C_LOG'];
    process.env['B2C_LOG'] = LOG_LEVEL;
  });

  afterAll(() => {
    if (previousLogLevel === undefined) {
      Reflect.deleteProperty(process.env, 'B2C_LOG');
    } else {
      process.env['B2C_LOG'] = previousLogLevel;
    }
  });

  it('drops commands that are not on the allowlist before the backend sees them (3)', async (context) => {
    const app = await launchApp(context);
    const probe = new IpcProbe(app.driver);
    const cases: AbuseCase[] = [
      ...FOREIGN_COMMANDS.map((cmd): AbuseCase => ({
        name: `invoke ${JSON.stringify(cmd)}`,
        cmd,
        expected: 'dropped',
      })),
      // Tauri's channel fetch is allowed only with a null payload.
      {
        name: 'channel fetch with a payload',
        cmd: 'plugin:__TAURI_CHANNEL__|fetch',
        args: { id: 1 },
        expected: 'dropped',
      },
    ];
    expect(await probe.check(cases)).toEqual([]);

    // The backend never saw them: its debug log names none of them.
    const log = readFileSync(appLog(profileOf(app)), 'utf8');
    const lines = log.split('\n').filter((line) => line.trim() !== '');
    expect(
      lines.some((line) => line.includes('"level":"DEBUG"')),
      'a debug log',
    ).toBe(true);
    const named = FOREIGN_COMMANDS.filter((cmd) =>
      lines.some(
        (line) => line.includes(cmd.trim()) || line.includes(JSON.stringify(cmd).slice(1, -1)),
      ),
    );
    expect(named, 'foreign commands named in the backend log').toEqual([]);
  });

  it('drops malformed messages of every command at the isolation hook (4)', async (context) => {
    const app = await launchApp(context);
    const probe = new IpcProbe(app.driver);
    const cases = [...hookMutations(samples), ...protoCases()];
    expect(cases.length).toBeGreaterThan(Object.keys(samples).length * 3);
    expect(await probe.check(cases)).toEqual([]);
    // No `__proto__` key reached a prototype of the page (freezePrototype is on as well).
    const polluted: unknown = await app.driver.executeScript(
      'return ({}).b2cPolluted !== undefined || Object.prototype.hasOwnProperty.call(Object.prototype, "b2cPolluted");',
    );
    expect(polluted).toBe(false);
  });

  it('refuses oversized payloads, out-of-range sizes and forged IDs at the hook or in the backend (4)', async (context) => {
    const app = await launchApp(context);
    const probe = new IpcProbe(app.driver);
    // Each group on its own: the oversized documents are 32 MiB strings in the page.
    for (const cases of [limitCases(samples), forgedIdCases(samples), documentCases(samples)]) {
      expect(await probe.check(cases)).toEqual([]);
    }
    // None of it opened a project or wrote a snapshot, and the backend still answers.
    expect(await probe.answer<RecentListResponse>('recent_list')).toEqual({ entries: [] });
    expect(await probe.answer<RecoveryListResponse>('recovery_list')).toEqual({ snapshots: [] });
  });

  it('refuses messages sent around the isolation frame (3, 4)', async (context) => {
    const app = await launchApp(context);
    const probe = new IpcProbe(app.driver);
    expect(await probe.check(bypassCases(samples))).toEqual([]);
  });
});
