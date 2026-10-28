import { coreModule, initCore } from '@blocks2cpp/b2c-core-wasm';
import type { WorkerLike } from '@blocks2cpp/b2c-core-wasm/worker';
import { isTauri } from '@tauri-apps/api/core';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { createAppRuntime } from './app/bootstrap';
import { commands } from './app/commands';
import { getCore } from './app/core';
import { dialogs, installBlocklyDialogs } from './app/dialogs';
import { getEditorHandle } from './app/editor-types';
import { Root } from './app/Root';
import { screens } from './app/screens';
import { useAppStore } from './app/store';
import { previewWorkerEnabled } from './editor/preview/workerService';
import { installFeatures } from './features';
import { consoleBridge } from './features/build-run';
import { ipc } from './lib/ipc';
import { startTrustedTypesCollector } from './lib/trustedTypes';
import './app/app.css';

// First, so it sees every Content Security Policy violation from the start (the Trusted Types
// trial, docs/spec/08-security.md §8.8).
const trustedTypes = startTrustedTypesCollector(document);

const container = document.getElementById('root');
if (container === null) {
  throw new Error('index.html has no element with the id "root"');
}

// The runtime starts outside React, so it runs exactly once (StrictMode renders twice in
// development) and the backend subscription is made once.
const runtime = createAppRuntime({
  ipc,
  store: useAppStore,
  commands,
  screens,
  dialogs,
  installFeatures,
  installBlocklyDialogs,
  window,
  // The dev server serves the WebAssembly core as it is (vite/wasmCore.ts only warns), so a
  // development build checks at start-up that the core has the backend's block catalog
  // (docs/spec/02-architecture.md §2.5.7); the editor then reuses the started core. A production
  // build cannot embed an out-of-date core, so it keeps the core off the start-up path.
  ...(import.meta.env.DEV
    ? { coreCatalogVersion: async () => (await initCore()).version().catalog }
    : {}),
  // Only the Vite dev server opened in a browser has no backend; the desktop app always has one.
  withoutBackend: import.meta.env.DEV && !isTauri(),
});

// The end-to-end test hook (src/e2e/), only in the frontend built for the E2E tests
// (`vite build --mode e2e`). Every other build replaces the condition with `false` and leaves the
// module out of the bundle.
if (import.meta.env.MODE === 'e2e') {
  void Promise.all([
    import('./e2e'),
    import('@blocks2cpp/b2c-core-wasm/worker'),
    import('./editor/preview/worker/spawn'),
    import('./editor/preview/worker/probe'),
  ]).then(([{ installE2eHook }, { createCoreWorkerClient }, spawn, { probeRequest }]) => {
    installE2eHook({
      target: window,
      phase: runtime.getPhase,
      store: useAppStore,
      editor: getEditorHandle,
      core: getCore,
      console: consoleBridge,
      trustedTypes: () => trustedTypes.report(),
      trustedTypesCount: () => trustedTypes.trustedTypesCount(),
      workerClient: (acceptVersion) =>
        createCoreWorkerClient({
          spawn: spawn.spawnPreviewWorker,
          module: coreModule,
          acceptVersion,
        }),
      spawnUnwrappedWorker: () =>
        new Worker(spawn.previewWorkerUrl(), {
          type: 'module',
          name: spawn.PREVIEW_WORKER_NAME,
        }) as unknown as WorkerLike,
      pageRequest: () => probeRequest(() => trustedTypes.report().directives),
      previewWorkerEnabled,
      previewService: spawn.currentWorkerPreviewService,
    });
  });
}

createRoot(container).render(
  <StrictMode>
    <Root runtime={runtime} />
  </StrictMode>,
);

void runtime.start();
