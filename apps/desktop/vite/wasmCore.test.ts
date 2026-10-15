// @vitest-environment node
// The stale-core guard (vite/wasmCore.ts) in real Vite builds and a real dev server, over a copy
// of the WebAssembly core's inputs whose pkg/build-info.json the test writes.

import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  BUILD_INFO_FILE,
  buildInfoText,
  computeFingerprint,
  INPUT_GROUPS,
  PKG_DIR,
} from '@blocks2cpp/b2c-core-wasm/build-info';
import { build, createLogger, createServer, type InlineConfig } from 'vite';
import { afterEach, describe, expect, it } from 'vitest';

import { wasmCore, wasmCoreProblem } from './wasmCore';

const repoRoot = fileURLToPath(new URL('../../../', import.meta.url));

const scratch: string[] = [];

afterEach(() => {
  for (const dir of scratch.splice(0)) {
    rmSync(dir, { recursive: true, force: true });
  }
});

/**
 * A folder with a copy of the core's inputs (`repo/`), a pkg/ built from them with its
 * build-info.json, and a one-module app (`app/`) to build.
 */
function fixture(): { repo: string; app: string } {
  const root = mkdtempSync(path.join(tmpdir(), 'b2c-wasm-core-plugin-'));
  scratch.push(root);
  const repo = path.join(root, 'repo');
  for (const relative of ['Cargo.lock', ...INPUT_GROUPS.flatMap((group) => group.paths)]) {
    cpSync(path.join(repoRoot, relative), path.join(repo, relative), { recursive: true });
  }
  const pkg = path.join(repo, PKG_DIR);
  mkdirSync(pkg, { recursive: true });
  writeFileSync(path.join(pkg, 'glue.js'), 'export function createGlue() {}\n');
  writeFileSync(
    path.join(pkg, BUILD_INFO_FILE),
    buildInfoText(computeFingerprint(repo), { wasmBindgen: '0.2.129', wasmOpt: null }),
  );

  const app = path.join(root, 'app');
  mkdirSync(app);
  writeFileSync(
    path.join(app, 'index.html'),
    '<!doctype html><script type="module" src="./main.js"></script>\n',
  );
  writeFileSync(path.join(app, 'main.js'), 'document.title = "built";\n');
  return { repo, app };
}

function config(app: string, repo: string): InlineConfig {
  return {
    configFile: false,
    root: app,
    logLevel: 'silent',
    plugins: [wasmCore({ repoRoot: repo })],
    build: { write: false },
  };
}

/** Changes a file the core is built from. */
function editCore(repo: string): void {
  const file = path.join(repo, 'crates/b2c-lang/src/lib.rs');
  writeFileSync(file, `${readFileSync(file, 'utf8')}\n// edited after the core was built\n`);
}

describe('a production build', () => {
  it('goes ahead when the core matches its source', async () => {
    const { repo, app } = fixture();
    await expect(build(config(app, repo))).resolves.toBeDefined();
  });

  it('stops when the core is older than its source, naming what changed and the command', async () => {
    const { repo, app } = fixture();
    editCore(repo);
    await expect(build(config(app, repo))).rejects.toThrow(
      /out of date: crates\/b2c-lang changed since it was built.*`pnpm wasm:build`/s,
    );
  });

  it('stops when the core has no build-info.json or was not built', async () => {
    const { repo, app } = fixture();
    rmSync(path.join(repo, PKG_DIR, BUILD_INFO_FILE));
    await expect(build(config(app, repo))).rejects.toThrow(/has no build-info\.json/);

    rmSync(path.join(repo, PKG_DIR), { recursive: true });
    await expect(build(config(app, repo))).rejects.toThrow(/has not been built/);
  });

  it('stops when an input of the core is missing', async () => {
    const { repo, app } = fixture();
    rmSync(path.join(repo, 'rust-toolchain.toml'));
    await expect(build(config(app, repo))).rejects.toThrow(
      /cannot be checked: the input rust-toolchain\.toml of the WebAssembly core is missing/,
    );
  });
});

describe('the dev server', () => {
  /** Starts a dev server over the fixture and returns what it warned. */
  async function devServerWarnings(app: string, repo: string): Promise<string> {
    const warnings: string[] = [];
    const logger = createLogger('silent');
    logger.warn = (message) => {
      warnings.push(message);
    };
    const server = await createServer({
      ...config(app, repo),
      customLogger: logger,
      server: { middlewareMode: true, ws: false },
    });
    await server.close();
    return warnings.join('\n');
  }

  it('warns about an out-of-date core and starts anyway', async () => {
    const { repo, app } = fixture();
    editCore(repo);
    const warnings = await devServerWarnings(app, repo);
    expect(warnings).toMatch(/out of date: crates\/b2c-lang changed/);
    expect(warnings).toContain('a production build stops');
  });

  it('says nothing when the core matches its source', async () => {
    const { repo, app } = fixture();
    expect(wasmCoreProblem(repo)).toBeNull();
    expect(await devServerWarnings(app, repo)).toBe('');
  });
});
