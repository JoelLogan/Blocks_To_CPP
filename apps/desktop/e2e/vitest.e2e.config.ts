// The end-to-end tests (docs/spec/09-quality-and-delivery.md §9.2, docs/adr/0009): selenium-webdriver
// drives the real app through tauri-driver. Run: pnpm --filter @blocks2cpp/desktop run e2e
// (see e2e/README.md for the app build and the drivers).
//
// Two projects:
// - `support`: unit tests of the harness's own logic (no app, no driver), fast;
// - `e2e`: the specs, one app at a time: a single forked worker, files one after the other, and
//   long timeouts (a build may take up to two minutes).
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    root: import.meta.dirname,
    teardownTimeout: 60_000,
    projects: [
      {
        test: {
          name: 'support',
          root: import.meta.dirname,
          include: ['support/**/*.test.ts'],
          environment: 'node',
        },
      },
      {
        test: {
          name: 'e2e',
          root: import.meta.dirname,
          include: ['specs/**/*.e2e.ts'],
          globalSetup: ['support/global.ts'],
          environment: 'node',
          pool: 'forks',
          maxWorkers: 1,
          fileParallelism: false,
          // A test owns one app from launch to quit; nothing is retried behind its back.
          retry: 0,
          testTimeout: 300_000,
          hookTimeout: 120_000,
        },
      },
    ],
  },
});
