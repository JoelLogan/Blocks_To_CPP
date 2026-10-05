import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    include: ['src/**/*.test.ts'],
    coverage: {
      provider: 'v8',
      include: ['src/**/*.ts'],
      // types.ts holds type declarations only, so it has no code to cover.
      exclude: ['src/**/*.test.ts', 'src/generated/types.ts'],
      // lcov for CI's coverage artifact, json-summary for its job summary (desktop.yml).
      reporter: ['text', 'lcov', 'json-summary'],
      reportsDirectory: 'coverage',
      thresholds: { lines: 75 },
    },
  },
});
