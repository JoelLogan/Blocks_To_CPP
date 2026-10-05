// Vitest under Node (docs/spec/09-quality-and-delivery.md §9.2). The tests in test/wasm.test.ts
// need the built module (pnpm --filter @blocks2cpp/b2c-core-wasm build) and are skipped without
// it unless B2C_REQUIRE_WASM is set; the others run without a build.
import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    environment: 'node',
    include: ['test/**/*.test.ts'],
    // The suite runs every example and malicious project through the real module.
    testTimeout: 60_000,
    coverage: {
      provider: 'v8',
      include: ['src/**/*.ts'],
      exclude: ['src/**/*.d.ts'],
      reporter: ['text', 'lcov'],
      thresholds: { lines: 75 },
    },
  },
});
