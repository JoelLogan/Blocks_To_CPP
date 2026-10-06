/**
 * The generated benchmark document (docs/spec/09-quality-and-delivery.md §9.2 "Benchmarks"): one
 * module whose `program.main` holds repeated units of ordinary blocks.
 *
 * The same generator exists in Rust for the native benchmark
 * (crates/b2c-core-wasm/benches/pipeline/document.rs), so the native and the webview numbers are
 * about the same programs. Both check {@link parityDigest} against the same constants
 * (document.test.ts here, benches/pipeline/tests.rs there): a change made to one generator and
 * not the other fails a test.
 *
 * Shape:
 * - `program.main` (`b0`, at x 40, y 40) holds `units` units, then `fillers` prints, where
 *   `units = (main − 1) / 8` and `fillers = (main − 1) % 8`, `main` being the requested block count
 *   (minus the two blocks of the drag handle, when asked for).
 * - Unit `u` is 8 blocks: `int value_u = u;`, then `for (int i_u = 0; i_u < 10; …)` holding an
 *   `if (i_u % 2 == 0)` whose branches change `value_u` by `i_u` and subtract 1 from it, then
 *   `print "value u: ", value_u + 1` (a `math.arithmetic` with a `var.get` in it).
 * - A filler `f` is `print "filler f"`.
 * - The drag handle is a separate `func.define dragMe` at x 800, y 40 holding one print: a
 *   top-level block the drag benchmark moves back and forth without changing the program.
 *
 * Block IDs are `b<n>` in document order; symbol IDs `s_value_<u>`, `s_i_<u>` and `s_drag`.
 */
import { createHash } from 'node:crypto';
import { writeFileSync } from 'node:fs';
import path from 'node:path';

/** Blocks per unit. */
export const UNIT_BLOCKS = 8;

/** Blocks of the drag handle (`func.define` and its print). */
export const HANDLE_BLOCKS = 2;

/** The most blocks a generated document may have: the project limit (05 §5.6). */
export const MAX_BLOCKS = 100_000;

/** The ID of the drag handle's `func.define` in a document of `blocks` blocks. */
export function handleId(blocks: number): string {
  return `b${String(blocks - HANDLE_BLOCKS)}`;
}

/** The ID of `program.main`. */
export const MAIN_ID = 'b0';

/** What to generate. */
export interface Shape {
  /** How many blocks the document has in all. */
  readonly blocks: number;
  /** Whether it has the drag handle. */
  readonly dragHandle: boolean;
}

/** A JSON value as the generator builds it. */
export type Json =
  null | boolean | number | string | readonly Json[] | { readonly [key: string]: Json };

/** A shape that cannot be generated. */
export class ShapeError extends Error {
  override readonly name = 'ShapeError';
}

/** The fewest blocks a shape can have (`program.main`, plus the handle when asked for). */
export function minBlocks(shape: Pick<Shape, 'dragHandle'>): number {
  return shape.dragHandle ? 1 + HANDLE_BLOCKS : 1;
}

/** Hands out block IDs in document order. */
class Ids {
  #next = 0;

  next(): string {
    const id = `b${String(this.#next)}`;
    this.#next += 1;
    return id;
  }
}

function num(value: number): Json {
  return { expr: [{ num: String(value) }] };
}

function print(ids: Ids, text: string): Json {
  return {
    id: ids.next(),
    type: 'io.print',
    v: 1,
    extra: { itemCount: 1 },
    fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
    inputs: { ITEM0: { expr: [{ str: text }] } },
  };
}

/** Unit `u`: 8 blocks in three statements of `main`. IDs are handed out in document order. */
function unit(ids: Ids, u: number): Json[] {
  const value = `s_value_${String(u)}`;
  const counter = `s_i_${String(u)}`;
  const declare: Json = {
    id: ids.next(),
    type: 'var.declare',
    v: 1,
    fields: { CONST: false, NAME: { sym: value, name: `value_${String(u)}` }, TYPE: 'int' },
    inputs: { VALUE: num(u) },
  };
  const loopId = ids.next();
  const ifId = ids.next();
  const change: Json = {
    id: ids.next(),
    type: 'var.change',
    v: 1,
    fields: { VAR: { ref: value } },
    inputs: { BY: { expr: [{ ref: counter }] } },
  };
  const update: Json = {
    id: ids.next(),
    type: 'var.update',
    v: 1,
    fields: { OP: 'sub', VAR: { ref: value } },
    inputs: { VALUE: num(1) },
  };
  const branch: Json = {
    id: ifId,
    type: 'control.if',
    v: 1,
    extra: { elseIfCount: 0, hasElse: true },
    inputs: {
      COND0: {
        expr: [{ ref: counter }, { op: '%' }, { num: '2' }, { op: '==' }, { num: '0' }],
      },
    },
    statements: { DO0: [change], ELSE: [update] },
  };
  const counted: Json = {
    id: loopId,
    type: 'control.for_range',
    v: 1,
    fields: { DIRECTION: 'to', VAR: { sym: counter, name: `i_${String(u)}` } },
    inputs: { FROM: num(0), TO: num(10) },
    statements: { BODY: [branch] },
  };
  const printId = ids.next();
  const sumId = ids.next();
  const get: Json = { id: ids.next(), type: 'var.get', v: 1, fields: { VAR: { ref: value } } };
  const report: Json = {
    id: printId,
    type: 'io.print',
    v: 1,
    extra: { itemCount: 2 },
    fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
    inputs: {
      ITEM0: { expr: [{ str: `value ${String(u)}: ` }] },
      ITEM1: {
        block: {
          id: sumId,
          type: 'math.arithmetic',
          v: 1,
          fields: { OP: 'add' },
          inputs: { A: { block: get }, B: num(1) },
        },
      },
    },
  };
  return [declare, counted, report];
}

/** The project settings every generated document has (those of the examples). */
function project(blocks: number): Json {
  return {
    id: `prj_bench_${String(blocks)}`,
    name: `Benchmark ${String(blocks)} blocks`,
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

/**
 * The benchmark document of `shape` (see the module comment).
 *
 * @throws ShapeError when the count is not a whole number, or too small or too large.
 */
export function generateDocument(shape: Shape): Json {
  const min = minBlocks(shape);
  if (!Number.isSafeInteger(shape.blocks) || shape.blocks < min) {
    throw new ShapeError(
      `A benchmark document needs at least ${String(min)} blocks, not ${String(shape.blocks)}`,
    );
  }
  if (shape.blocks > MAX_BLOCKS) {
    throw new ShapeError(
      `A benchmark document may have at most ${String(MAX_BLOCKS)} blocks, not ${String(shape.blocks)}`,
    );
  }
  const inMain = shape.blocks - (shape.dragHandle ? HANDLE_BLOCKS : 0);
  const units = Math.floor((inMain - 1) / UNIT_BLOCKS);
  const fillers = (inMain - 1) % UNIT_BLOCKS;

  const ids = new Ids();
  const mainId = ids.next();
  const body: Json[] = [];
  for (let index = 0; index < units; index += 1) {
    body.push(...unit(ids, index));
  }
  for (let index = 0; index < fillers; index += 1) {
    body.push(print(ids, `filler ${String(index)}`));
  }
  const top: Json[] = [
    { id: mainId, type: 'program.main', v: 1, x: 40, y: 40, statements: { BODY: body } },
  ];
  if (shape.dragHandle) {
    const defineId = ids.next();
    top.push({
      id: defineId,
      type: 'func.define',
      v: 1,
      x: 800,
      y: 40,
      extra: { params: [] },
      fields: { NAME: { sym: 's_drag', name: 'dragMe' }, RETURNS: 'void' },
      statements: { BODY: [print(ids, 'drag me')] },
    });
  }
  return {
    format: 'blocks2cpp/project',
    formatVersion: 1,
    generator: { app: '0.1.0', catalog: '1.0.0' },
    project: project(shape.blocks),
    modules: [{ id: 'mod_main', name: 'main', workspace: { blocks: top } }],
  };
}

/** Whether a JSON value is an object (not an array or null). */
function isObject(value: Json | undefined): value is Readonly<Record<string, Json>> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

/** Compact JSON with every object's keys in order (code-unit order, the same as byte order here). */
export function sortedJson(value: Json): string {
  if (Array.isArray(value)) {
    return `[${(value as readonly Json[]).map(sortedJson).join(',')}]`;
  }
  if (isObject(value)) {
    const keys = Object.keys(value).sort();
    return `{${keys.map((key) => `${JSON.stringify(key)}:${sortedJson(value[key] ?? null)}`).join(',')}}`;
  }
  return JSON.stringify(value);
}

/**
 * The SHA-256 (64 lower-case hex digits) of the document as compact JSON with sorted keys: the
 * Rust generator gives the same digest for the same shape.
 */
export function parityDigest(document: Json): string {
  return createHash('sha256').update(sortedJson(document), 'utf8').digest('hex');
}

/**
 * How many blocks a document has, counted independently of the generator: every block node at any
 * depth (top-level blocks, statement lists, value inputs and stacks).
 */
export function countBlocks(document: Json): number {
  const pending: Json[] = [];
  const modules = isObject(document) ? document['modules'] : undefined;
  if (Array.isArray(modules)) {
    for (const module of modules as readonly Json[]) {
      const workspace = isObject(module) ? module['workspace'] : undefined;
      const blocks = isObject(workspace) ? workspace['blocks'] : undefined;
      if (Array.isArray(blocks)) {
        pending.push(...(blocks as readonly Json[]));
      }
    }
  }
  let count = 0;
  for (let block = pending.pop(); block !== undefined; block = pending.pop()) {
    if (!isObject(block)) {
      continue;
    }
    count += 1;
    const statements = block['statements'];
    if (isObject(statements)) {
      for (const list of Object.values(statements)) {
        if (Array.isArray(list)) {
          pending.push(...(list as readonly Json[]));
        }
      }
    }
    const inputs = block['inputs'];
    if (isObject(inputs)) {
      for (const input of Object.values(inputs)) {
        if (isObject(input) && input['block'] !== undefined) {
          pending.push(input['block']);
        }
      }
    }
    const stack = block['stack'];
    if (Array.isArray(stack)) {
      pending.push(...(stack as readonly Json[]));
    }
  }
  return count;
}

/**
 * Writes the benchmark document of `shape` as a project file in `folder` and returns its path
 * (`bench-<blocks>[-drag].b2c`). The app opens it through the scripted open dialog.
 */
export function writeBenchProject(folder: string, shape: Shape): string {
  const name = `bench-${String(shape.blocks)}${shape.dragHandle ? '-drag' : ''}.b2c`;
  const file = path.join(folder, name);
  writeFileSync(file, `${JSON.stringify(generateDocument(shape), null, 2)}\n`);
  return file;
}
