// Vitest configuration (docs/spec/09-quality-and-delivery.md §9.2).
// Run: pnpm --filter @blocks2cpp/catalog-gen test (or test:coverage for the coverage gate).
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    // The generator runs in Node.js at build time; it has no DOM.
    environment: 'node',
    include: ['test/**/*.test.ts'],
    restoreMocks: true,
    unstubEnvs: true,
    coverage: {
      provider: 'v8',
      // Listing the files makes untested ones count as uncovered instead of disappearing.
      include: ['src/**/*.ts'],
      // The command-line entry point only parses its arguments and calls src/files.ts.
      exclude: ['src/cli.ts'],
      reporter: ['text', 'lcov', 'json-summary'],
      reportsDirectory: 'coverage',
      // The frontend coverage gate (§9.2).
      thresholds: { lines: 75 },
    },
  },
});
