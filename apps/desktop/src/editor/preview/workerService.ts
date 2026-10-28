/**
 * The preview in a Web Worker (ADR-0016, docs/plans/m3-contracts/worker.md §6): the preview service
 * that runs `update()` in the preview worker once it has started, and on the main instance until
 * then and for good once the worker has failed.
 *
 * The worker is started after the first update, which runs on the main instance, so neither a
 * project's first preview nor the window's start waits for it. A worker that fails for good (three
 * failed starts, or a webview that cannot post a `Module`) is logged once, with no banner; the
 * preview then works as in M2.
 *
 * The editor uses this service only in builds made with `B2C_PREVIEW_WORKER=1`
 * ({@link previewWorkerEnabled}), until the integration step after W2 makes it the default
 * (docs/plans/m3-contracts/w5-worker.md §4).
 */
import { CoreError, CoreTrap, ScopeIndex, type UpdateResult } from '@blocks2cpp/b2c-core-wasm';
import type { CoreWorkerClient } from '@blocks2cpp/b2c-core-wasm/worker';

import { timed } from '../../lib/timing';
import type { CoreHost } from './coreHost';
import {
  mainThreadPreviewService,
  type PreviewAnswer,
  type PreviewService,
  type PreviewThread,
} from './service';

/** Whether this build previews in the worker: `B2C_PREVIEW_WORKER=1` at build time (vite.config.ts). */
export function previewWorkerEnabled(): boolean {
  return import.meta.env['B2C_PREVIEW_WORKER'] === '1';
}

/**
 * The worker's scope index for its preview, or `null` when the document did not load (the index is
 * then the last loaded document's, which the pipeline does not apply either) or the index cannot
 * be read, which is logged: the menus then ask the main instance, as before the worker.
 */
function scopeOf(
  json: string | null,
  result: UpdateResult,
  log: (message: string) => void,
): ScopeIndex | null {
  if (json === null || result.contentHash === null) {
    return null;
  }
  try {
    return ScopeIndex.parse(json, result);
  } catch (error: unknown) {
    log(
      `The preview worker's scope index could not be read: ${error instanceof Error ? error.message : 'unknown error'}`,
    );
    return null;
  }
}

/** What {@link workerPreviewService} needs. */
export interface WorkerPreviewServiceOptions {
  readonly host: CoreHost;
  /** Creates the worker client (once); the main instance's version is the one to accept. */
  readonly createClient: () => CoreWorkerClient;
  /** Where the service reports that it fell back; `console.warn` by default. */
  readonly log?: (message: string) => void;
}

/** The worker preview service, with the client it started (end-to-end hooks only). */
export interface WorkerPreviewService extends PreviewService {
  /** The client, once the first update has started it. */
  readonly client: () => CoreWorkerClient | null;
  /** Stops the worker for good. */
  dispose(): void;
}

/** Creates the worker preview service. */
export function workerPreviewService(options: WorkerPreviewServiceOptions): WorkerPreviewService {
  const main = mainThreadPreviewService(options.host);
  const log =
    options.log ??
    ((message: string) => {
      console.warn(message);
    });
  let client: CoreWorkerClient | null = null;
  let disposed = false;
  let fellBack = false;
  /** Where the last `CoreTrap` came from, which decides how `recover()` gets a fresh instance. */
  let trappedIn: PreviewThread = 'main';

  const thread = (): PreviewThread =>
    client !== null && client.everStarted && client.state !== 'failed' ? 'worker' : 'main';

  const noteFailed = (): void => {
    if (!fellBack) {
      fellBack = true;
      log('The preview worker failed; the preview runs on the main thread from now on.');
    }
  };

  async function onMain(
    documentJson: string,
    previewOptions: Parameters<PreviewService['update']>[1],
  ) {
    try {
      return await main.update(documentJson, previewOptions);
    } catch (error: unknown) {
      if (error instanceof CoreTrap) {
        trappedIn = 'main';
      }
      throw error;
    }
  }

  return {
    thread,
    client: () => client,
    async update(documentJson, previewOptions): Promise<PreviewAnswer | null> {
      if (thread() === 'main') {
        const answer = await onMain(documentJson, previewOptions);
        // Started after the first update, off the path to the first preview.
        if (client === null && !disposed) {
          client = options.createClient();
        }
        if (client?.state === 'failed') {
          noteFailed();
        }
        return answer;
      }
      const worker = client;
      if (worker === null) {
        return onMain(documentJson, previewOptions);
      }
      try {
        const answer = await worker.update(documentJson, previewOptions);
        return answer === null
          ? null
          : {
              result: answer.result,
              scope: timed('b2c:preview:receive', () =>
                scopeOf(answer.scopeJson, answer.result, log),
              ),
              timing: {
                startedAt: answer.startedAt,
                updateMs: answer.updateMs,
                indexMs: answer.indexMs,
              },
            };
      } catch (error: unknown) {
        if (error instanceof CoreTrap) {
          trappedIn = 'worker';
          throw error;
        }
        if (error instanceof CoreError && error.kind === 'init' && !disposed) {
          // The worker failed for good while this request waited: run it here instead.
          noteFailed();
          return onMain(documentJson, previewOptions);
        }
        throw error;
      }
    },
    async recover() {
      if (trappedIn === 'main') {
        await main.recover();
        return;
      }
      // The worker replaced its own instance, or the client is starting a new worker; a worker
      // that failed for good leaves the retry to the main instance.
      try {
        await client?.ready();
      } catch {
        noteFailed();
      }
    },
    dispose() {
      disposed = true;
      client?.dispose();
    },
  };
}
