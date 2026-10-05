/**
 * The live preview's asynchronous interface (06 §6.13, M2 decision "Web Worker and incremental
 * analysis"). In milestone M2 it runs on the main thread; the interface is a promise so that a Web
 * Worker can take over (for modules above 2,000 blocks) without changing its callers.
 */
import type { PreviewOptions, PreviewResult } from '@blocks2cpp/b2c-core-wasm';

import type { CoreHost } from './coreHost';

/** Runs the compiler core's preview. */
export interface PreviewService {
  /**
   * The preview of a document given as JSON text. Rejects with `CoreTrap` when the instance
   * stopped, and with `CoreError` when it could not run.
   */
  preview(documentJson: string, options: PreviewOptions): Promise<PreviewResult>;
}

/** The main-thread preview service: the host's core, called after the current task. */
export function mainThreadPreviewService(host: CoreHost): PreviewService {
  return {
    async preview(documentJson, options) {
      const core = host.current() ?? (await host.start());
      return core.preview(documentJson, options);
    },
  };
}
