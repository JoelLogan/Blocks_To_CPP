// The scope query, block types, conversion table and the clipboard through the TypeScript API
// with the real compiler core, built by `pnpm --filter @blocks2cpp/b2c-core-wasm build`. The same
// cases as crates/b2c-core-wasm/tests/{scope,clipboard}.rs. Skipped without a build unless
// B2C_REQUIRE_WASM is set (as in CI).

import { existsSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { beforeAll, describe, expect, it } from 'vitest';

import type {
  BdmBlock,
  ConversionRow,
  CoreWasm,
  Diagnostic,
  PastePrepareResult,
  PasteTarget,
  StaticConversion,
  StaticType,
} from '../src/index';
import { CoreError, initCoreFromBytes, randomSeedHex } from '../src/index';

const repo = (path: string) => fileURLToPath(new URL(`../../../${path}`, import.meta.url));
const builtModule = fileURLToPath(new URL('../pkg/b2c_core_wasm_bg.wasm', import.meta.url));
const built = existsSync(builtModule);
const required = (process.env['B2C_REQUIRE_WASM'] ?? '') !== '';

const example = (name: string) => readFileSync(repo(`examples/${name}.b2c`), 'utf8');
const codes = (diagnostics: Diagnostic[]) => diagnostics.map((d) => d.code);
const names = (symbols: { name: string }[]) => symbols.map((s) => s.name);
const seed = (n: number) => n.toString(16).padStart(64, '0');

/** A JSON tree as plain data, for the tests' own edits. */
type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

/** The block node with this ID anywhere in a tree. */
function findBlock(value: Json, id: string): Record<string, Json> | undefined {
  if (Array.isArray(value)) {
    for (const item of value) {
      const found = findBlock(item, id);
      if (found !== undefined) {
        return found;
      }
    }
  } else if (typeof value === 'object' && value !== null) {
    if (value['id'] === id && typeof value['type'] === 'string') {
      return value;
    }
    for (const child of Object.values(value)) {
      const found = findBlock(child, id);
      if (found !== undefined) {
        return found;
      }
    }
  }
  return undefined;
}

/** The list holding the block with this ID, and its index there. */
function listHolding(value: Json, id: string): [Json[], number] | undefined {
  if (Array.isArray(value)) {
    const at = value.findIndex(
      (item) =>
        typeof item === 'object' && item !== null && !Array.isArray(item) && item['id'] === id,
    );
    if (at >= 0) {
      return [value, at];
    }
    for (const item of value) {
      const found = listHolding(item, id);
      if (found !== undefined) {
        return found;
      }
    }
  } else if (typeof value === 'object' && value !== null) {
    for (const child of Object.values(value)) {
      const found = listHolding(child, id);
      if (found !== undefined) {
        return found;
      }
    }
  }
  return undefined;
}

/** Inserts pasted blocks where the target says, as the editor would. */
function insert(document: string, target: PasteTarget, blocks: BdmBlock[]): string {
  const tree = JSON.parse(document) as Json;
  const copies = structuredClone(blocks) as unknown as Json[];
  if (target.block === null) {
    const modules = (tree as { modules: { id: string; workspace: { blocks: Json[] } }[] }).modules;
    const module = modules.find((m) => m.id === target.module);
    copies.forEach((block, i) => {
      module?.workspace.blocks.push({ ...(block as Record<string, Json>), x: 1000 + i, y: 1000 });
    });
  } else if (target.input !== null) {
    const node = findBlock(tree, target.block);
    const statements = node?.['statements'] as Record<string, Json[]> | undefined;
    statements?.[target.input]?.unshift(...copies);
  } else {
    const found = listHolding(tree, target.block);
    found?.[0].splice(found[1] + 1, 0, ...copies);
  }
  return JSON.stringify(tree, null, 2);
}

/** Every block ID in a tree. */
function blockIds(value: unknown, out: string[] = []): string[] {
  if (Array.isArray(value)) {
    value.forEach((item) => blockIds(item, out));
  } else if (typeof value === 'object' && value !== null) {
    const record = value as Record<string, unknown>;
    if (typeof record['id'] === 'string' && typeof record['type'] === 'string') {
      out.push(record['id']);
    }
    Object.values(record).forEach((child) => blockIds(child, out));
  }
  return out;
}

/** `hello_world` with `create int guess = 0` (c1) and `create int secret = 5` (c2). */
function otherDocument(): string {
  const tree = JSON.parse(example('hello_world')) as Json;
  const main = findBlock(tree, 'b002');
  const body = (main?.['statements'] as Record<string, Json[]>)['BODY'];
  const declare = (id: string, sym: string, name: string, value: string): Json => ({
    id,
    type: 'var.declare',
    v: 1,
    fields: { CONST: false, NAME: { sym, name }, TYPE: 'int' },
    inputs: { VALUE: { expr: [{ num: value }] } },
  });
  body?.unshift(declare('c1', 't_guess', 'guess', '0'), declare('c2', 't_secret', 'secret', '5'));
  return JSON.stringify(tree, null, 2);
}

/** The analyser's conversion rule (`b2c_lang::conversion`), written out from its documentation. */
function expectedConversion(from: StaticType, to: StaticType): StaticConversion {
  const numbers: StaticType[] = ['int', 'double', 'char'];
  if (from === 'error' || to === 'error' || from === to) {
    return 'same';
  }
  if ((from === 'bool' && numbers.includes(to)) || (numbers.includes(from) && to === 'bool')) {
    return 'boolNumber';
  }
  if (
    (to === 'double' && (from === 'int' || from === 'char')) ||
    (from === 'char' && to === 'int')
  ) {
    return 'widening';
  }
  if ((from === 'double' && (to === 'int' || to === 'char')) || (from === 'int' && to === 'char')) {
    return 'narrowing';
  }
  return 'invalid';
}

/** The rows of tests/security/clipboard/README.md: file and loader codes (null: accepted). */
function clipboardCases(): { file: string; loader: string[] | null }[] {
  const readme = readFileSync(repo('tests/security/clipboard/README.md'), 'utf8');
  return readme
    .split('\n')
    .filter((line) => line.startsWith('| `'))
    .map((line) => {
      const cells = line.split('|').map((cell) => cell.trim());
      const loader = cells[4] ?? '';
      return {
        file: (cells[1] ?? '').replaceAll('`', ''),
        loader:
          loader === 'accepted' ? null : loader.split('`').filter((p) => p.startsWith('B2C-')),
      };
    });
}

function pasted(result: PastePrepareResult): BdmBlock[] {
  if (!result.ok) {
    throw new Error(`the paste failed: ${codes(result.diagnostics).join(', ')}`);
  }
  return result.blocks;
}

describe.skipIf(!built && !required)('scope and clipboard in the built core', () => {
  let core: CoreWasm;

  beforeAll(async () => {
    // An independent instance, so the kept preview is only this suite's.
    core = await initCoreFromBytes(new Uint8Array(readFileSync(builtModule)));
  });

  describe('symbolsInScope', () => {
    it('answers nothing before a successful preview', () => {
      core.preview('{', { indentWidth: 4 });
      expect(core.symbolsInScope('b005', null)).toEqual([]);
    });

    it('lists guess and secret at the guessing game ask block, and nothing at b002', () => {
      core.preview(example('guessing_game'), { indentWidth: 4 });
      expect(names(core.symbolsInScope('b005', null))).toEqual(['guess', 'secret']);
      expect(core.symbolsInScope('b002', null)).toEqual([]);
      expect(names(core.symbolsInScope('b010', 'BODY'))).toEqual(['guess', 'secret']);
      expect(core.symbolsInScope('b005', null)[0]).toEqual({
        id: 's_guess',
        name: 'guess',
        kind: 'variable',
        isConst: false,
        type: 'int',
        module: 'mod_main',
        declBlock: 'b003',
      });
      expect(core.symbolsInScope('nope', null)).toEqual([]);
    });

    it('sees loop counters, parameters and functions', () => {
      core.preview(example('factorial'), { indentWidth: 4 });
      expect(names(core.symbolsInScope('b003', null))).toEqual(['factorial', 'n']);
      expect(names(core.symbolsInScope('b007', null))).toEqual(['factorial']);
      expect(names(core.symbolsInScope('b007', 'BODY'))).toEqual(['factorial', 'i']);
      const [factorial] = core.symbolsInScope('b007', null);
      expect(factorial).toMatchObject({ kind: 'function', returns: 'int', params: ['s_n'] });
    });
  });

  describe('preview', () => {
    it('fills symbols and block types', () => {
      const game = core.preview(example('guessing_game'), { indentWidth: 4 });
      expect(game.blockTypes).toEqual({ b001: 'int' });
      expect(names(game.symbols)).toEqual(['guess', 'secret']);
      const factorial = core.preview(example('factorial'), { indentWidth: 4 });
      expect(factorial.blockTypes).toEqual({ b005: 'int' });
      expect(names(factorial.symbols)).toEqual(['factorial', 'i', 'n']);
    });
  });

  describe('conversionTable', () => {
    it('is the analyser rule for every pair', () => {
      const table: ConversionRow[] = core.conversionTable();
      const types: StaticType[] = ['void', 'bool', 'char', 'int', 'double', 'string', 'error'];
      expect(table).toHaveLength(49);
      expect(table).toEqual(
        types.flatMap((from) =>
          types.map((to) => ({ from, to, conversion: expectedConversion(from, to) })),
        ),
      );
    });
  });

  describe('clipboard', () => {
    it('copies blocks with their outside references and their C++', () => {
      const game = example('guessing_game');
      core.preview(game, { indentWidth: 4 });
      const made = core.clipboardMake(game, ['b009']);
      if (!made.ok) {
        throw new Error('the copy failed');
      }
      const payload = JSON.parse(made.payload) as { refs: Record<string, unknown> };
      expect(payload.refs).toEqual({
        s_guess: { name: 'guess', kind: 'variable' },
        s_secret: { name: 'secret', kind: 'variable' },
      });
      expect(made.text).toMatch(/^if \(guess < secret\) \{\n/);
      expect(made.text?.endsWith('}\n')).toBe(true);
      expect(made.payload).not.toContain('"x"');
    });

    it('round-trips: a paste next to the original builds', () => {
      const game = example('guessing_game');
      const made = core.clipboardMake(game, ['b009']);
      if (!made.ok) {
        throw new Error('the copy failed');
      }
      const target: PasteTarget = { module: 'mod_main', block: 'b005', input: null };
      const result = core.pastePrepare(made.payload, game, target, randomSeedHex());
      const blocks = pasted(result);
      expect(result.unresolved).toEqual([]);
      expect(blockIds(blocks)).toHaveLength(4);
      const before = new Set(blockIds(JSON.parse(game)));
      for (const id of blockIds(blocks)) {
        expect(before.has(id)).toBe(false);
      }
      const preview = core.preview(insert(game, target, blocks), { indentWidth: 4 });
      expect(preview.buildable).toBe(true);
      expect(preview.files[0]?.contents.split('Too low!')).toHaveLength(3);
    });

    it('re-binds by name where the names are visible, and gives E0201 elsewhere', () => {
      const made = core.clipboardMake(example('guessing_game'), ['b009']);
      if (!made.ok) {
        throw new Error('the copy failed');
      }
      const other = otherDocument();
      const after: PasteTarget = { module: 'mod_main', block: 'c2', input: null };
      const bound = core.pastePrepare(made.payload, other, after, seed(1));
      expect(bound.unresolved).toEqual([]);
      const text = JSON.stringify(pasted(bound));
      expect(text).toContain('t_guess');
      expect(text).not.toContain('s_guess');
      expect(core.preview(insert(other, after, pasted(bound)), { indentWidth: 4 }).buildable).toBe(
        true,
      );

      const start: PasteTarget = { module: 'mod_main', block: 'b002', input: 'BODY' };
      const outside = core.pastePrepare(made.payload, other, start, seed(2));
      expect(outside.unresolved).toEqual([
        { sym: 's_guess', name: 'guess' },
        { sym: 's_secret', name: 'secret' },
      ]);
      expect(codes(outside.diagnostics)).toEqual(['B2C-E0201', 'B2C-E0201']);
      expect(outside.diagnostics[0]?.message).toContain('`guess`');
      const preview = core.preview(insert(other, start, pasted(outside)), { indentWidth: 4 });
      expect(codes(preview.diagnostics)).toContain('B2C-E0201');
    });

    it('gives fresh IDs on every duplicate (1,000 times, the document still loads)', () => {
      let document = example('guessing_game');
      const made = core.clipboardMake(document, ['b005']);
      if (!made.ok) {
        throw new Error('the copy failed');
      }
      const target: PasteTarget = { module: 'mod_main', block: 'b005', input: null };
      const seen = new Set(blockIds(JSON.parse(document)));
      for (let i = 0; i < 1000; i += 1) {
        const blocks = pasted(core.pastePrepare(made.payload, document, target, randomSeedHex()));
        for (const id of blockIds(blocks)) {
          expect(seen.has(id)).toBe(false);
          seen.add(id);
        }
        document = insert(document, target, blocks);
      }
      const loaded = core.load(new TextEncoder().encode(document));
      expect(loaded.ok).toBe(true);
      expect(core.preview(document, { indentWidth: 4 }).buildable).toBe(true);
    });

    it.each(clipboardCases())('gives $file its expected loader outcome', ({ file, loader }) => {
      const text = readFileSync(repo(`tests/security/clipboard/${file}`), 'utf8');
      const target: PasteTarget = { module: 'mod_main', block: null, input: null };
      const result = core.pastePrepare(text, example('guessing_game'), target, seed(3));
      if (loader === null) {
        expect(result.ok).toBe(true);
      } else {
        expect(result.ok).toBe(false);
        expect([...new Set(codes(result.diagnostics))].sort()).toEqual([...loader].sort());
      }
    });

    it('refuses generated attacks with the loader codes', () => {
      const game = example('guessing_game');
      const made = core.clipboardMake(game, ['b005']);
      if (!made.ok) {
        throw new Error('the copy failed');
      }
      const target: PasteTarget = { module: 'mod_main', block: null, input: null };
      const refused = (text: string) => {
        const result = core.pastePrepare(text, game, target, seed(4));
        expect(result.ok).toBe(false);
        return codes(result.diagnostics);
      };
      expect(refused(`${'['.repeat(10_000)}${']'.repeat(10_000)}`)).toEqual(['B2C-E0104']);
      expect(refused(`{"pad": "${'x'.repeat(33 * 1024 * 1024)}"}`)).toEqual(['B2C-E0101']);
      expect(
        refused(made.payload.replace('"catalog": "1.0.0",', '"catalog": "1.0.0", "catalog": "2",')),
      ).toEqual(['B2C-E0105']);
      expect(
        refused(
          made.payload.replace(
            '"refs": {',
            '"refs": {"__proto__": {"name": "x", "kind": "variable"},',
          ),
        ),
      ).toContain('B2C-E0127');
      expect(refused(made.payload.replace('blocks2cpp/clipboard', 'blockly/clipboard'))).toEqual([
        'B2C-E0138',
      ]);
      expect(refused(game)).toEqual(['B2C-E0138']);
    });

    it('refuses bad arguments with CoreError', () => {
      const game = example('guessing_game');
      const made = core.clipboardMake(game, ['b005']);
      if (!made.ok) {
        throw new Error('the copy failed');
      }
      const invalid = (run: () => unknown) => {
        try {
          run();
        } catch (error) {
          expect(error).toBeInstanceOf(CoreError);
          return (error as CoreError).kind;
        }
        throw new Error('expected a CoreError');
      };
      expect(invalid(() => core.clipboardMake(game, ['nope']))).toBe('invalidArguments');
      expect(invalid(() => core.clipboardMake(game, ['b005', 'b005']))).toBe('invalidArguments');
      const target: PasteTarget = { module: 'mod_main', block: null, input: null };
      expect(invalid(() => core.pastePrepare(made.payload, game, target, 'short'))).toBe(
        'invalidArguments',
      );
      expect(
        invalid(() =>
          core.pastePrepare(made.payload, game, { ...target, module: 'nope' }, seed(5)),
        ),
      ).toBe('invalidArguments');
      // The instance keeps working.
      expect(core.version().catalog).toBe('1.0.0');
    });
  });
});
