/**
 * What each webview benchmark needs of the app's end-to-end hook (src/e2e/contract.ts), and the
 * check that the app under test has it.
 *
 * The A/B job (ab-rounds.sh, docs/spec/09-quality-and-delivery.md §9.2) runs the head's
 * benchmarks against the base's build too, and the base's hook may lack a method that came later
 * (`scrollCanvasBy` came with the benchmarks' W2-1 core, so a base before it cannot run the
 * selection benchmarks). Each benchmark that uses the hook checks its methods right after the
 * launch:
 *
 * - with `B2C_BENCH_SKIP_NOTE` set (an absolute path; ab-rounds.sh sets it for the base's runs),
 *   a benchmark whose methods are missing writes why into that file and is skipped, so it neither
 *   fails nor stops the other benchmarks;
 * - without it (the nightly job, the head's runs, local runs), a missing method fails the
 *   benchmark at once with the methods' names, rather than halfway through its samples.
 *
 * A benchmark that starts to call another hook method adds it to its list here; a method it lacks
 * in the list would make an older base fail that benchmark instead of skipping it (which the A/B
 * job reports, but does not fail on).
 */
import { mkdirSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import type { TestContext } from 'vitest';

import { E2E_HOOK_NAME, type E2eHookContract } from '../../src/e2e/contract';
import type { App } from '../support/app';

/** A method of the end-to-end hook. */
export type HookMethod = keyof E2eHookContract;

/**
 * The hook methods each benchmark calls once the app has started (beyond `ready`, which every
 * launch waits for), by the name of its file (`<name>.bench.e2e.ts`). Only what a measurement
 * needs: what is read only to describe a failure (the project's text after a lost drop, the
 * console's transcript of a failed test) is left out. The cold-start benchmark is not here: it
 * watches the page for the readiness marker and calls no hook method.
 */
export const BENCH_HOOK_METHODS = {
  // Opening the document (openProjectFile), the edits (EDIT_SCRIPT) and deleting them.
  preview: ['blockElement', 'code', 'insertBlocks', 'selectBlock'],
  // Opening the document, then timedClick: scrolling each target into view and where to press.
  select: ['blockElement', 'code', 'scrollCanvasBy', 'grabPoint'],
  // The clicks of `select` and the edits of `preview`, on the guessing game.
  guessing: ['blockElement', 'code', 'scrollCanvasBy', 'grabPoint', 'insertBlocks', 'selectBlock'],
  // Opening each document, the declarations (EDIT_SCRIPT) and deleting them.
  declare: ['blockElement', 'code', 'insertBlocks', 'selectBlock'],
  // The Empty template's `main` (from the document), the program, and the console's transcript.
  console: ['document', 'insertBlocks', 'consoleText'],
  // Opening the document, centring on the handle, where the handle is and where to grab it.
  drag: ['blockElement', 'code', 'selectBlock', 'grabPoint'],
  // Opening the document, centring on the number block, where it and the column's inputs are, the
  // document after each drop, and the keyboard listing's span.
  'typed-slots': [
    'blockElement',
    'code',
    'selectBlock',
    'grabPoint',
    'connectionPoint',
    'document',
    'timings',
  ],
} as const satisfies Readonly<Record<string, readonly HookMethod[]>>;

/** A benchmark that uses the hook. */
export type HookBenchmark = keyof typeof BENCH_HOOK_METHODS;

/**
 * Runs in the webview (`executeScript(MISSING_METHODS_SCRIPT, hookName, methods)`): those of
 * `methods` that the hook does not have as functions (all of them without a hook).
 */
export const MISSING_METHODS_SCRIPT = `
  const [hookName, methods] = arguments;
  const hook = window[hookName];
  if (typeof hook !== 'object' || hook === null) {
    return methods;
  }
  return methods.filter((method) => typeof hook[method] !== 'function');
`;

/** What a benchmark does about the hook methods the app lacks. */
export type HookVerdict =
  | { readonly kind: 'run' }
  /** Skipped: `reason` is written to `note`, the file `B2C_BENCH_SKIP_NOTE` names. */
  | { readonly kind: 'skip'; readonly note: string; readonly reason: string }
  | { readonly kind: 'fail'; readonly message: string };

/** The longest reason a skip note holds (ab-rounds.sh keeps 300 characters of it). */
const MAX_REASON = 280;

/**
 * Where a skipped benchmark says why: `B2C_BENCH_SKIP_NOTE` (absolute), or `null` when it is not
 * set and a missing method fails the benchmark.
 */
export function skipNotePath(env: NodeJS.ProcessEnv = process.env): string | null {
  const note = env['B2C_BENCH_SKIP_NOTE'];
  if (note === undefined || note === '') {
    return null;
  }
  if (!path.isAbsolute(note)) {
    throw new Error('B2C_BENCH_SKIP_NOTE must be an absolute path');
  }
  return note;
}

/** What `benchmark` does when the app's hook lacks `missing` (none: it runs). */
export function hookVerdict(
  benchmark: HookBenchmark,
  missing: readonly string[],
  env: NodeJS.ProcessEnv = process.env,
): HookVerdict {
  if (missing.length === 0) {
    return { kind: 'run' };
  }
  const reason = `the app's end-to-end hook has no ${missing.join(', ')}, which the ${benchmark} benchmark needs`;
  const note = skipNotePath(env);
  if (note === null) {
    return {
      kind: 'fail',
      message:
        `The app's end-to-end hook has no ${missing.join(', ')}, which the ${benchmark} benchmark ` +
        'needs: build the app under test from the commit of the benchmarks (or set ' +
        'B2C_BENCH_SKIP_NOTE to skip such a benchmark, as the A/B job does for its base)',
    };
  }
  return { kind: 'skip', note, reason: reason.slice(0, MAX_REASON) };
}

/** The methods of `methods` that the app's hook lacks, asked in one script. */
export async function missingHookMethods(
  app: App,
  methods: readonly HookMethod[],
): Promise<string[]> {
  const missing: unknown = await app.driver.executeScript(
    MISSING_METHODS_SCRIPT,
    E2E_HOOK_NAME,
    methods,
  );
  if (
    !Array.isArray(missing) ||
    !missing.every((method) => typeof method === 'string' && methods.includes(method as HookMethod))
  ) {
    throw new Error('The page did not say which end-to-end hook methods it lacks');
  }
  return missing as string[];
}

/**
 * Checks, right after the launch, that the app's hook has every method `benchmark` calls
 * ({@link BENCH_HOOK_METHODS}): returns when it has, skips the test (writing the reason to
 * `B2C_BENCH_SKIP_NOTE`) or throws otherwise ({@link hookVerdict}).
 */
export async function requireHookMethods(
  context: TestContext,
  app: App,
  benchmark: HookBenchmark,
  env: NodeJS.ProcessEnv = process.env,
): Promise<void> {
  const verdict = hookVerdict(
    benchmark,
    await missingHookMethods(app, BENCH_HOOK_METHODS[benchmark]),
    env,
  );
  switch (verdict.kind) {
    case 'run':
      return;
    case 'fail':
      throw new Error(verdict.message);
    case 'skip':
      mkdirSync(path.dirname(verdict.note), { recursive: true });
      writeFileSync(verdict.note, `${verdict.reason}\n`);
      process.stdout.write(`Skipped: ${verdict.reason}\n`);
      context.skip(verdict.reason);
  }
}
