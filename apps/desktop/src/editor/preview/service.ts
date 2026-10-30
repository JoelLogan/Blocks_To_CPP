/**
 * The live preview's asynchronous interface (06 §6.13, ADR-0016). In milestone M2 it runs on the
 * main thread; the preview worker (`workerService.ts`) takes over behind the same interface, so
 * the pipeline does not need to know where `update()` ran.
 */
import type { PreviewOptions, ScopeIndex, UpdateResult } from '@blocks2cpp/b2c-core-wasm';

import type { CoreHost } from './coreHost';

/** Where an update runs. */
export type PreviewThread = 'worker' | 'main';

/** When `update()` started (the page's clock) and how long it and `scope_index()` took. */
export interface PreviewTiming {
  readonly startedAt: number;
  readonly updateMs: number;
  readonly indexMs: number;
}

/** One answer of the preview service. */
export interface PreviewAnswer {
  /** The preview of the document, with the hash of its canonical text. */
  readonly result: UpdateResult;
  /**
   * The scope index of the instance that made the preview, when that is not the main instance
   * (the worker's; docs/plans/m3-contracts/worker.md §4), or `null`: the main instance made it and
   * answers the scope query itself, or the document did not load.
   */
  readonly scope: ScopeIndex | null;
  readonly timing: PreviewTiming;
}

/** Runs the compiler core's `update()`. */
export interface PreviewService {
  /** Where the next update runs. */
  readonly thread: () => PreviewThread;
  /**
   * The preview of a document given as JSON text, from one load of the document. Resolves `null`
   * when a newer request replaced this one before it ran. Rejects with `CoreTrap` when the
   * instance stopped, and with `CoreError` when it could not run.
   */
  update(documentJson: string, options: PreviewOptions): Promise<PreviewAnswer | null>;
  /**
   * After `update` rejected with `CoreTrap`: resolves when a fresh instance can take the retry.
   * On the main thread it restarts the host's core; for the worker it waits until the worker that
   * replaced its own instance, or the one the client started, is ready, and never restarts one.
   *
   * @throws CoreError (`init`) when no fresh instance can be had.
   */
  recover(): Promise<void>;
}

/** The main-thread preview service: the host's core, called after the current task. */
export function mainThreadPreviewService(
  host: CoreHost,
  now: () => number = () => performance.now(),
): PreviewService {
  return {
    thread: () => 'main',
    async update(documentJson, options) {
      const core = host.current() ?? (await host.start());
      const startedAt = now();
      const result = core.update(documentJson, options);
      return {
        result,
        scope: null,
        timing: { startedAt, updateMs: now() - startedAt, indexMs: 0 },
      };
    },
    async recover() {
      await host.restart();
    },
  };
}
