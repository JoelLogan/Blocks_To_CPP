/**
 * Calling the backend from the webview the way injected script could (docs/spec/08-security.md
 * §8.8 and §8.13): with Tauri's internal `window.__TAURI_INTERNALS__`, which exists in every build
 * whatever `withGlobalTauri` says, and never with the app's own IPC client or the test hook.
 *
 * Three routes:
 *
 * - `invoke`: `__TAURI_INTERNALS__.invoke`, the route the app uses. The message passes the
 *   isolation hook, which drops it (the promise never settles) unless the allowlist names the
 *   command and the payload has exactly its shape; the backend then checks it again.
 * - `aroundHook`: `__TAURI_INTERNALS__.postMessage` with the payload as plain JSON, which sends it
 *   straight to the IPC endpoint without the isolation frame. The backend expects an encrypted
 *   payload and refuses it.
 * - `aroundHookEmpty`: the same with an empty body, which Tauri does not try to decrypt. Only a
 *   command that takes no argument can run that way; every other command, and every command the
 *   capability does not grant, is refused.
 *
 * Whether a message was dropped is decided against a control call: once `app_info` has been
 * answered after it, and a grace period has passed, a call that is still unanswered was never
 * sent.
 */
import type { ToolchainListResponse } from '@blocks2cpp/ipc-types';
import type { WebDriver } from 'selenium-webdriver';

import { waitFor } from '../../../support/wait';

/** How a call reaches the backend (see the module comment). */
export type IpcRoute = 'invoke' | 'aroundHook' | 'aroundHookEmpty';

/**
 * An argument value built in the webview instead of being sent through WebDriver:
 * a new channel whose messages are kept under `name` (`{$channel: name}`), a long string
 * (`{$repeat: {text, count}}`), or JSON parsed in the page so that keys such as `__proto__` stay
 * own properties (`{$json: text}`).
 */
export type PageValue =
  | { readonly $channel: string }
  | { readonly $repeat: { readonly text: string; readonly count: number } }
  | { readonly $json: string };

/** One command call. */
export interface IpcCall {
  /** A name for the reports, unique within a batch. */
  readonly name: string;
  /** The command, as Tauri names it. */
  readonly cmd: string;
  /** The arguments object (default `{}`); may hold {@link PageValue}s at any depth. */
  readonly args?: unknown;
  /** How the call reaches the backend (default `invoke`). */
  readonly route?: IpcRoute;
}

/** What became of a call. */
export type IpcOutcome =
  /** The command ran and answered. */
  | { readonly kind: 'answered'; readonly value: unknown }
  /** The backend (or Tauri in front of it) refused the call with this error. */
  | { readonly kind: 'refused'; readonly error: unknown }
  /** No answer after a later call was answered: the isolation hook dropped the message. */
  | { readonly kind: 'dropped' }
  /** The page threw before anything was sent. */
  | { readonly kind: 'threw'; readonly error: unknown };

/** Options of {@link IpcProbe.run}. */
export interface RunOptions {
  /**
   * Wait for each call (up to `settleMs`) before sending the next one. Needed for commands that
   * must not overlap (a native dialog at a time) or that depend on each other.
   */
  readonly sequential?: boolean;
  /** How long a sequential call may take before the next one is sent (default 30 s). */
  readonly settleMs?: number;
  /** How long a call may stay unanswered after the control call was answered (default 1.5 s). */
  readonly graceMs?: number;
}

/** The longest time the control call may take before the IPC counts as broken. */
const CONTROL_TIMEOUT_MS = 10_000;

/** The default of {@link RunOptions.settleMs}. */
const DEFAULT_SETTLE_MS = 30_000;

/** The default of {@link RunOptions.graceMs}. */
const DEFAULT_GRACE_MS = 1_500;

/** The session's script timeout that the harness sets (support/app.ts), restored after a run. */
const DEFAULT_SCRIPT_TIMEOUT_MS = 30_000;

/** Time for WebDriver itself on top of what a run may take in the page. */
const SCRIPT_MARGIN_MS = 10_000;

/** The window global under which the page keeps the channels' messages. */
export const CHANNELS_GLOBAL = '__b2cSecurityChannels';

/**
 * Runs in the webview (`executeAsyncScript`): sends the calls of `arguments[0]` with the options
 * `arguments[1]`, then, when a call is still unanswered, the control call and the grace period, and
 * calls back with `{control, entries}` (`control` is `notNeeded` when every call settled). A plain script
 * (not a function from this file), so nothing the test runner adds to compiled code reaches the
 * page; the page's CSP forbids `eval` and `new Function`, so the script builds nothing from text.
 */
const RUN_SCRIPT = `
  const done = arguments[arguments.length - 1];
  const calls = arguments[0];
  const options = arguments[1];
  const internals = window.__TAURI_INTERNALS__;
  if (typeof internals !== 'object' || internals === null || typeof internals.invoke !== 'function'
      || typeof internals.postMessage !== 'function' || typeof internals.transformCallback !== 'function') {
    done({ fatal: 'window.__TAURI_INTERNALS__ has no invoke, postMessage or transformCallback' });
    return;
  }
  const channelsName = options.channelsGlobal;
  if (typeof window[channelsName] !== 'object' || window[channelsName] === null) {
    window[channelsName] = Object.create(null);
  }
  const channels = window[channelsName];
  const plain = (error) => {
    if (typeof error === 'string') {
      return error;
    }
    try {
      const text = JSON.stringify(error);
      return text === undefined ? String(error) : JSON.parse(text);
    } catch (problem) {
      return String(error);
    }
  };
  const define = (object, key, value) => {
    Object.defineProperty(object, key, { value, enumerable: true, writable: true, configurable: true });
  };
  const build = (value) => {
    if (Array.isArray(value)) {
      return value.map(build);
    }
    if (typeof value !== 'object' || value === null) {
      return value;
    }
    const keys = Object.keys(value);
    if (keys.length === 1 && keys[0] === '$channel') {
      const name = String(value.$channel);
      if (!Array.isArray(channels[name])) {
        channels[name] = [];
      }
      const list = channels[name];
      return '__CHANNEL__:' + internals.transformCallback((raw) => { list.push(raw); });
    }
    if (keys.length === 1 && keys[0] === '$repeat') {
      return String(value.$repeat.text).repeat(Number(value.$repeat.count));
    }
    if (keys.length === 1 && keys[0] === '$json') {
      return JSON.parse(String(value.$json));
    }
    const copy = {};
    for (const key of keys) {
      define(copy, key, build(value[key]));
    }
    return copy;
  };
  const send = (call) => {
    const args = build(call.args === undefined ? {} : call.args);
    if (call.route === 'invoke') {
      return internals.invoke(call.cmd, args);
    }
    return new Promise((resolve, reject) => {
      const callback = internals.transformCallback(resolve, true);
      const error = internals.transformCallback(reject, true);
      const payload = call.route === 'aroundHookEmpty' ? new Uint8Array(0) : args;
      internals.postMessage({ cmd: call.cmd, callback, error, payload, options: {} });
    });
  };
  const wait = (ms) => new Promise((resolve) => { setTimeout(resolve, ms); });
  (async () => {
    const entries = [];
    for (const call of calls) {
      const entry = { name: call.name, state: 'pending' };
      entries.push(entry);
      let settled;
      try {
        settled = Promise.resolve(send(call)).then(
          (value) => { entry.state = 'answered'; entry.value = value === undefined ? null : value; },
          (error) => { entry.state = 'refused'; entry.error = plain(error); },
        );
      } catch (error) {
        entry.state = 'threw';
        entry.error = plain(error instanceof Error ? error.message : error);
        settled = Promise.resolve();
      }
      if (options.sequential) {
        await Promise.race([settled, wait(options.settleMs)]);
      }
    }
    // Something unanswered: a later call must be answered first, then the grace period passes.
    let control = 'notNeeded';
    if (entries.some((entry) => entry.state === 'pending')) {
      control = 'pending';
      internals.invoke('app_info', {}).then(
        () => { control = 'answered'; },
        () => { control = 'refused'; },
      );
      const deadline = Date.now() + options.controlMs;
      while (control === 'pending' && Date.now() < deadline) {
        await wait(20);
      }
      await wait(options.graceMs);
    }
    done({ control, entries });
  })();
`;

/** The page's report of one call. */
interface RawEntry {
  readonly name: string;
  readonly state: 'pending' | 'answered' | 'refused' | 'threw';
  readonly value?: unknown;
  readonly error?: unknown;
}

/** The IPC from the webview stopped answering, or the page has no `__TAURI_INTERNALS__`. */
export class IpcProbeError extends Error {
  override readonly name = 'IpcProbeError';
}

/** The outcome of a page entry (an entry still pending at the end was dropped). */
export function outcomeOf(entry: RawEntry): IpcOutcome {
  switch (entry.state) {
    case 'answered':
      return { kind: 'answered', value: entry.value ?? null };
    case 'refused':
      return { kind: 'refused', error: entry.error ?? null };
    case 'threw':
      return { kind: 'threw', error: entry.error ?? null };
    case 'pending':
      return { kind: 'dropped' };
  }
}

/** Whether a value is a plain object (a JSON object). */
function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** The `code` of an `IpcError` the backend refused a call with, or `null` (dropped, answered, …). */
export function errorCode(outcome: IpcOutcome): string | null {
  if (outcome.kind !== 'refused' || !isRecord(outcome.error)) {
    return null;
  }
  const code = outcome.error['code'];
  return typeof code === 'string' ? code : null;
}

/** JSON text of a value, or `undefined` for one JSON cannot show (undefined, a function). */
function jsonText(value: unknown): string | undefined {
  return JSON.stringify(value);
}

/** A short description of an outcome for messages (values and errors cut at 300 characters). */
export function describeOutcome(outcome: IpcOutcome): string {
  const shown = (value: unknown): string => {
    const text = typeof value === 'string' ? value : (jsonText(value) ?? String(value));
    return text.length > 300 ? `${text.slice(0, 300)}…` : text;
  };
  switch (outcome.kind) {
    case 'answered':
      return `answered ${shown(outcome.value)}`;
    case 'refused':
      return `refused with ${shown(outcome.error)}`;
    case 'dropped':
      return 'dropped (never answered)';
    case 'threw':
      return `threw in the page: ${shown(outcome.error)}`;
  }
}

/**
 * What a call must come to:
 * - `dropped`: the isolation hook never sent it;
 * - `{code}`: the backend refused it with this `IpcError` code;
 * - `refused`: refused before any command ran (Tauri's own error text, such as a payload it
 *   cannot decrypt or a command the capability does not grant), not an `IpcError`;
 * - `notAccepted`: anything but an answer.
 */
export type Expected = 'dropped' | 'refused' | 'notAccepted' | { readonly code: string };

/** The text of an expectation, for messages. */
export function describeExpected(expected: Expected): string {
  if (typeof expected === 'string') {
    return expected === 'notAccepted' ? 'not accepted' : expected;
  }
  return `refused with ${expected.code}`;
}

/** `null` when `outcome` is what `expected` asks for, else what is wrong. */
export function mismatch(outcome: IpcOutcome, expected: Expected): string | null {
  const wrong = `expected ${describeExpected(expected)}, but it was ${describeOutcome(outcome)}`;
  if (expected === 'notAccepted') {
    return outcome.kind === 'answered' ? wrong : null;
  }
  if (expected === 'dropped') {
    return outcome.kind === 'dropped' ? null : wrong;
  }
  if (expected === 'refused') {
    return outcome.kind === 'refused' && errorCode(outcome) === null ? null : wrong;
  }
  return errorCode(outcome) === expected.code ? null : wrong;
}

/** One abuse case: a call and what must become of it. */
export interface AbuseCase extends IpcCall {
  readonly expected: Expected;
}

/** One raw channel message: `{index, message}`, or `{index, end: true}` at the end. */
interface RawChannelMessage {
  readonly index?: unknown;
  readonly message?: unknown;
  readonly end?: unknown;
}

/** The messages of a channel in order (Tauri numbers them; the end marker is left out). */
export function orderedMessages(raw: readonly unknown[]): unknown[] {
  return raw
    .filter((entry): entry is RawChannelMessage => isRecord(entry) && !('end' in entry))
    .map((entry) => ({ index: Number(entry.index), message: entry.message }))
    .sort((a, b) => a.index - b.index)
    .map((entry) => entry.message);
}

/** Calls commands from the webview of the app under test; see the module comment. */
export class IpcProbe {
  readonly #driver: WebDriver;

  constructor(driver: WebDriver) {
    this.#driver = driver;
  }

  /**
   * Sends `calls` (at the same time, or one after the other with `sequential`) and returns each
   * call's outcome by name.
   *
   * @throws IpcProbeError when the page has no `__TAURI_INTERNALS__` or the control call was not
   * answered, so that a broken IPC never passes for dropped messages.
   */
  async run(calls: readonly IpcCall[], options: RunOptions = {}): Promise<Map<string, IpcOutcome>> {
    const names = new Set<string>();
    for (const call of calls) {
      if (names.has(call.name)) {
        throw new IpcProbeError(`Two calls are named ${call.name}`);
      }
      names.add(call.name);
    }
    const sent = calls.map((call) => ({
      name: call.name,
      cmd: call.cmd,
      args: call.args ?? {},
      route: call.route ?? 'invoke',
    }));
    const sequential = options.sequential === true;
    const settleMs = options.settleMs ?? DEFAULT_SETTLE_MS;
    const graceMs = options.graceMs ?? DEFAULT_GRACE_MS;
    // WebDriver ends an asynchronous script after the session's script timeout.
    const budget = (sequential ? calls.length * settleMs : 0) + CONTROL_TIMEOUT_MS + graceMs;
    await this.#driver.manage().setTimeouts({ script: budget + SCRIPT_MARGIN_MS });
    let report: unknown;
    try {
      report = await this.#driver.executeAsyncScript(RUN_SCRIPT, sent, {
        sequential,
        settleMs,
        graceMs,
        controlMs: CONTROL_TIMEOUT_MS,
        channelsGlobal: CHANNELS_GLOBAL,
      });
      await this.#driver.manage().setTimeouts({ script: DEFAULT_SCRIPT_TIMEOUT_MS });
    } catch (error: unknown) {
      // An abuse that got through may have ended the app (app_quit) or replaced the page.
      throw new IpcProbeError(
        `A batch of ${String(calls.length)} calls (${calls
          .slice(0, 3)
          .map((call) => call.cmd)
          .join(
            ', ',
          )}${calls.length > 3 ? ', …' : ''}) did not finish: ${error instanceof Error ? `${error.name} ${error.message}` : String(error)}. If the app ended, a call that should have been refused ran; the app's log (kept with the test's artifacts) names every command that ran.`,
        { cause: error },
      );
    }
    if (!isRecord(report)) {
      throw new IpcProbeError('The page returned no report');
    }
    if (typeof report['fatal'] === 'string') {
      throw new IpcProbeError(report['fatal']);
    }
    if (report['control'] !== 'answered' && report['control'] !== 'notNeeded') {
      throw new IpcProbeError(
        `The control call app_info was not answered (${String(report['control'])}): the IPC is broken, so no call can count as dropped`,
      );
    }
    const entries = Array.isArray(report['entries']) ? (report['entries'] as RawEntry[]) : [];
    return new Map(entries.map((entry) => [entry.name, outcomeOf(entry)]));
  }

  /** Sends one call and waits for it (up to `settleMs`); see {@link IpcProbe.run}. */
  async call(call: Omit<IpcCall, 'name'>, options: RunOptions = {}): Promise<IpcOutcome> {
    const outcomes = await this.run([{ ...call, name: 'call' }], { sequential: true, ...options });
    const outcome = outcomes.get('call');
    if (outcome === undefined) {
      throw new IpcProbeError(`No outcome for ${call.cmd}`);
    }
    return outcome;
  }

  /**
   * Calls a command through the isolation hook and returns its answer.
   *
   * @throws IpcProbeError with the outcome when it was not answered.
   */
  async answer<T>(cmd: string, args: unknown = {}, options: RunOptions = {}): Promise<T> {
    const outcome = await this.call({ cmd, args }, options);
    if (outcome.kind !== 'answered') {
      throw new IpcProbeError(`${cmd} was not answered: ${describeOutcome(outcome)}`);
    }
    return outcome.value as T;
  }

  /**
   * Sends every case and returns a line for each one that did not come to what it expects (an
   * empty list when all did).
   */
  async check(cases: readonly AbuseCase[], options: RunOptions = {}): Promise<string[]> {
    const outcomes = await this.run(cases, options);
    const problems: string[] = [];
    for (const abuse of cases) {
      const outcome = outcomes.get(abuse.name);
      const problem =
        outcome === undefined ? 'no outcome was reported' : mismatch(outcome, abuse.expected);
      if (problem !== null) {
        problems.push(`${abuse.name} (${abuse.cmd} by ${abuse.route ?? 'invoke'}): ${problem}`);
      }
    }
    return problems;
  }

  /** The messages channel `name` has received so far, in order. */
  async channelMessages(name: string): Promise<unknown[]> {
    const raw: unknown = await this.#driver.executeScript(
      'const all = window[arguments[0]]; const list = all && all[arguments[1]]; return Array.isArray(list) ? list : [];',
      CHANNELS_GLOBAL,
      name,
    );
    return orderedMessages(Array.isArray(raw) ? raw : []);
  }

  /** Waits until channel `name` has a message that `matches` accepts, and returns it. */
  async waitForMessage(
    name: string,
    matches: (message: unknown) => boolean,
    timeout: number,
  ): Promise<unknown> {
    return waitFor(async () => (await this.channelMessages(name)).find(matches) ?? null, {
      timeout,
      interval: 200,
      message: async () =>
        `a matching message on channel ${name} (it has ${JSON.stringify(await this.channelMessages(name)).slice(0, 1000)})`,
    });
  }
}

/** The `kind` of a channel message (`finished`, `exit`, …), or `null`. */
export function messageKind(message: unknown): string | null {
  return isRecord(message) && typeof message['kind'] === 'string' ? message['kind'] : null;
}

/**
 * Waits until toolchain discovery has finished with at least one usable g++, so that no
 * discovery probe runs (and starts the compiler) while a test watches for compilers.
 */
export async function waitForToolchains(probe: IpcProbe, timeout = 60_000): Promise<void> {
  await waitFor(
    async () => {
      const list = await probe.answer<ToolchainListResponse>('toolchain_list');
      return !list.discovering && list.toolchains.some((toolchain) => toolchain.usable);
    },
    { timeout, interval: 500, message: 'toolchain discovery to finish with a usable g++' },
  );
}
