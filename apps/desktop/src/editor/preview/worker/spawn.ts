/**
 * Creating the preview worker (ADR-0016 §2): the packaged module script Vite builds under
 * `workers/`, from 'self', never a `blob:` or `data:` URL, through the `b2c-core-worker` Trusted
 * Types policy when the webview has Trusted Types; and the editor's worker preview service over it.
 */
import { coreModule, type CoreVersion } from '@blocks2cpp/b2c-core-wasm';
import { createCoreWorkerClient, type WorkerLike } from '@blocks2cpp/b2c-core-wasm/worker';

import { coreWorkerScriptUrl } from '../../../lib/trustedTypes';
import type { CoreHost } from '../coreHost';
import { type WorkerPreviewService, workerPreviewService } from '../workerService';
import workerUrl from './core.worker.ts?worker&url';

/** The worker's name, as the webview's developer tools show it. */
export const PREVIEW_WORKER_NAME = 'b2c-preview';

/** The worker script's absolute URL. */
export function previewWorkerUrl(): string {
  return new URL(workerUrl, document.baseURI).href;
}

/** Starts a new preview worker. */
export function spawnPreviewWorker(): WorkerLike {
  return new Worker(coreWorkerScriptUrl(previewWorkerUrl()), {
    type: 'module',
    name: PREVIEW_WORKER_NAME,
  }) as unknown as WorkerLike;
}

/** Whether two cores are the same version. */
export function sameVersion(a: CoreVersion, b: CoreVersion): boolean {
  return (
    a.app === b.app &&
    a.catalog === b.catalog &&
    a.formatVersion === b.formatVersion &&
    a.sourceMapVersion === b.sourceMapVersion
  );
}

/** The editor's worker preview service, while an editor uses it (for the end-to-end hooks). */
let current: WorkerPreviewService | null = null;

/** The worker preview service of the open editor, or `null` (also when the build has it off). */
export function currentWorkerPreviewService(): WorkerPreviewService | null {
  return current;
}

/**
 * The editor's worker preview service: the app's worker, posted the page's compiled module, whose
 * core must be the main instance's version.
 */
export function appWorkerPreviewService(host: CoreHost): WorkerPreviewService {
  const service = workerPreviewService({
    host,
    createClient: () =>
      createCoreWorkerClient({
        spawn: spawnPreviewWorker,
        module: coreModule,
        acceptVersion: (version) => {
          const main = host.current();
          return main !== null && sameVersion(main.version(), version);
        },
      }),
  });
  current = service;
  return {
    ...service,
    dispose() {
      if (current === service) {
        current = null;
      }
      service.dispose();
    },
  };
}
