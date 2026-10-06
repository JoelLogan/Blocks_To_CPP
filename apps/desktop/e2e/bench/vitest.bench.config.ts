// The webview benchmarks (docs/spec/09-quality-and-delivery.md §9.2 "Benchmarks"): cold start, the
// preview at 1,000 blocks and dragging in a 5,000-block workspace, in the real app through the E2E
// harness. The nightly `bench` job runs them and compares the results with the baseline
// (tools/bench-compare.py). See README.md here.
//
// Two projects:
// - `bench-unit`: unit tests of the benchmarks' own logic (no app), fast;
// - `bench`: the benchmarks, one app at a time, on an otherwise quiet machine.
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    root: import.meta.dirname,
    teardownTimeout: 60_000,
    projects: [
      {
        test: {
          name: 'bench-unit',
          root: import.meta.dirname,
          include: ['**/*.test.ts'],
          environment: 'node',
        },
      },
      {
        test: {
          name: 'bench',
          root: import.meta.dirname,
          include: ['**/*.bench.e2e.ts'],
          environment: 'node',
          pool: 'forks',
          maxWorkers: 1,
          fileParallelism: false,
          // A benchmark owns the machine while it runs; nothing is retried behind its back.
          retry: 0,
          // Eleven launches, or opening a 5,000-block project and measuring in it, take minutes.
          testTimeout: 900_000,
          hookTimeout: 120_000,
        },
      },
    ],
  },
});
