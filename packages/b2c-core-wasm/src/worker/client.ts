/**
 * The page's side of the preview worker's protocol (ADR-0016 §3, docs/plans/m3-contracts/worker.md
 * §3): starts the worker with the page's compiled `WebAssembly.Module`, sends it one request at a
 * time with the newest waiting, checks every reply, and replaces a worker that fails. Three failed
 * starts in a row, or a webview that cannot post a `Module`, make the client fail for good; the
 * preview service then runs every update on the main thread.
 *
 * Restarts have one owner: a worker that trapped replaces its own instance and the client restarts
 * nothing; the client replaces the worker only when it fails to start, raises an `error` or
 * `messageerror` event, breaks the protocol, or does not answer an update in time (the watchdog).
 */

import type { CoreVersion, PreviewOptions, UpdateResult } from '../api';
import { boundText, parseUpdate, parseVersion, previewOptionsJson } from '../core';
import { CoreError, CoreTrap } from '../errors';
import { parseReply, WORKER_PROTOCOL, type WorkerProbe, type WorkerRequest } from './protocol';

/** The members of `Worker` the client uses (one end of a `MessageChannel` in tests). */
export interface WorkerLike {
  postMessage(message: WorkerRequest): void;
  onmessage: ((event: { readonly data: unknown }) => void) | null;
  onerror: ((event: unknown) => void) | null;
  onmessageerror: ((event: unknown) => void) | null;
  terminate(): void;
}

/** An answer of the worker to an `update`. */
export interface WorkerUpdate {
  readonly result: UpdateResult;
  /** What `scope_index()` returned, unparsed, or null (worker.md §4 parses it into a `ScopeIndex`). */
  readonly scopeJson: string | null;
  /** When `update()` started, on the page's clock (`performance.now()`). */
  readonly startedAt: number;
  /** How long `update()` alone took, in milliseconds. */
  readonly updateMs: number;
  /** How long `scope_index()` took, in milliseconds. */
  readonly indexMs: number;
}

/** The client's state: starting a worker (also while replacing one), ready, or failed for good. */
export type CoreWorkerState = 'starting' | 'ready' | 'failed';

/** The preview worker, from the page. */
export interface CoreWorkerClient {
  readonly state: CoreWorkerState;
  /** Whether a worker of this client has ever answered `started`. */
  readonly everStarted: boolean;
  /**
   * Resolves with the worker's core version once a worker is ready.
   *
   * @throws CoreError (`init`) when the client has failed.
   */
  ready(): Promise<CoreVersion>;
  /**
   * The worker's answer to `update()` of the document. Resolves `null` when a newer request
   * replaced this one before it was sent, or the client was disposed.
   *
   * @throws CoreTrap when the instance stopped or the worker was replaced while it ran this
   *   request; CoreError for an error envelope (`invalidOptions`) or, with kind `init`, when the
   *   client has failed (then run it on the main thread).
   */
  update(documentJson: string, options: PreviewOptions): Promise<WorkerUpdate | null>;
  /** End-to-end hook only: terminates the worker and handles it as an `error` event. */
  kill(): void;
  /**
   * End-to-end builds only: what the worker's probe finds.
   *
   * @throws CoreError (`init`) when the client has failed; CoreTrap when the worker was replaced.
   */
  probe(): Promise<WorkerProbe>;
  /** Terminates the worker for good; whatever waits resolves `null` (a probe rejects). */
  dispose(): void;
}

/** What {@link createCoreWorkerClient} is built from. */
export interface CoreWorkerClientOptions {
  /** Creates the worker; tests pass one end of a `MessageChannel`. */
  readonly spawn: () => WorkerLike;
  /** The page's compiled module (`coreModule()`). */
  readonly module: () => Promise<WebAssembly.Module>;
  /**
   * Whether the worker's core may serve this page: its version must equal the main instance's.
   * Every version is accepted without it; one that throws refuses.
   */
  readonly acceptVersion?: (version: CoreVersion) => boolean;
  /** From `start` to `started`, in milliseconds: 20,000 by default. */
  readonly startTimeoutMs?: number;
  /** From posting an `update` to its reply, in milliseconds: 30,000 by default. */
  readonly updateTimeoutMs?: number;
  /** Failed starts in a row before the client fails: 3 by default. */
  readonly maxFailedStarts?: number;
  /** The page's clock origin: `performance.timeOrigin` by default. */
  readonly timeOrigin?: number;
  /** Where the client reports what it did; `console` by default. Never given project text. */
  readonly log?: (message: string) => void;
}

export const DEFAULT_START_TIMEOUT_MS = 20_000;
export const DEFAULT_UPDATE_TIMEOUT_MS = 30_000;
export const DEFAULT_MAX_FAILED_STARTS = 3;

interface Deferred<T> {
  readonly resolve: (value: T) => void;
  readonly reject: (error: unknown) => void;
}

/** A request waiting to be sent. */
interface WaitingUpdate extends Deferred<WorkerUpdate | null> {
  readonly documentJson: string;
  readonly optionsJson: string;
}

/** The request the worker is working on. */
type InFlight =
  | ({ readonly kind: 'update'; readonly id: number } & Deferred<WorkerUpdate | null>)
  | ({ readonly kind: 'probe'; readonly id: number } & Deferred<WorkerProbe>);

/** Why a worker is replaced. */
type Replacement = 'startFailed' | 'startTimeout' | 'error' | 'protocol' | 'watchdog';

function trapOf(reason: Replacement): CoreTrap {
  return new CoreTrap(`The preview worker was replaced (${reason}) while it ran a request.`);
}

function failedError(): CoreError {
  return new CoreError('init', 'The preview worker could not be started.');
}

/** Creates the client and starts its first worker. */
export function createCoreWorkerClient(options: CoreWorkerClientOptions): CoreWorkerClient {
  const startTimeoutMs = options.startTimeoutMs ?? DEFAULT_START_TIMEOUT_MS;
  const updateTimeoutMs = options.updateTimeoutMs ?? DEFAULT_UPDATE_TIMEOUT_MS;
  const maxFailedStarts = options.maxFailedStarts ?? DEFAULT_MAX_FAILED_STARTS;
  const timeOrigin = options.timeOrigin ?? performance.timeOrigin;
  const log =
    options.log ??
    ((message: string) => {
      console.error(message);
    });

  let state: CoreWorkerState = 'starting';
  let everStarted = false;
  let disposed = false;
  let version: CoreVersion | null = null;
  let worker: WorkerLike | null = null;
  /** Whether the current worker has answered an `update` (its replacement is then no failed start). */
  let answered = false;
  let failedStarts = 0;
  let nextId = 1;
  let startTimer: ReturnType<typeof setTimeout> | null = null;
  let watchdog: ReturnType<typeof setTimeout> | null = null;
  let inFlight: InFlight | null = null;
  let waiting: WaitingUpdate | null = null;
  const probes: Deferred<WorkerProbe>[] = [];
  const readyWaiters: Deferred<CoreVersion>[] = [];

  function clearTimers(): void {
    if (startTimer !== null) {
      clearTimeout(startTimer);
      startTimer = null;
    }
    if (watchdog !== null) {
      clearTimeout(watchdog);
      watchdog = null;
    }
  }

  function stopWorker(): void {
    clearTimers();
    const stopping = worker;
    worker = null;
    if (stopping !== null) {
      stopping.onmessage = null;
      stopping.onerror = null;
      stopping.onmessageerror = null;
      stopping.terminate();
    }
  }

  /** Stops for good: every waiting request is told to run on the main thread. */
  function fail(): void {
    stopWorker();
    state = 'failed';
    log('The preview worker failed to start; the preview runs on the main thread.');
    const error = failedError();
    if (inFlight !== null) {
      inFlight.reject(new CoreTrap('The preview worker failed while it ran a request.'));
      inFlight = null;
    }
    waiting?.reject(error);
    waiting = null;
    for (const probe of probes.splice(0)) {
      probe.reject(error);
    }
    for (const waiter of readyWaiters.splice(0)) {
      waiter.reject(error);
    }
  }

  function replace(reason: Replacement): void {
    if (worker === null || disposed) {
      return;
    }
    // A failed start: a worker replaced before its first answer to an update, for any reason but
    // the watchdog (the same document would hang the main thread too).
    const failedStart = !answered && reason !== 'watchdog';
    log(`The preview worker is replaced (${reason}).`);
    stopWorker();
    if (inFlight !== null) {
      inFlight.reject(trapOf(reason));
      inFlight = null;
    }
    if (failedStart) {
      failedStarts += 1;
      if (failedStarts >= maxFailedStarts) {
        fail();
        return;
      }
    }
    spawn();
  }

  function send(request: WorkerRequest): boolean {
    try {
      worker?.postMessage(request);
      return true;
    } catch (error: unknown) {
      // A `DataCloneError`: this webview cannot post a `Module` (or the text), which will not
      // change by trying again.
      log(
        `The preview worker could not be sent a message (${error instanceof Error ? error.name : 'unknown error'}).`,
      );
      return false;
    }
  }

  function spawn(): void {
    state = 'starting';
    answered = false;
    let created: WorkerLike;
    try {
      created = options.spawn();
    } catch (error: unknown) {
      log(
        `The preview worker could not be created (${error instanceof Error ? error.name : 'unknown error'}).`,
      );
      failedStarts = maxFailedStarts;
      fail();
      return;
    }
    worker = created;
    created.onmessage = (event) => {
      if (worker === created) {
        receive(event.data);
      }
    };
    created.onerror = () => {
      if (worker === created) {
        replace('error');
      }
    };
    created.onmessageerror = () => {
      if (worker === created) {
        replace('error');
      }
    };
    startTimer = setTimeout(() => {
      if (worker === created) {
        replace('startTimeout');
      }
    }, startTimeoutMs);
    options.module().then(
      (module) => {
        if (worker === created && !send({ p: WORKER_PROTOCOL, kind: 'start', module })) {
          fail();
        }
      },
      () => {
        // The page's own compile failed; so does the main thread's core, which reports it.
        if (worker === created) {
          fail();
        }
      },
    );
  }

  function pump(): void {
    if (state !== 'ready' || inFlight !== null || worker === null) {
      return;
    }
    const probe = probes.shift();
    if (probe !== undefined) {
      const id = nextId++;
      inFlight = { kind: 'probe', id, ...probe };
      if (!send({ p: WORKER_PROTOCOL, kind: 'probe', id })) {
        replace('protocol');
      }
      return;
    }
    const next = waiting;
    if (next === null) {
      return;
    }
    waiting = null;
    const id = nextId++;
    inFlight = { kind: 'update', id, resolve: next.resolve, reject: next.reject };
    watchdog = setTimeout(() => {
      watchdog = null;
      log('The preview worker did not answer an update in time.');
      replace('watchdog');
    }, updateTimeoutMs);
    if (
      !send({
        p: WORKER_PROTOCOL,
        kind: 'update',
        id,
        documentJson: next.documentJson,
        optionsJson: next.optionsJson,
      })
    ) {
      replace('protocol');
    }
  }

  function settled(): InFlight | null {
    const done = inFlight;
    inFlight = null;
    if (watchdog !== null) {
      clearTimeout(watchdog);
      watchdog = null;
    }
    return done;
  }

  function receive(data: unknown): void {
    const reply = parseReply(data);
    if (reply === null) {
      replace('protocol');
      return;
    }
    switch (reply.kind) {
      case 'started': {
        if (state !== 'starting') {
          replace('protocol');
          return;
        }
        let started: CoreVersion;
        try {
          started = parseVersion(reply.versionJson);
        } catch {
          replace('protocol');
          return;
        }
        let accepted: boolean;
        try {
          accepted = options.acceptVersion?.(started) ?? true;
        } catch {
          // Nothing may escape the message handler: a check that cannot answer refuses.
          accepted = false;
        }
        if (!accepted) {
          log('The preview worker runs another version of the compiler core.');
          replace('protocol');
          return;
        }
        if (startTimer !== null) {
          clearTimeout(startTimer);
          startTimer = null;
        }
        state = 'ready';
        everStarted = true;
        version = started;
        for (const waiter of readyWaiters.splice(0)) {
          waiter.resolve(started);
        }
        pump();
        return;
      }
      case 'startFailed':
        replace('startFailed');
        return;
      case 'updated': {
        if (state !== 'ready' || inFlight?.kind !== 'update' || inFlight.id !== reply.id) {
          replace('protocol');
          return;
        }
        const done = settled() as Extract<InFlight, { kind: 'update' }>;
        answered = true;
        failedStarts = 0;
        try {
          done.resolve({
            result: parseUpdate(reply.updateJson),
            scopeJson: reply.scopeJson,
            startedAt: reply.startedAt - timeOrigin,
            updateMs: reply.updateMs,
            indexMs: reply.indexMs,
          });
        } catch (error: unknown) {
          // An error envelope (invalid options): a `CoreError`, as on the main thread.
          done.reject(error);
        }
        pump();
        return;
      }
      case 'trapped': {
        if (state !== 'ready' || inFlight?.kind !== 'update' || inFlight.id !== reply.id) {
          replace('protocol');
          return;
        }
        // The worker has replaced its own instance; it is still ready.
        settled()?.reject(
          new CoreTrap('The compiler core stopped in the preview worker; it was restarted there.'),
        );
        pump();
        return;
      }
      case 'probed': {
        if (state !== 'ready' || inFlight?.kind !== 'probe' || inFlight.id !== reply.id) {
          replace('protocol');
          return;
        }
        (settled() as Extract<InFlight, { kind: 'probe' }>).resolve(reply.probe);
        pump();
        return;
      }
    }
  }

  spawn();

  return {
    get state() {
      return state;
    },
    get everStarted() {
      return everStarted;
    },
    ready() {
      if (state === 'failed' || disposed) {
        return Promise.reject(failedError());
      }
      if (state === 'ready' && version !== null) {
        return Promise.resolve(version);
      }
      return new Promise((resolve, reject) => {
        readyWaiters.push({ resolve, reject });
      });
    },
    update(documentJson, previewOptions) {
      if (state === 'failed' || disposed) {
        return Promise.reject(failedError());
      }
      return new Promise((resolve, reject) => {
        // A newer request replaces the waiting one, which resolves as superseded.
        waiting?.resolve(null);
        waiting = {
          documentJson: boundText(documentJson),
          optionsJson: previewOptionsJson(previewOptions),
          resolve,
          reject,
        };
        pump();
      });
    },
    kill() {
      replace('error');
    },
    probe() {
      if (state === 'failed' || disposed) {
        return Promise.reject(failedError());
      }
      return new Promise((resolve, reject) => {
        probes.push({ resolve, reject });
        pump();
      });
    },
    dispose() {
      if (disposed) {
        return;
      }
      disposed = true;
      stopWorker();
      const done = inFlight;
      inFlight = null;
      if (done?.kind === 'update') {
        done.resolve(null);
      } else {
        done?.reject(failedError());
      }
      waiting?.resolve(null);
      waiting = null;
      for (const probe of probes.splice(0)) {
        probe.reject(failedError());
      }
      for (const waiter of readyWaiters.splice(0)) {
        waiter.reject(failedError());
      }
    },
  };
}
