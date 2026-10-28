/**
 * The preview worker's entry (ADR-0016, docs/plans/m3-contracts/worker.md §2): a dedicated module
 * worker, built by Vite under `workers/` and served with its own policy (`WORKER_CSP`,
 * src-tauri/src/window.rs). It instantiates the `WebAssembly.Module` the page posts and answers
 * the page's `update` requests; it never loads the embedded bytes, has no DOM, no IPC and no
 * `@tauri-apps` code.
 */
// First: everything after it runs with `Object.prototype` frozen.
import './freeze';

import { serveCoreWorker, type WorkerScopeLike } from '@blocks2cpp/b2c-core-wasm/worker';

import { type ProbeScope, workerProbe } from './probe';

const scope = self as unknown as WorkerScopeLike & ProbeScope;

serveCoreWorker(
  scope,
  // The probe exists only in the frontend built for the E2E tests (ADR-0009's seam).
  import.meta.env.MODE === 'e2e' ? { probe: workerProbe(scope) } : {},
);
