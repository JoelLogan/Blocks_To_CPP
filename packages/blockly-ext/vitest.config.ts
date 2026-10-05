// Vitest configuration (docs/spec/09-quality-and-delivery.md §9.2).
// Run: pnpm --filter @blocks2cpp/blockly-ext test (or test:coverage for the coverage gate).
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    // A fast DOM without layout, so tests run headless in Node.js. Blockly runs in it, both as a
    // headless workspace and injected with the Zelos renderer (see test/setup.ts).
    environment: 'happy-dom',
    include: ['src/**/*.test.ts', 'test/**/*.test.ts'],
    setupFiles: ['./test/setup.ts'],
    // Every test starts from the same state: no recorded calls, no replaced globals.
    clearMocks: true,
    restoreMocks: true,
    unstubGlobals: true,
    unstubEnvs: true,
    coverage: {
      provider: 'v8',
      // Listing the files makes untested ones count as uncovered instead of disappearing.
      include: ['src/**/*.ts'],
      exclude: ['src/**/*.test.ts', 'src/**/*.d.ts', 'src/**/generated/**'],
      reporter: ['text', 'lcov', 'json-summary'],
      reportsDirectory: 'coverage',
      // The frontend coverage gate (§9.2).
      thresholds: { lines: 75 },
    },
  },
});
