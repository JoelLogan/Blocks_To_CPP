/**
 * The document of the typed-slots benchmarks (typed-slots.bench.e2e.ts, docs/plans/m3-contracts/
 * slots.md §8): a 1,000-block module in which a number block is dragged, or moved with the
 * keyboard, past many typed slots.
 *
 * Shape:
 * - `program.main` (`b0`, at x 40, y 40) holds {@link TYPED_CREATES} `create` blocks of
 *   alternating types, `int t_k = 0` for even `k` and `string t_k = ""` for odd `k` (each a typed
 *   value input, half of which a number fits), then prints (`print "filler f"`) up to the block
 *   count;
 * - the moved block is a loose `math.number` (`1`) at x {@link NUMBER_X}, y {@link NUMBER_Y},
 *   right of the column's top, far enough from it not to connect where it lies.
 *
 * Every block is one that every block catalog since 1.0.0 reads the same way (README.md, "A/B
 * runs": the base's build opens the head's documents too). Block IDs are `b<n>` in document order,
 * the number last; symbol IDs `s_t_<k>`. It has no twin in the native benchmark, whose generated
 * module gains W2's blocks with W2 (slots.md §8).
 */
import { writeFileSync } from 'node:fs';
import path from 'node:path';

import type { E2ePoint } from '../../src/e2e/contract';
import type { Json } from './document';

/** The workspace size. */
export const TYPED_BLOCKS = 1_000;

/** The `create` blocks of the column. */
export const TYPED_CREATES = 200;

/** Where the moved number block lies, in workspace units. */
export const NUMBER_X = 600;
export const NUMBER_Y = 240;

/** The ID of `program.main`. */
export const TYPED_MAIN_ID = 'b0';

/** The ID of the moved number block. */
export const NUMBER_ID = `b${String(TYPED_BLOCKS - 1)}`;

/** The ID of the `k`-th `create` of the column (from 0). */
export function createId(k: number): string {
  return `b${String(k + 1)}`;
}

/** The project settings (those of the other generated documents). */
function project(): Json {
  return {
    id: 'prj_bench_typed_1000',
    name: 'Benchmark typed slots',
    language: { standard: 'c++20' },
    options: {
      showAdvanced: false,
      manualMemory: false,
      preferPlainStd: false,
      formattingStyle: 'stream',
      checkedIndexing: true,
    },
    build: {
      configurations: {
        debug: {
          optimization: 'none',
          debugInfo: true,
          sanitizers: ['address', 'undefined'],
          warnings: 'helpful',
          hardening: true,
        },
        release: {
          optimization: 'speed',
          debugInfo: false,
          sanitizers: [],
          warnings: 'helpful',
          hardening: true,
        },
      },
    },
    run: { workingDirectory: 'project' },
  };
}

/** The benchmark document (see the module comment). */
export function generateTypedDocument(): Json {
  const body: Json[] = [];
  for (let k = 0; k < TYPED_CREATES; k += 1) {
    const whole = k % 2 === 0;
    body.push({
      id: createId(k),
      type: 'var.declare',
      v: 1,
      fields: {
        CONST: false,
        NAME: { sym: `s_t_${String(k)}`, name: `t_${String(k)}` },
        TYPE: whole ? 'int' : 'std::string',
      },
      inputs: { VALUE: { expr: [whole ? { num: '0' } : { str: '' }] } },
    });
  }
  const fillers = TYPED_BLOCKS - 2 - TYPED_CREATES;
  for (let f = 0; f < fillers; f += 1) {
    body.push({
      id: `b${String(TYPED_CREATES + 1 + f)}`,
      type: 'io.print',
      v: 1,
      extra: { itemCount: 1 },
      fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
      inputs: { ITEM0: { expr: [{ str: `filler ${String(f)}` }] } },
    });
  }
  return {
    format: 'blocks2cpp/project',
    formatVersion: 1,
    generator: { app: '0.1.0', catalog: '1.0.0' },
    project: project(),
    modules: [
      {
        id: 'mod_main',
        name: 'main',
        workspace: {
          blocks: [
            {
              id: TYPED_MAIN_ID,
              type: 'program.main',
              v: 1,
              x: 40,
              y: 40,
              statements: { BODY: body },
            },
            {
              id: NUMBER_ID,
              type: 'math.number',
              v: 1,
              x: NUMBER_X,
              y: NUMBER_Y,
              fields: { VALUE: '1' },
            },
          ],
        },
      },
    ],
  };
}

/** Writes the document as a project file in `folder` and returns its path. */
export function writeTypedProject(folder: string): string {
  const file = path.join(folder, `bench-typed-${String(TYPED_BLOCKS)}.b2c`);
  writeFileSync(file, `${JSON.stringify(generateTypedDocument(), null, 2)}\n`);
  return file;
}

/**
 * The pointer's path for one drag down the column and back (typed-slots.bench.e2e.ts): from
 * `grab`, where the number block was pressed, through each point where its output connection (at
 * `own` before the drag) is on one of `targets` (the column's value inputs on screen, taken top to
 * bottom), and back to `grab`, where the block is let go where it lay. `steps` pointer moves in
 * all, shared by the path's segments by their lengths (at least one each); the last is `grab`.
 */
export function columnPath(
  grab: E2ePoint,
  own: E2ePoint,
  targets: readonly E2ePoint[],
  steps: number,
): E2ePoint[] {
  const offset = { x: grab.x - own.x, y: grab.y - own.y };
  const waypoints = [...targets]
    .sort((a, b) => a.y - b.y)
    .map((target) => ({ x: target.x + offset.x, y: target.y + offset.y }));
  const route = [grab, ...waypoints, grab];
  const lengths = route
    .slice(1)
    .map((point, index) =>
      Math.hypot(point.x - (route[index]?.x ?? 0), point.y - (route[index]?.y ?? 0)),
    );
  const total = lengths.reduce((sum, length) => sum + length, 0);
  const path: E2ePoint[] = [];
  lengths.forEach((length, index) => {
    const from = route[index] ?? grab;
    const to = route[index + 1] ?? grab;
    const moves = Math.max(1, Math.round(total === 0 ? 1 : (steps * length) / total));
    for (let move = 1; move <= moves; move += 1) {
      path.push({
        x: from.x + ((to.x - from.x) * move) / moves,
        y: from.y + ((to.y - from.y) * move) / moves,
      });
    }
  });
  return path;
}
