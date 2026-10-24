/**
 * Runs before every test file (vitest.config.ts `setupFiles`).
 *
 * Testing Library unmounts rendered components after each test by itself only when the test
 * framework's globals are enabled; this project imports `describe`, `it` and `expect` explicitly,
 * so the cleanup is registered here.
 */
import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

import { installEventTargetReceiverShim } from './dom-shims';

// The build plugins' tests (vite/*.test.ts) run in Node, which has no document to correct.
if (typeof document !== 'undefined') {
  installEventTargetReceiverShim();
}

afterEach(() => {
  cleanup();
});
