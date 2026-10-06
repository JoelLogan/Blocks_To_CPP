// The visual diff of the block canvas (docs/spec/09-quality-and-delivery.md §9.2 "Visual diff"),
// in the real app through the E2E harness; desktop.yml's e2e job runs it after the other
// end-to-end tests. See README.md here.
//
// Two projects:
// - `visual-unit`: unit tests of the comparison and the fixture (no app), fast;
// - `visual`: the screenshots, one app at a time.
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    root: import.meta.dirname,
    teardownTimeout: 60_000,
    projects: [
      {
        test: {
          name: 'visual-unit',
          root: import.meta.dirname,
          include: ['**/*.test.ts'],
          environment: 'node',
        },
      },
      {
        test: {
          name: 'visual',
          root: import.meta.dirname,
          include: ['**/*.visual.e2e.ts'],
          globalSetup: ['global.ts'],
          environment: 'node',
          pool: 'forks',
          maxWorkers: 1,
          fileParallelism: false,
          // A screenshot that differs is a finding, not a flake: nothing is retried.
          retry: 0,
          testTimeout: 300_000,
          hookTimeout: 120_000,
        },
      },
    ],
  },
});
