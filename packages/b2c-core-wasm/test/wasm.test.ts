// The real compiler core, built by `pnpm --filter @blocks2cpp/b2c-core-wasm build`, under Node:
// the examples, the golden C++ and the malicious-project suite through the TypeScript API.
// Without a build these tests are skipped, unless B2C_REQUIRE_WASM is set (as in CI), in which
// case a missing build fails them.

import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { beforeAll, describe, expect, it } from 'vitest';

import type { CoreWasm, Diagnostic } from '../src/index';
import {
  CoreError,
  CURRENT_FORMAT_VERSION,
  initCore,
  initCoreFromBytes,
  MAX_DOCUMENT_BYTES,
  resetCore,
} from '../src/index';

const repo = (path: string) => fileURLToPath(new URL(`../../../${path}`, import.meta.url));
const builtModule = fileURLToPath(new URL('../pkg/b2c_core_wasm_bg.wasm', import.meta.url));
const built = existsSync(builtModule);
const required = (process.env['B2C_REQUIRE_WASM'] ?? '') !== '';

if (!built && !required) {
  console.warn(
    'test/wasm.test.ts skipped: build the module first (pnpm --filter @blocks2cpp/b2c-core-wasm build)',
  );
}

const examplesDir = repo('examples');
const exampleNames = readdirSync(examplesDir)
  .filter((name) => name.endsWith('.b2c'))
  .map((name) => name.slice(0, -'.b2c'.length))
  .sort();

const exampleText = (name: string) => readFileSync(`${examplesDir}/${name}.b2c`, 'utf8');
const codes = (diagnostics: Diagnostic[]) => diagnostics.map((d) => d.code);

interface SecurityCase {
  file: string;
  /** `null` when the loader accepts the file. */
  loader: string[] | null;
}

/** The Loader column of tests/security/projects/README.md (as the Rust suite reads it). */
function securityCases(): SecurityCase[] {
  const readme = readFileSync(repo('tests/security/projects/README.md'), 'utf8');
  const cases = readme
    .split('\n')
    .filter((line) => line.startsWith('| `'))
    .map((line) => {
      const cells = line.split('|').map((cell) => cell.trim());
      expect(cells, line).toHaveLength(8);
      const loaderCell = cells[4] ?? '';
      return {
        file: (cells[1] ?? '').replaceAll('`', ''),
        loader:
          loaderCell === 'accepted'
            ? null
            : loaderCell.split('`').filter((piece) => piece.startsWith('B2C-')),
      };
    });
  expect(cases.length).toBeGreaterThanOrEqual(60);
  return cases;
}

describe.skipIf(!built && !required)('the built compiler core', () => {
  let core: CoreWasm;

  beforeAll(async () => {
    core = await initCore();
  });

  it('reports its versions', () => {
    const version = core.version();
    expect(version).toEqual({
      app: expect.stringMatching(/^\d+\.\d+\.\d+/) as unknown,
      catalog: '1.0.0',
      formatVersion: CURRENT_FORMAT_VERSION,
      sourceMapVersion: 1,
    });
  });

  it('has all 15 examples', () => {
    expect(exampleNames.length).toBeGreaterThanOrEqual(15);
  });

  it.each(exampleNames)('loads %s and saves it back byte for byte', (name) => {
    const text = exampleText(name);
    const loaded = core.load(new TextEncoder().encode(text));
    if (!loaded.ok) {
      throw new Error(`${name} did not load: ${codes(loaded.diagnostics).join(', ')}`);
    }
    expect(loaded.diagnostics).toEqual([]);
    expect(loaded.document.format).toBe('blocks2cpp/project');
    // The editor's round trip: keep the loaded object, send it back as JSON.
    const saved = core.canonical(JSON.stringify(loaded.document));
    if (!saved.ok) {
      throw new Error(`${name} did not save: ${codes(saved.diagnostics).join(', ')}`);
    }
    expect(saved.text === text).toBe(true);
    expect(saved.hash).toMatch(/^[0-9a-f]{64}$/);
  });

  it.each(exampleNames)('previews %s exactly as the build generates it', (name) => {
    const golden = readFileSync(repo(`tests/golden/${name}/main.cpp`), 'utf8');
    const preview = core.preview(exampleText(name), { indentWidth: 4 });
    expect(preview.stage).toBe('generate');
    expect(preview.diagnostics.filter((d) => d.severity === 'error')).toEqual([]);
    expect(preview.buildable).toBe(true);
    expect(preview.placeholders).toBe(0);
    const main = preview.files.find((file) => file.path === 'main.cpp');
    expect(main?.kind).toBe('source');
    expect(main?.contents === golden).toBe(true);
    expect(preview.sourceMap?.version).toBe(1);
    expect(preview.sourceMap?.files.map((file) => file.path)).toEqual(
      preview.files.map((file) => file.path),
    );
    const saved = core.canonical(exampleText(name));
    expect(saved.ok && preview.contentHash === saved.hash).toBe(true);
    // Scope data of the program (the exact values are checked natively and in
    // scope-clipboard.test.ts): static types, and symbols sorted by name.
    const types = ['void', 'bool', 'char', 'int', 'double', 'string', 'error'];
    for (const type of Object.values(preview.blockTypes)) {
      expect(types).toContain(type);
    }
    const symbolNames = preview.symbols.map((symbol) => symbol.name);
    expect(symbolNames).toEqual([...symbolNames].sort());
    for (const symbol of preview.symbols) {
      expect(types).toContain(symbol.type);
    }
  });

  it('indents with two spaces when asked', () => {
    const preview = core.preview(exampleText('hello_world'), { indentWidth: 2 });
    expect(preview.files[0]?.contents).toContain('\n  std::cout');
  });

  it('previews best-effort C++ for a dangling reference', () => {
    const document = JSON.parse(exampleText('hello_world')) as {
      modules: { workspace: { blocks: { statements: Record<string, { inputs: unknown }[]> }[] } }[];
    };
    const print = document.modules[0]?.workspace.blocks[0]?.statements['BODY']?.[0];
    if (print === undefined) {
      throw new Error('hello_world has changed shape');
    }
    print.inputs = { ITEM0: { expr: [{ ref: 'sym_gone' }] } };
    const preview = core.preview(JSON.stringify(document), { indentWidth: 4 });
    expect(preview.stage).toBe('analyze');
    expect(codes(preview.diagnostics)).toContain('B2C-E0201');
    expect(preview.buildable).toBe(false);
    expect(preview.files[0]?.contents).toContain('/* error */');
  });

  it.each(securityCases())('gives $file its expected loader outcome', ({ file, loader }) => {
    const bytes = new Uint8Array(readFileSync(repo(`tests/security/projects/${file}`)));
    const loaded = core.load(bytes);
    if (loader === null) {
      if (!loaded.ok) {
        throw new Error(`${file} should load: ${codes(loaded.diagnostics).join(', ')}`);
      }
      const saved = core.canonical(JSON.stringify(loaded.document));
      expect(saved.ok).toBe(true);
      const preview = core.preview(JSON.stringify(loaded.document), { indentWidth: 4 });
      expect(preview.stage).not.toBe('load');
      expect(preview.files.length).toBeGreaterThan(0);
    } else {
      expect(loaded.ok).toBe(false);
      expect([...new Set(codes(loaded.diagnostics))].sort()).toEqual([...loader].sort());
      for (const diagnostic of loaded.diagnostics) {
        expect(diagnostic.severity).toBe('error');
        expect(diagnostic.source).toBe('loader');
      }
    }
  });

  it('refuses an oversized document without copying all of it', () => {
    const text = `{"pad": "${'x'.repeat(MAX_DOCUMENT_BYTES + 10)}"}`;
    for (const result of [
      core.canonical(text),
      core.load(new TextEncoder().encode(text)),
      core.preview(text, { indentWidth: 4 }),
    ]) {
      expect(codes(result.diagnostics)).toEqual(['B2C-E0101']);
    }
  });

  it('refuses invalid preview options with CoreError', () => {
    expect(() => core.preview('{}', { indentWidth: 3 } as unknown as { indentWidth: 4 })).toThrow(
      CoreError,
    );
    // The instance keeps working.
    expect(core.version().catalog).toBe('1.0.0');
  });

  it('returns failures, never throws, for garbage', () => {
    for (const text of ['', '{', '[]', 'null', '{"format": 1}', '['.repeat(10_000)]) {
      const loaded = core.load(new TextEncoder().encode(text));
      expect(loaded.ok).toBe(false);
      expect(loaded.diagnostics.length).toBeGreaterThan(0);
      expect(core.canonical(text).ok).toBe(false);
      const preview = core.preview(text, { indentWidth: 2 });
      expect(preview.stage).toBe('load');
      expect(preview.files).toEqual([]);
      expect(preview.sourceMap).toBeNull();
    }
  });

  it('starts identical independent instances from the module bytes', async () => {
    const other = await initCoreFromBytes(new Uint8Array(readFileSync(builtModule)));
    expect(other).not.toBe(core);
    const text = exampleText('guessing_game');
    expect(other.preview(text, { indentWidth: 4 })).toEqual(core.preview(text, { indentWidth: 4 }));
  });

  it('starts a fresh working instance after resetCore', async () => {
    resetCore();
    const fresh = await initCore();
    expect(fresh).not.toBe(core);
    expect(fresh.canonical(exampleText('primes'))).toEqual(core.canonical(exampleText('primes')));
  });
});
