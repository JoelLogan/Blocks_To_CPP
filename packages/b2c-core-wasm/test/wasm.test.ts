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
      // The editor's round trip gives the file's own canonical text and hash (05 §5.6).
      const saved = core.canonical(JSON.stringify(loaded.document));
      const direct = core.canonical(new TextDecoder().decode(bytes));
      expect(saved.ok && direct.ok).toBe(true);
      if (saved.ok && direct.ok) {
        expect(saved.text === direct.text).toBe(true);
        expect(saved.hash).toBe(direct.hash);
      }
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

  describe('numbers through JSON.parse and JSON.stringify (05 §5.6)', () => {
    /** `hello_world` with `defines` and `x-ext` spliced in as raw JSON text. */
    function hello(defines: string, ext: string): string {
      const text = exampleText('hello_world')
        .replace('\n  "formatVersion": 1,\n', `\n  "formatVersion": 1,\n  "x-ext": ${ext},\n`)
        .replace('\n    "build": {\n', `\n    "build": {\n      "defines": ${defines},\n`);
      expect(text).toContain('"x-ext"');
      expect(text).toContain('"defines"');
      return text;
    }

    /** The loader's codes for a text it must refuse. */
    function refused(text: string): string[] {
      const loaded = core.load(new TextEncoder().encode(text));
      expect(loaded.ok).toBe(false);
      return codes(loaded.diagnostics);
    }

    /**
     * What the editor does with a text that must load: keep the loaded object and send it back as
     * JSON. Returns the canonical text and hash of the file and of the editor's copy.
     */
    function throughTheEditor(text: string) {
      const loaded = core.load(new TextEncoder().encode(text));
      if (!loaded.ok) {
        throw new Error(`the document did not load: ${codes(loaded.diagnostics).join(', ')}`);
      }
      const direct = core.canonical(text);
      const edited = core.canonical(JSON.stringify(loaded.document));
      if (!direct.ok || !edited.ok) {
        throw new Error('the document did not save');
      }
      return { direct, edited };
    }

    it('refuses what JavaScript would change', () => {
      for (const [defines, ext] of [
        ['[{"name": "BIG", "value": {"int": 9007199254740993}}]', '{}'],
        ['[{"name": "SMALL", "value": {"int": -9007199254740992}}]', '{}'],
        ['[]', '{"c": 12345678901234567890}'],
        ['[]', '{"c": [1, {"d": 1e300}]}'],
      ] as const) {
        expect(refused(hello(defines, ext))).toEqual(['B2C-E0112']);
      }
    });

    it('keeps defines and free-form numbers, and writes them as JavaScript does', () => {
      const { direct, edited } = throughTheEditor(
        hello(
          '[{"name": "TOP", "value": {"int": 9007199254740991}}, {"name": "LOW", "value": {"int": -9007199254740991}}]',
          '{"a": 1.0, "b": 1.5e-6, "c": 1e-7, "d": -2.5, "e": 2e3, "f": 0.1, "g": -0.0, "h": 5e-324}',
        ),
      );
      expect(edited.text === direct.text).toBe(true);
      expect(edited.hash).toBe(direct.hash);
      // The canonical text is exactly what JSON.stringify writes for it.
      expect(`${JSON.stringify(JSON.parse(direct.text), null, 2)}\n` === direct.text).toBe(true);
      expect(direct.text).toContain('"int": 9007199254740991');
      expect(direct.text).toContain('"b": 0.0000015,');
      expect(direct.text).toContain('"g": 0,');
    });

    it('keeps any number within ±(2^53 − 1) and refuses the others', () => {
      // Reproducible random doubles from every part of the range (xorshift64 over the bits).
      let state = 0x9e3779b97f4a7c15n;
      const view = new DataView(new ArrayBuffer(8));
      const kept: number[] = [];
      for (let i = 0; i < 4000; i += 1) {
        state ^= (state << 13n) & 0xffffffffffffffffn;
        state ^= state >> 7n;
        state ^= (state << 17n) & 0xffffffffffffffffn;
        view.setBigUint64(0, state);
        const x = view.getFloat64(0);
        if (!Number.isFinite(x)) {
          continue;
        }
        if (Math.abs(x) > Number.MAX_SAFE_INTEGER) {
          if (i % 50 === 0) {
            expect(refused(hello('[]', JSON.stringify({ x }))), String(x)).toEqual(['B2C-E0112']);
          }
          continue;
        }
        // Quarters just above 2^50 are ties between two shortest spellings.
        kept.push(x, x / 3, Math.round(x), 2 ** 50 + (i % 1024) + 0.25);
      }
      expect(kept.length).toBeGreaterThan(1000);
      const { direct, edited } = throughTheEditor(hello('[]', JSON.stringify({ kept })));
      expect(edited.text === direct.text).toBe(true);
      expect(`${JSON.stringify(JSON.parse(direct.text), null, 2)}\n` === direct.text).toBe(true);
      const saved = (JSON.parse(direct.text) as { 'x-ext': { kept: number[] } })['x-ext'].kept;
      const zero = (x: number) => (Object.is(x, -0) ? 0 : x);
      expect(saved.map(zero)).toEqual(kept.map(zero));
    });
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
