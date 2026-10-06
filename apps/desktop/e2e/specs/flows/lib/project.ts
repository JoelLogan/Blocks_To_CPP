/**
 * Projects in the flow tests: creating one from the Empty template, opening and saving files
 * through the scripted dialogs, the fixture and example files, comparing documents, and the
 * block nodes the tests insert (docs/spec/05-project-format.md §5.4) beyond those of
 * ../../../support/bdm.ts. ./project.test.ts checks the builders against the catalog.
 */
import { copyFileSync, readFileSync, statSync, type Stats } from 'node:fs';
import path from 'node:path';

import type { App } from '../../../support/app';
import {
  type BlockInput,
  type BlockNode,
  type ExprInput,
  type NodeIds,
  onlyTopBlock,
  type ProjectDocument,
  statementsOf,
} from '../../../support/bdm';
import { currentDocument } from '../../../support/editor';
import { REPOSITORY_ROOT } from '../../../support/env';
import { clickTestId } from '../../../support/ui';
import { waitFor } from '../../../support/wait';

/** The flow tests' own project files. */
export const FIXTURES = path.resolve(import.meta.dirname, '../../../fixtures/flows');

/** The repository's examples (examples/*.b2c). */
export const EXAMPLES = path.join(REPOSITORY_ROOT, 'examples');

/** Copies `examples/<name>.b2c` to `folder` (as a new path, so no trust record matches it). */
export function copyExample(name: string, folder: string, as = `${name}.b2c`): string {
  const target = path.join(folder, as);
  copyFileSync(path.join(EXAMPLES, `${name}.b2c`), target);
  return target;
}

/** Copies the fixture `fixtures/flows/<name>` to `folder`. */
export function copyFixture(name: string, folder: string): string {
  const target = path.join(folder, name);
  copyFileSync(path.join(FIXTURES, name), target);
  return target;
}

/** Waits until the editor shows a project (a module with blocks), and returns its document. */
export function waitForProject(app: App, timeout = 15_000): Promise<ProjectDocument> {
  return waitFor(
    async () => {
      const found = await currentDocument(app);
      return (found.modules[0]?.workspace.blocks.length ?? 0) > 0 ? found : null;
    },
    { timeout, message: 'a project in the editor' },
  );
}

/** Creates a project from the Empty template on the start page; returns `program.main`'s ID. */
export async function newEmptyProject(app: App): Promise<string> {
  await clickTestId(app.driver, 'template-empty');
  return onlyTopBlock(await waitForProject(app), 'program.main').id;
}

/** Opens a project from the start page: its *Open…* button, answered by the dialog script. */
export async function openFromStartPage(app: App): Promise<ProjectDocument> {
  await clickTestId(app.driver, 'start-open');
  return waitForProject(app);
}

/** The blocks of `main`'s body, from the canvas. */
export async function mainBody(app: App): Promise<readonly BlockNode[]> {
  return statementsOf(onlyTopBlock(await currentDocument(app), 'program.main'), 'BODY');
}

/** The blocks of every module (what "the same blocks" compares: no viewport, no metadata). */
export function moduleBlocks(doc: ProjectDocument): unknown[] {
  return doc.modules.map((module) => ({
    id: module.id,
    name: module.name,
    blocks: module.workspace.blocks,
  }));
}

/** A file's identity and time, to tell when it was written again (an atomic save replaces it). */
export function fileStamp(file: string): string {
  const stats: Stats = statSync(file, { bigint: false });
  return `${String(stats.ino)}:${String(stats.mtimeMs)}:${String(stats.size)}`;
}

/** Waits until `file` exists and its stamp differs from `before` (`null`: it did not exist). */
export function waitForWrite(
  file: string,
  before: string | null,
  timeout = 15_000,
): Promise<Buffer> {
  return waitFor(
    () => {
      let stamp: string;
      try {
        stamp = fileStamp(file);
      } catch {
        return null;
      }
      return stamp === before ? null : readFileSync(file);
    },
    { timeout, message: `${file} to be written` },
  );
}

/** Reads a project file as JSON. */
export function readProjectFile(file: string): ProjectDocument {
  return JSON.parse(readFileSync(file, 'utf8')) as ProjectDocument;
}

// ---- Block nodes ---------------------------------------------------------------------------------

/** An expression input that names a variable. */
export function ref(sym: string): ExprInput {
  return { expr: [{ ref: sym }] };
}

/** `control.forever`. */
export function forever(ids: NodeIds, body: readonly BlockNode[]): BlockNode {
  return { id: ids.next(), type: 'control.forever', v: 1, statements: { BODY: body } };
}

/** `control.repeat`: `repeat TIMES times`. */
export function repeat(ids: NodeIds, times: ExprInput, body: readonly BlockNode[]): BlockNode {
  return {
    id: ids.next(),
    type: 'control.repeat',
    v: 1,
    inputs: { TIMES: times },
    statements: { BODY: body },
  };
}

/** `var.change`: `change VAR by BY`. */
export function changeBy(ids: NodeIds, sym: string, by: ExprInput): BlockNode {
  return {
    id: ids.next(),
    type: 'var.change',
    v: 1,
    fields: { VAR: { ref: sym } },
    inputs: { BY: by },
  };
}

/** `io.print` of one value (an expression or a reporter block), then a new line. */
export function printValue(ids: NodeIds, item: ExprInput | BlockInput): BlockNode {
  return {
    id: ids.next(),
    type: 'io.print',
    v: 1,
    extra: { itemCount: 1 },
    fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
    inputs: { ITEM0: item },
  };
}

/** The operators of `math.arithmetic`. */
export type ArithmeticOp = 'add' | 'sub' | 'mul' | 'div' | 'mod';

/** `math.arithmetic`: `A OP B`. */
export function arithmetic(
  ids: NodeIds,
  op: ArithmeticOp,
  a: ExprInput | BlockInput,
  b: ExprInput | BlockInput,
): BlockNode {
  return {
    id: ids.next(),
    type: 'math.arithmetic',
    v: 1,
    fields: { OP: op },
    inputs: { A: a, B: b },
  };
}

/** `var.declare` of any type: `TYPE name = VALUE;` (or `const`), declaring `sym`. */
export function declare(
  ids: NodeIds,
  decl: {
    readonly sym: string;
    readonly name: string;
    readonly type: string;
    readonly value: ExprInput | BlockInput;
    readonly isConst?: boolean;
  },
): BlockNode {
  return {
    id: ids.next(),
    type: 'var.declare',
    v: 1,
    fields: {
      CONST: decl.isConst === true,
      NAME: { sym: decl.sym, name: decl.name },
      TYPE: decl.type,
    },
    inputs: { VALUE: decl.value },
  };
}

/** A nested block as an input. */
export function block(node: BlockNode): BlockInput {
  return { block: node };
}
