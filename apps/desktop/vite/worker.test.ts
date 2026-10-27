// @vitest-environment node
// The preview worker's build (ADR-0016 §2, docs/plans/m3-contracts/w5-worker.md §3.2): a real Vite
// build of the module that creates the worker, with the app's own `worker` options. The worker is
// one entry under workers/, and nothing under workers/ holds the embedded WebAssembly bytes, Tauri's
// API or (outside end-to-end builds) the probe. The worker imports the core's generated glue, so
// these tests need the built package (pnpm --filter @blocks2cpp/b2c-core-wasm build); without it
// they are skipped, unless B2C_REQUIRE_WASM is set (as in CI).

import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import { build } from 'vite';
import { afterEach, describe, expect, it } from 'vitest';

import appConfig from '../vite.config';

const appRoot = fileURLToPath(new URL('../', import.meta.url));
const glue = fileURLToPath(new URL('../../../packages/b2c-core-wasm/pkg/glue.js', import.meta.url));
const built = existsSync(glue);
const required = (process.env['B2C_REQUIRE_WASM'] ?? '') !== '';

const scratch: string[] = [];

afterEach(() => {
  for (const dir of scratch.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

/** Builds the module that creates the worker in `mode`; returns the files under workers/. */
async function buildWorker(mode: string): Promise<{ name: string; text: string }[]> {
  const root = mkdtempSync(path.join(tmpdir(), 'b2c-worker-build-'));
  scratch.push(root);
  const outDir = path.join(root, 'dist');
  // An entry that uses the module, so the build keeps it (and with it the worker).
  const entry = path.join(root, 'entry.ts');
  const spawn = path.join(appRoot, 'src/editor/preview/worker/spawn.ts');
  writeFileSync(
    entry,
    `import { spawnPreviewWorker } from ${JSON.stringify(spawn)};\n` +
      'Object.assign(globalThis, { spawnPreviewWorker });\n',
  );
  const config = typeof appConfig === 'function' ? undefined : appConfig;
  await build({
    configFile: false,
    root: appRoot,
    mode,
    logLevel: 'silent',
    ...(config?.worker === undefined ? {} : { worker: config.worker }),
    build: {
      outDir,
      emptyOutDir: true,
      write: true,
      rolldownOptions: { input: entry },
    },
  });
  const folder = path.join(outDir, 'workers');
  return readdirSync(folder).map((name) => ({
    name,
    text: readFileSync(path.join(folder, name), 'utf8'),
  }));
}

describe.skipIf(!built && !required)('the preview worker’s build', () => {
  it('is one module entry under workers/ without the bytes, Tauri’s API or the probe', async () => {
    const files = await buildWorker('production');
    const entries = files.filter(({ name }) => name.startsWith('core.worker-'));
    expect(entries).toHaveLength(1);
    expect(entries[0]?.name).toMatch(/^core\.worker-[\w-]+\.js$/);
    for (const { name, text } of files) {
      expect(name).toMatch(/\.js$/);
      expect(text, name).not.toContain('WASM_BASE64');
      expect(text, name).not.toContain('@tauri-apps');
      expect(text, name).not.toContain('__TAURI');
      expect(text, name).not.toContain('ipc.localhost');
      expect(text, name).not.toMatch(/\beval\(/);
    }
    // The prototype is frozen before anything else of the worker runs.
    expect(entries[0]?.text.startsWith('Object.freeze(Object.prototype)')).toBe(true);
  });

  it('has the probe in end-to-end builds only', async () => {
    const files = await buildWorker('e2e');
    const entry = files.find(({ name }) => name.startsWith('core.worker-'));
    expect(entry?.text).toContain('ipc.localhost');
    for (const { name, text } of files) {
      expect(text, name).not.toContain('WASM_BASE64');
    }
  });
});
