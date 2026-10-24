/**
 * The live preview's asynchronous interface (06 §6.13, M2 decision "Web Worker and incremental
 * analysis"). In milestone M2 it runs on the main thread; the interface is a promise so that a Web
 * Worker can take over (ADR-0016, M3) without changing its callers.
 */
import type { PreviewOptions, UpdateResult } from '@blocks2cpp/b2c-core-wasm';

import type { CoreHost } from './coreHost';

/** Runs the compiler core's `update()`. */
export interface PreviewService {
  /**
   * The preview of a document given as JSON text, with the hash of its canonical text, from one
   * load of the document. Rejects with `CoreTrap` when the instance stopped, and with `CoreError`
   * when it could not run.
   */
  update(documentJson: string, options: PreviewOptions): Promise<UpdateResult>;
}

/** The main-thread preview service: the host's core, called after the current task. */
export function mainThreadPreviewService(host: CoreHost): PreviewService {
  return {
    async update(documentJson, options) {
      const core = host.current() ?? (await host.start());
      return core.update(documentJson, options);
    },
  };
}
