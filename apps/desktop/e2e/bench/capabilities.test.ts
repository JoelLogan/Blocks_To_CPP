/**
 * The benchmarks' check of the app's end-to-end hook (capabilities.ts): which methods the page
 * lacks, what a benchmark does about them (run, skip with a note for the A/B job's base, or fail),
 * and that every benchmark that uses the hook checks its methods right after the launch.
 */
import { mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import type { TestContext } from 'vitest';
import { afterEach, describe, expect, it } from 'vitest';

import { E2E_HOOK_NAME } from '../../src/e2e/contract';
import type { App } from '../support/app';
import { runScript } from '../support/fake-page';
import {
  BENCH_HOOK_METHODS,
  type HookBenchmark,
  hookVerdict,
  MISSING_METHODS_SCRIPT,
  missingHookMethods,
  requireHookMethods,
  skipNotePath,
} from './capabilities';

const folders: string[] = [];

afterEach(() => {
  for (const folder of folders.splice(0)) {
    rmSync(folder, { recursive: true, force: true });
  }
});

/** An app whose page has `hook` (or none) as the end-to-end hook; only `executeScript` works. */
function appWith(hook: Record<string, unknown> | undefined): App {
  const window: Record<string, unknown> = {};
  if (hook !== undefined) {
    window[E2E_HOOK_NAME] = hook;
  }
  const driver = {
    executeScript: (script: string, ...args: unknown[]) =>
      Promise.resolve(runScript(script, { window }, args)),
  };
  return { driver } as unknown as App;
}

/** A hook with every method of `methods`, as functions. */
function hookOf(methods: readonly string[]): Record<string, unknown> {
  return Object.fromEntries(methods.map((method) => [method, () => undefined]));
}

describe('MISSING_METHODS_SCRIPT', () => {
  it('names the methods the hook lacks, or all of them without a hook', () => {
    const methods = ['code', 'scrollCanvasBy', 'grabPoint'];
    const window = { [E2E_HOOK_NAME]: { code: () => '', grabPoint: 'not a function' } };
    expect(runScript(MISSING_METHODS_SCRIPT, { window }, [E2E_HOOK_NAME, methods])).toEqual([
      'scrollCanvasBy',
      'grabPoint',
    ]);
    expect(runScript(MISSING_METHODS_SCRIPT, { window: {} }, [E2E_HOOK_NAME, methods])).toEqual(
      methods,
    );
  });
});

describe('missingHookMethods', () => {
  it('asks the page, and refuses an answer it did not ask for', async () => {
    expect(await missingHookMethods(appWith(hookOf(['code'])), ['code', 'grabPoint'])).toEqual([
      'grabPoint',
    ]);
    const odd = {
      driver: { executeScript: () => Promise.resolve(['ready']) },
    } as unknown as App;
    await expect(missingHookMethods(odd, ['code'])).rejects.toThrow(/did not say/);
  });
});

describe('hookVerdict', () => {
  const note = path.resolve(tmpdir(), 'skip-note.txt');

  it('runs when nothing is missing', () => {
    expect(hookVerdict('select', [], { B2C_BENCH_SKIP_NOTE: note })).toEqual({ kind: 'run' });
  });

  it('skips with a note when B2C_BENCH_SKIP_NOTE is set, and fails without it', () => {
    expect(hookVerdict('select', ['scrollCanvasBy'], { B2C_BENCH_SKIP_NOTE: note })).toEqual({
      kind: 'skip',
      note,
      reason: "the app's end-to-end hook has no scrollCanvasBy, which the select benchmark needs",
    });
    const fail = hookVerdict('guessing', ['scrollCanvasBy', 'grabPoint'], {});
    expect(fail.kind).toBe('fail');
    expect(fail.kind === 'fail' && fail.message).toMatch(
      /^The app's end-to-end hook has no scrollCanvasBy, grabPoint, which the guessing benchmark needs: build the app under test/,
    );
  });

  it('takes only an absolute note path', () => {
    expect(skipNotePath({})).toBeNull();
    expect(skipNotePath({ B2C_BENCH_SKIP_NOTE: '' })).toBeNull();
    expect(() => skipNotePath({ B2C_BENCH_SKIP_NOTE: 'note.txt' })).toThrow(/absolute/);
  });
});

describe('requireHookMethods', () => {
  /** A test context whose `skip` throws, as Vitest's does, recording the note. */
  function context(): { readonly context: TestContext; readonly skipped: string[] } {
    const skipped: string[] = [];
    const skip = (reason: string): never => {
      skipped.push(reason);
      throw new Error('skipped');
    };
    return { context: { skip } as unknown as TestContext, skipped };
  }

  it('returns when the app has every method the benchmark calls', async () => {
    const { context: ctx, skipped } = context();
    await requireHookMethods(ctx, appWith(hookOf(BENCH_HOOK_METHODS.guessing)), 'guessing', {});
    expect(skipped).toEqual([]);
  });

  it("skips and writes the note for the A/B job's base", async () => {
    const folder = mkdtempSync(path.join(tmpdir(), 'b2c-bench-capabilities-'));
    folders.push(folder);
    const note = path.join(folder, 'notes', 'base-round-1-select.bench.txt');
    const { context: ctx, skipped } = context();
    const older = hookOf(['blockElement', 'code', 'grabPoint', 'flyoutBlockId']);
    await expect(
      requireHookMethods(ctx, appWith(older), 'select', { B2C_BENCH_SKIP_NOTE: note }),
    ).rejects.toThrow('skipped');
    const reason =
      "the app's end-to-end hook has no scrollCanvasBy, which the select benchmark needs";
    expect(skipped).toEqual([reason]);
    expect(readFileSync(note, 'utf8')).toBe(`${reason}\n`);
  });

  it('fails without a note path', async () => {
    const { context: ctx, skipped } = context();
    await expect(requireHookMethods(ctx, appWith({}), 'console', {})).rejects.toThrow(
      /has no document, insertBlocks, consoleText, which the console benchmark needs/,
    );
    expect(skipped).toEqual([]);
  });
});

describe('BENCH_HOOK_METHODS', () => {
  const folder = import.meta.dirname;

  it('lists every benchmark that launches the app, and each checks its own list after the launch', () => {
    const launching = readdirSync(folder)
      .filter((file) => file.endsWith('.bench.e2e.ts'))
      .filter((file) => readFileSync(path.join(folder, file), 'utf8').includes('launchApp('))
      .map((file) => file.replace('.bench.e2e.ts', ''))
      .sort();
    expect(Object.keys(BENCH_HOOK_METHODS).sort()).toEqual(launching);
    for (const name of launching as HookBenchmark[]) {
      const source = readFileSync(path.join(folder, `${name}.bench.e2e.ts`), 'utf8');
      // The check comes right after the launch, before anything uses the hook.
      expect(source).toMatch(
        new RegExp(
          String.raw`= await launchApp\([^;]*\);\n\s*await requireHookMethods\(context, app, '${name}'\);`,
        ),
      );
    }
  });
});
