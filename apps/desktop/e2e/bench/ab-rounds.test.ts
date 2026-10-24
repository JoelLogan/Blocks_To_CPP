/**
 * The A/B rounds (ab-rounds.sh) with a stand-in for `pnpm … vitest run` on `PATH`, so no app runs:
 * each benchmark in a call of its own, the sides alternating round by round, a benchmark the base
 * cannot run skipped on both sides, a failed one not started again (but run by the head in the
 * round the base failed it), the record they leave (ab-runs.tsv), and what the comparison
 * (tools/bench-compare.py ab --runs) makes of it, when Python is installed. Needs bash (Linux CI).
 */
import { spawnSync } from 'node:child_process';
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import { REPOSITORY_ROOT } from '../support/env';

const SCRIPT = path.join(REPOSITORY_ROOT, 'apps', 'desktop', 'e2e', 'bench', 'ab-rounds.sh');
const COMPARE = path.join(REPOSITORY_ROOT, 'tools', 'bench-compare.py');

/**
 * Stands in for `pnpm --filter @blocks2cpp/desktop exec vitest run --config … --project bench
 * <benchmark>`: logs the call (side, round, the benchmarks after `--project bench`, whether a skip
 * note was offered, the artifacts folder), then does what `FAKE_SCENARIO` says for this side,
 * benchmark and round (lines `<side> <benchmark> <round or *> <ok|skip|fail>`; ok by default): a
 * results file of `B2C_BENCH_SAMPLES` samples of `FAKE_<SIDE>_MEDIAN` ms, a skip note, or a
 * failure.
 */
const FAKE_PNPM = `#!/usr/bin/env bash
set -u
args=("$@")
benches=()
for ((i = 0; i < \${#args[@]}; i++)); do
  if [ "\${args[i]}" = --project ]; then benches=("\${args[@]:i+2}"); fi
done
bench=\${benches[0]}
side=base
if [ "$B2C_E2E_APP" = "$FAKE_HEAD_APP" ]; then side=head; fi
round=$((B2C_BENCH_OFFSET / B2C_BENCH_SAMPLES + 1))
printf '%s %s %s note=%s artifacts=%s\\n' "$side" "$round" "\${benches[*]}" \\
  "\${B2C_BENCH_SKIP_NOTE:+yes}" "\${B2C_E2E_ARTIFACTS:-}" >> "$FAKE_LOG"
action=ok
while read -r s b r a; do
  if [ "$s" = "$side" ] && [ "$b" = "$bench" ] && { [ "$r" = "*" ] || [ "$r" = "$round" ]; }; then
    action=$a
  fi
done < "$FAKE_SCENARIO"
case "$action" in
  skip)
    if [ -z "\${B2C_BENCH_SKIP_NOTE:-}" ]; then exit 1; fi
    printf "the app's end-to-end hook has no scrollCanvasBy, which the %s benchmark needs\\n" \\
      "\${bench%.bench}" > "$B2C_BENCH_SKIP_NOTE"
    exit 0
    ;;
  fail) exit 1 ;;
esac
median=$FAKE_BASE_MEDIAN
if [ "$side" = head ]; then median=$FAKE_HEAD_MEDIAN; fi
samples=$median
for ((i = 1; i < B2C_BENCH_SAMPLES; i++)); do samples="$samples, $median"; done
mkdir -p "$B2C_BENCH_OUT"
printf '{"format": "blocks2cpp/bench-results", "formatVersion": 1, "os": "linux", "metrics": [{"name": "webview.%s", "unit": "ms", "better": "lower", "gate": true, "description": "fake", "samples": [%s]}]}\\n' \\
  "\${bench%.bench}" "$samples" > "$B2C_BENCH_OUT/webview.\${bench%.bench}.json"
`;

/** Whether bash and python3 can be run here. */
const bash = process.platform !== 'win32' && spawnSync('bash', ['--version']).status === 0;
const python = spawnSync('python3', ['--version'], { encoding: 'utf8' }).status === 0;

const folders: string[] = [];

afterEach(() => {
  for (const folder of folders.splice(0)) {
    rmSync(folder, { recursive: true, force: true });
  }
});

/** A folder with the two apps (empty files), the stand-in `pnpm` and the call log. */
interface Setup {
  readonly root: string;
  readonly base: string;
  readonly head: string;
  readonly out: string;
  readonly log: string;
  readonly scenario: string;
  readonly bin: string;
}

function setup(scenario: readonly string[] = []): Setup {
  const root = mkdtempSync(path.join(tmpdir(), 'b2c-ab-rounds-'));
  folders.push(root);
  const bin = path.join(root, 'bin');
  mkdirSync(bin);
  writeFileSync(path.join(bin, 'pnpm'), FAKE_PNPM);
  chmodSync(path.join(bin, 'pnpm'), 0o755);
  for (const side of ['base', 'head']) {
    mkdirSync(path.join(root, side));
    writeFileSync(path.join(root, side, 'app'), '');
  }
  const file = path.join(root, 'scenario.txt');
  writeFileSync(file, scenario.map((line) => `${line}\n`).join(''));
  return {
    root,
    base: path.join(root, 'base', 'app'),
    head: path.join(root, 'head', 'app'),
    out: path.join(root, 'out'),
    log: path.join(root, 'calls.log'),
    scenario: file,
    bin,
  };
}

/** Runs ab-rounds.sh (2 rounds of 5 samples) on `benches`; `env` adds to the settings. */
function rounds(
  s: Setup,
  benches: readonly string[],
  env: Readonly<Record<string, string>> = {},
): ReturnType<typeof spawnSync> {
  // The caller's own settings of the script stay out.
  const unset = new Set(['AB_XVFB', 'B2C_E2E_ARTIFACTS', 'B2C_BENCH_SKIP_NOTE']);
  const settings = Object.fromEntries(
    Object.entries(process.env).filter(([name]) => !unset.has(name)),
  );
  return spawnSync('bash', [SCRIPT, s.base, s.head, s.out, ...benches], {
    encoding: 'utf8',
    env: {
      ...settings,
      PATH: `${s.bin}${path.delimiter}${process.env['PATH'] ?? ''}`,
      AB_ROUNDS: '2',
      AB_SAMPLES: '5',
      FAKE_HEAD_APP: s.head,
      FAKE_LOG: s.log,
      FAKE_SCENARIO: s.scenario,
      FAKE_BASE_MEDIAN: '100',
      FAKE_HEAD_MEDIAN: '100',
      ...env,
    },
  });
}

/** The calls the stand-in logged: `<side> <round> <benchmarks> note=<yes|> artifacts=<…>`. */
function calls(s: Setup): string[] {
  return existsSync(s.log) ? readFileSync(s.log, 'utf8').trimEnd().split('\n') : [];
}

/** The record's lines after its header, as fields. */
function record(s: Setup): string[][] {
  const text = readFileSync(path.join(s.out, 'ab-runs.tsv'), 'utf8');
  expect(text.endsWith('\n')).toBe(true);
  const lines = text.slice(0, -1).split('\n');
  expect(lines[0]).toBe('blocks2cpp/ab-runs\t1');
  return lines.slice(1).map((line) => line.split('\t'));
}

/** The benchmarks with a results folder under `<out>/<part>/round-<n>/`. */
function resultsIn(s: Setup, part: string, round: number): string[] {
  const folder = path.join(s.out, part, `round-${String(round)}`);
  return existsSync(folder) ? readdirSync(folder).sort() : [];
}

/** Runs `bench-compare.py ab --runs` on the rounds' output. */
function compare(s: Setup): ReturnType<typeof spawnSync> {
  return spawnSync(
    'python3',
    [
      COMPARE,
      'ab',
      '--os',
      'linux',
      '--base',
      path.join(s.out, 'base'),
      '--head',
      path.join(s.out, 'head'),
      '--runs',
      path.join(s.out, 'ab-runs.tsv'),
    ],
    { encoding: 'utf8' },
  );
}

describe.skipIf(!bash)('ab-rounds.sh', () => {
  it('runs each benchmark in a call of its own, base and head alternating round by round', () => {
    const s = setup();
    const run = rounds(s, ['preview.bench', 'select.bench'], { B2C_E2E_ARTIFACTS: '/artifacts' });
    expect(run.stderr).toBe('');
    expect(run.status).toBe(0);
    expect(calls(s)).toEqual([
      'base 1 preview.bench note=yes artifacts=/artifacts/base/round-1/preview.bench',
      'base 1 select.bench note=yes artifacts=/artifacts/base/round-1/select.bench',
      'head 1 preview.bench note= artifacts=/artifacts/head/round-1/preview.bench',
      'head 1 select.bench note= artifacts=/artifacts/head/round-1/select.bench',
      'head 2 preview.bench note= artifacts=/artifacts/head/round-2/preview.bench',
      'head 2 select.bench note= artifacts=/artifacts/head/round-2/select.bench',
      'base 2 preview.bench note=yes artifacts=/artifacts/base/round-2/preview.bench',
      'base 2 select.bench note=yes artifacts=/artifacts/base/round-2/select.bench',
    ]);
    expect(record(s)).toEqual([
      ['preview.bench', 'compared', '0', '0', ''],
      ['select.bench', 'compared', '0', '0', ''],
    ]);
    expect(resultsIn(s, 'base', 2)).toEqual(['preview.bench', 'select.bench']);
    expect(resultsIn(s, 'head', 1)).toEqual(['preview.bench', 'select.bench']);
  });

  it('skips what the base cannot run, leaves out what failed, and keeps going', () => {
    const s = setup([
      'base select.bench * skip',
      'base drag.bench 1 fail',
      'head console.bench 2 fail',
    ]);
    const run = rounds(s, ['preview.bench', 'select.bench', 'drag.bench', 'console.bench']);
    expect(run.status).toBe(0);
    expect(calls(s).map((call) => call.split(' ').slice(0, 3).join(' '))).toEqual([
      // Round 1, base first: select skips, drag fails.
      'base 1 preview.bench',
      'base 1 select.bench',
      'base 1 drag.bench',
      'base 1 console.bench',
      // The head never runs the skipped benchmark, but runs drag once: it may fail there too.
      'head 1 preview.bench',
      'head 1 drag.bench',
      'head 1 console.bench',
      // Round 2, head first: console fails on the head, so the base does not run it again.
      'head 2 preview.bench',
      'head 2 console.bench',
      'base 2 preview.bench',
    ]);
    expect(record(s)).toEqual([
      ['preview.bench', 'compared', '0', '0', ''],
      [
        'select.bench',
        'skipped',
        '0',
        '0',
        "the app's end-to-end hook has no scrollCanvasBy, which the select benchmark needs",
      ],
      ['drag.bench', 'failed', '1', '0', ''],
      ['console.bench', 'failed', '0', '2', ''],
    ]);
    // Only what ran in every round on both sides stays to be compared.
    for (const side of ['base', 'head']) {
      for (const round of [1, 2]) {
        expect(resultsIn(s, side, round)).toEqual(['preview.bench']);
      }
    }
    expect(resultsIn(s, 'left-out/base', 1)).toEqual([
      'console.bench',
      'drag.bench',
      'select.bench',
    ]);
    expect(resultsIn(s, 'left-out/head', 1)).toEqual(['console.bench', 'drag.bench']);
    expect(resultsIn(s, 'left-out/head', 2)).toEqual(['console.bench']);
  });

  it.skipIf(!python)('feeds the comparison, which fails only for the head', () => {
    const baseOnly = setup(['base select.bench * skip', 'base drag.bench 2 fail']);
    expect(rounds(baseOnly, ['preview.bench', 'select.bench', 'drag.bench']).status).toBe(0);
    const passed = compare(baseOnly);
    expect(passed.stderr).toBe('');
    expect(passed.stdout).toContain(
      '| `webview.preview` | yes | 100.0 ms (10) | 100.0 ms (10) | +0.0% | ok |',
    );
    expect(passed.stdout).toContain('- `select.bench`: skipped, as the base build cannot run it');
    expect(passed.stdout).toContain(
      '- `drag.bench`: failed on the base in round 2; the head ran it without failing.',
    );
    // Green, but it says nothing about the benchmarks left out.
    expect(passed.stdout).toContain(
      'Not conclusive: `select.bench`, `drag.bench` were not compared',
    );
    expect(passed.stdout).not.toContain('No gated metric regressed.');
    expect(passed.status).toBe(0);

    const headFailed = setup(['head drag.bench 1 fail']);
    expect(rounds(headFailed, ['preview.bench', 'drag.bench']).status).toBe(0);
    const failed = compare(headFailed);
    expect(failed.stdout).toContain('Failed on the head: `drag.bench`.');
    expect(failed.status).toBe(1);

    const regressed = setup();
    expect(rounds(regressed, ['preview.bench'], { FAKE_HEAD_MEDIAN: '120' }).status).toBe(0);
    const slower = compare(regressed);
    expect(slower.stdout).toContain('Regressed: `webview.preview`.');
    expect(slower.status).toBe(1);

    const nothing = setup(['base preview.bench * skip']);
    expect(rounds(nothing, ['preview.bench']).status).toBe(0);
    const empty = compare(nothing);
    expect(empty.stdout).toContain('No benchmark was compared.');
    expect(empty.stdout).toContain('Not conclusive: `preview.bench` was not compared');
    expect(empty.status).toBe(0);
  });

  it('refuses an out folder in use, bad names and missing apps before running anything', () => {
    const s = setup();
    mkdirSync(s.out);
    writeFileSync(path.join(s.out, 'old.json'), '{}');
    const used = rounds(s, ['preview.bench']);
    expect(used.status).toBe(2);
    expect(used.stderr).toContain('must be new or empty');
    rmSync(s.out, { recursive: true });
    for (const bad of [['preview'], ['../x.bench'], ['preview.bench', 'preview.bench']]) {
      const refused = rounds(s, bad);
      expect(refused.status).toBe(2);
    }
    rmSync(s.base);
    expect(rounds(s, ['preview.bench']).stderr).toContain('The app under test is missing');
    expect(calls(s)).toEqual([]);
  });
});
