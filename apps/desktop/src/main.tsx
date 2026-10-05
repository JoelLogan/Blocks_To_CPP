import { isTauri } from '@tauri-apps/api/core';
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { createAppRuntime } from './app/bootstrap';
import { commands } from './app/commands';
import { dialogs, installBlocklyDialogs } from './app/dialogs';
import { Root } from './app/Root';
import { screens } from './app/screens';
import { useAppStore } from './app/store';
import { installFeatures } from './features';
import { ipc } from './lib/ipc';
import './app/app.css';

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

createRoot(container).render(
  <StrictMode>
    <Root runtime={runtime} />
  </StrictMode>,
);

void runtime.start();
