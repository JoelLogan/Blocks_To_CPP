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
  // Only the Vite dev server opened in a browser has no backend; the desktop app always has one.
  withoutBackend: import.meta.env.DEV && !isTauri(),
});

// The end-to-end test hook (src/e2e/), only in the frontend built for the E2E tests
// (`vite build --mode e2e`). Every other build replaces the condition with `false` and leaves the
// module out of the bundle.
if (import.meta.env.MODE === 'e2e') {
  void import('./e2e').then(({ installE2eHook }) => {
    installE2eHook({
      target: window,
      phase: runtime.getPhase,
      store: useAppStore,
      editor: getEditorHandle,
      core: getCore,
      console: consoleBridge,
      trustedTypes: () => trustedTypes.report(),
    });
  });
}

createRoot(container).render(
  <StrictMode>
    <Root runtime={runtime} />
  </StrictMode>,
);

void runtime.start();
