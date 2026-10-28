/**
 * The preview worker's side of the protocol (ADR-0016 §3, docs/plans/m3-contracts/worker.md §3):
 * instantiates the `WebAssembly.Module` the page posts, answers each `update` with the core's text
 * unparsed, and replaces its own instance after a trap. It imports only the instance's glue, never
 * the embedded bytes (`#pkg-bytes`).
 */

import type { CoreHandle } from '../core';
import { CoreTrap } from '../errors';
import { instantiate } from '../instance';
import { parseRequest, WORKER_PROTOCOL, type WorkerProbe, type WorkerReply } from './protocol';

/** The members of `DedicatedWorkerGlobalScope` the host uses (one end of a `MessageChannel` in tests). */
export interface WorkerScopeLike {
  postMessage(message: WorkerReply): void;
  onmessage: ((event: { readonly data: unknown }) => void) | null;
}

/** What {@link serveCoreWorker} needs besides the scope. */
export interface ServeOptions {
  /** Answers `probe` requests (end-to-end builds only); without it a probe is a protocol error. */
  readonly probe?: () => Promise<WorkerProbe>;
  /** Instantiates a module (replaceable in tests). */
  readonly instantiate?: (module: WebAssembly.Module) => Promise<CoreHandle>;
  /** The worker's clock: `performance` by default. */
  readonly clock?: { readonly timeOrigin: number; now(): number };
}

/** Why a start failed, without anything from the project (there is none yet). */
function reasonOf(error: unknown): string {
  const text = error instanceof Error ? `${error.name}: ${error.message}` : 'unknown error';
  return text.slice(0, 256);
}

/**
 * Serves the protocol on `scope` until the worker is terminated. Requests are handled one at a
 * time, in the order they arrive (the client never sends a second before the first is answered).
 */
export function serveCoreWorker(scope: WorkerScopeLike, options: ServeOptions = {}): void {
  const start = options.instantiate ?? instantiate;
  const clock = options.clock ?? performance;
  let module: WebAssembly.Module | null = null;
  /** The instance answering updates: starting, started, or being replaced after a trap. */
  let instance: Promise<CoreHandle> | null = null;
  /** Requests are chained so that each waits for the one before it. */
  let queue: Promise<void> = Promise.resolve();

  const post = (reply: WorkerReply): void => {
    scope.postMessage(reply);
  };
  const ignore = (): void => {
    console.error('The preview worker ignored a message that is not of its protocol.');
  };

  async function handle(data: unknown): Promise<void> {
    const request = parseRequest(data);
    if (request === null) {
      ignore();
      return;
    }
    switch (request.kind) {
      case 'start': {
        if (module !== null) {
          ignore();
          return;
        }
        module = request.module;
        instance = start(module);
        try {
          const started = await instance;
          post({ p: WORKER_PROTOCOL, kind: 'started', versionJson: started.raw.version() });
        } catch (error: unknown) {
          post({ p: WORKER_PROTOCOL, kind: 'startFailed', message: reasonOf(error) });
        }
        return;
      }
      case 'update': {
        if (module === null || instance === null) {
          ignore();
          return;
        }
        const current = await instance;
        const began = clock.now();
        let updateJson: string;
        try {
          updateJson = current.raw.update(request.documentJson, request.optionsJson);
        } catch (error: unknown) {
          if (!(error instanceof CoreTrap)) {
            throw error;
          }
          // As `resetCore()` does: the trapped instance is dropped and a fresh one made from the
          // module this worker holds. The client restarts nothing for a trap.
          instance = start(module);
          try {
            await instance;
          } catch (restartError: unknown) {
            post({ p: WORKER_PROTOCOL, kind: 'startFailed', message: reasonOf(restartError) });
            return;
          }
          post({
            p: WORKER_PROTOCOL,
            kind: 'trapped',
            id: request.id,
            message: 'The compiler core stopped in the preview worker.',
          });
          return;
        }
        const updateMs = clock.now() - began;
        post({
          p: WORKER_PROTOCOL,
          kind: 'updated',
          id: request.id,
          updateJson,
          // The scope index comes with the core's `scope_index()` export (worker.md §4).
          scopeJson: null,
          startedAt: clock.timeOrigin + began,
          updateMs: Math.max(0, updateMs),
          indexMs: 0,
        });
        return;
      }
      case 'probe': {
        if (options.probe === undefined) {
          ignore();
          return;
        }
        post({ p: WORKER_PROTOCOL, kind: 'probed', id: request.id, probe: await options.probe() });
        return;
      }
    }
  }

  scope.onmessage = (event) => {
    queue = queue
      .then(() => handle(event.data))
      .catch((error: unknown) => {
        // Only a bug gets here; the message names no project text.
        console.error('The preview worker failed to handle a request.', reasonOf(error));
      });
  };
}
