/**
 * The preview worker's check (ADR-0016 §2 "Verify first", docs/plans/m3-contracts/w5-worker.md):
 * starts a preview worker of its own beside the editor's, as the app will (the page's compiled
 * `Module` posted to the packaged module script under the worker's policy), runs the open
 * project's preview in it and on the main thread, and measures what the spike asks for: the
 * worker's start-up, a preview's round trip through `postMessage` against the main-thread call,
 * and the structured-clone cost of the largest messages. Its probe tells which policy the worker
 * runs under. End-to-end builds only.
 *
 * The test starts it with {@link startWorkerCheck} and polls {@link WorkerCheck.result}: a hook
 * call cannot wait for a promise on every driver.
 */
import type { CoreVersion, CoreWasm, PreviewOptions } from '@blocks2cpp/b2c-core-wasm';
import {
  type CoreWorkerClient,
  type WorkerLike,
  type WorkerProbe,
} from '@blocks2cpp/b2c-core-wasm/worker';

import type { E2eWorkerCheck } from './contract';

/** The most rounds one check runs. */
export const MAX_WORKER_CHECK_ROUNDS = 50;

/**
 * How often each message is cloned to time it: the clock of a webview may count whole
 * milliseconds, so one clone is timed as the mean of many.
 */
const CLONE_REPEATS = 20;

/** What the check needs. */
export interface WorkerCheckDeps {
  /** The open project's document as JSON text, or `null` when none is open. */
  readonly documentJson: () => string | null;
  /** The main thread's core. */
  readonly core: () => CoreWasm | null;
  /** The machine's preview options. */
  readonly options: () => PreviewOptions;
  /** Creates a worker client over the app's worker. */
  readonly client: (acceptVersion: (version: CoreVersion) => boolean) => CoreWorkerClient;
  /** Creates a worker from a plain string URL, without the Trusted Types policy. */
  readonly spawnUnwrapped: () => WorkerLike;
  /** How many Content Security Policy violations the page has seen. */
  readonly violations: () => number;
  /** The probe's request to the IPC endpoint, made from the page: the control of the worker's. */
  readonly pageRequest: () => Promise<WorkerProbe['request']>;
  /** The page's clock. */
  readonly now?: () => number;
}

/** A running or finished check. */
export interface WorkerCheck {
  /** The result, or `null` while the check runs. */
  result(): E2eWorkerCheck | null;
}

function rounded(value: number): number {
  return Math.round(value * 100) / 100;
}

/** How long one `structuredClone` of `message` takes: the mean of {@link CLONE_REPEATS}. */
function cloneMs(message: unknown, now: () => number): number {
  const started = now();
  for (let repeat = 0; repeat < CLONE_REPEATS; repeat += 1) {
    structuredClone(message);
  }
  return rounded((now() - started) / CLONE_REPEATS);
}

/** Waits for a macrotask, so that a violation event queued by the webview is delivered. */
function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 100));
}

/** The name and nothing else of a failure: messages could quote project text. */
function describe(error: unknown): string {
  return error instanceof Error ? `${error.name}: ${error.message.slice(0, 200)}` : 'unknown error';
}

async function run(deps: WorkerCheckDeps, rounds: number): Promise<E2eWorkerCheck> {
  const now = deps.now ?? (() => performance.now());
  const documentJson = deps.documentJson();
  const core = deps.core();
  if (documentJson === null || core === null) {
    throw new Error('No project is open, or the compiler core has not started');
  }
  const options = deps.options();
  const mainVersion = core.version();

  // Trusted Types: what the trial reports for a worker made from a plain string.
  const before = deps.violations();
  deps.spawnUnwrapped().terminate();
  await settle();
  const unwrappedViolations = deps.violations() - before;

  const started = now();
  const client = deps.client((version) => JSON.stringify(version) === JSON.stringify(mainVersion));
  try {
    await client.ready();
    const startupMs = now() - started;
    await settle();
    const policyViolations = deps.violations() - before - unwrappedViolations;

    const firstStarted = now();
    const first = await client.update(documentJson, options);
    const firstMs = now() - firstStarted;
    if (first === null) {
      throw new Error('The first update was superseded');
    }
    const main = core.update(documentJson, options);
    const sameAsMain = JSON.stringify(first.result) === JSON.stringify(main);

    const roundTripMs: number[] = [];
    const workerUpdateMs: number[] = [];
    const mainUpdateMs: number[] = [];
    for (let round = 0; round < rounds; round += 1) {
      const sent = now();
      const answer = await client.update(documentJson, options);
      roundTripMs.push(now() - sent);
      workerUpdateMs.push(answer?.updateMs ?? Number.NaN);
      const called = now();
      core.update(documentJson, options);
      mainUpdateMs.push(now() - called);
    }

    const updateJson = JSON.stringify(main);
    const request = { p: 1, kind: 'update', id: 1, documentJson, optionsJson: '{}' };
    const reply = {
      p: 1,
      kind: 'updated',
      id: 1,
      updateJson,
      scopeJson: null,
      startedAt: 0,
      updateMs: 0,
      indexMs: 0,
    };
    const probe: WorkerProbe = await client.probe();
    const pageRequest = await deps.pageRequest();
    return {
      ok: true,
      error: null,
      origin: globalThis.location.origin,
      startupMs: rounded(startupMs),
      firstUpdateMs: rounded(firstMs),
      sameAsMain,
      contentHash: first.result.contentHash,
      roundTripMs: roundTripMs.map(rounded),
      workerUpdateMs: workerUpdateMs.map(rounded),
      mainUpdateMs: mainUpdateMs.map(rounded),
      requestChars: documentJson.length,
      replyChars: updateJson.length,
      requestCloneMs: cloneMs(request, now),
      replyCloneMs: cloneMs(reply, now),
      probe: { eval: probe.eval, request: probe.request, directives: [...probe.directives] },
      pageRequest,
      unwrappedViolations,
      policyViolations,
    };
  } finally {
    client.dispose();
  }
}

/** Starts a check of `rounds` round trips (1 to {@link MAX_WORKER_CHECK_ROUNDS}). */
export function startWorkerCheck(deps: WorkerCheckDeps, rounds: number): WorkerCheck {
  if (!Number.isSafeInteger(rounds) || rounds < 1 || rounds > MAX_WORKER_CHECK_ROUNDS) {
    throw new TypeError(
      `rounds must be a whole number from 1 to ${String(MAX_WORKER_CHECK_ROUNDS)}`,
    );
  }
  let result: E2eWorkerCheck | null = null;
  run(deps, rounds).then(
    (done) => {
      result = done;
    },
    (error: unknown) => {
      result = { ok: false, error: describe(error) };
    },
  );
  return { result: () => result };
}
