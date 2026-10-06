// Vitest configuration for the frontend (docs/spec/09-quality-and-delivery.md §9.2).
// Run: pnpm --filter @blocks2cpp/desktop test (or test:coverage for the coverage gate).
import { defineConfig, mergeConfig } from 'vitest/config';

import viteConfig from './vite.config.ts';

export default mergeConfig(
  viteConfig,
  defineConfig({
    test: {
      // A fast DOM without layout, so tests run headless in Node.js. Anything that needs real
      // layout or rendering (colour contrast, drag and drop, canvas pixels) is tested end to end.
      environment: 'happy-dom',
      include: ['src/**/*.test.{ts,tsx}'],
      setupFiles: ['./src/test/setup.ts'],
      // Style sheets stay empty in tests, except when a test reads one as text (`?raw`): the
      // accessibility checks read the colour tokens and control sizes from them
      // (src/app/layout/styles.test.ts).
      css: { include: [/\.css\?raw$/] },
      // Every test starts from the same state: no recorded calls, no replaced globals.
      clearMocks: true,
      restoreMocks: true,
      unstubGlobals: true,
      unstubEnvs: true,
      coverage: {
        provider: 'v8',
        // Listing the files makes untested ones count as uncovered instead of disappearing.
        include: ['src/**/*.{ts,tsx}'],
        exclude: ['src/**/*.test.{ts,tsx}', 'src/test/**', 'src/**/*.d.ts', 'src/**/generated/**'],
        reporter: ['text', 'lcov', 'json-summary'],
        reportsDirectory: 'coverage',
        // The frontend coverage gate (§9.2).
        thresholds: { lines: 75 },
      },
    },
  }),
);
