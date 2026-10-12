/**
 * The blocks the selection benchmarks click (targets.ts): they exist in their documents, are of
 * the kinds the benchmarks name, and every click (also from the last block back to the first)
 * moves to a block that sees other variables, so the toolbox's listing changes each time.
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

import { type BlockNode, parseDocument } from '../support/bdm';
import { REPOSITORY_ROOT } from '../support/env';
import { generateDocument } from './document';
import {
  GUESSING_MAIN_ID,
  GUESSING_TARGETS,
  SCOPES,
  selectionTargets,
  unitBlockId,
} from './targets';

/** The name a symbol field declares (`{ sym, name }`), or `null`. */
function declaredName(block: BlockNode, field: string): string | null {
  const value = block.fields?.[field];
  return typeof value === 'object' && value !== null && 'name' in value ? String(value.name) : null;
}

/** The blocks in a block's value inputs. */
function valueBlocks(block: BlockNode): BlockNode[] {
  return Object.values(block.inputs ?? {}).flatMap((input) =>
    'block' in input ? [input.block] : [],
  );
}

/** Whether `id` is `block` or a value block inside it (not inside its statement lists). */
function holdsValue(block: BlockNode, id: string): boolean {
  return block.id === id || valueBlocks(block).some((inner) => holdsValue(inner, id));
}

/**
 * The variables the toolbox lists for a selected block, as the editor's listing point puts it
 * (src/editor/toolbox/scope.ts): just after a selected statement (its own variable included), at
 * a selected value block (what its statement sees); a loop's counter only inside the loop.
 */
function visibleAt(
  list: readonly BlockNode[],
  id: string,
  outer: readonly string[],
): string[] | null {
  const declared = [...outer];
  for (const statement of list) {
    const own = statement.type === 'var.declare' ? declaredName(statement, 'NAME') : null;
    if (statement.id === id) {
      return own === null ? declared : [...declared, own];
    }
    if (holdsValue(statement, id)) {
      return declared;
    }
    const counter = statement.type === 'control.for_range' ? declaredName(statement, 'VAR') : null;
    for (const inner of Object.values(statement.statements ?? {})) {
      const found = visibleAt(inner, id, counter === null ? declared : [...declared, counter]);
      if (found !== null) {
        return found;
      }
    }
    if (own !== null) {
      declared.push(own);
    }
  }
  return null;
}

/** The body of a document's `program.main`. */
function mainBody(text: string, mainId: string): readonly BlockNode[] {
  const document = parseDocument(text);
  const blocks = document.modules[0]?.workspace.blocks ?? [];
  const main = blocks.find((block) => block.id === mainId);
  if (main === undefined) {
    throw new Error(`No ${mainId} in the document`);
  }
  return main.statements?.['BODY'] ?? [];
}

/** The type of block `id` in `list`, at any depth. */
function typeOf(list: readonly BlockNode[], id: string): string | undefined {
  for (const block of list) {
    if (block.id === id) {
      return block.type;
    }
    const inValue = typeOf(valueBlocks(block), id);
    if (inValue !== undefined) {
      return inValue;
    }
    for (const inner of Object.values(block.statements ?? {})) {
      const found = typeOf(inner, id);
      if (found !== undefined) {
        return found;
      }
    }
  }
  return undefined;
}

/** Each pair of consecutive targets, the last with the first. */
function steps(targets: readonly string[]): [string, string][] {
  return targets.map((id, index) => [id, targets[(index + 1) % targets.length] ?? id]);
}

describe('selectionTargets', () => {
  const body = mainBody(
    JSON.stringify(generateDocument({ blocks: 1_000, dragHandle: false })),
    'b0',
  );
  const targets = selectionTargets(1_000);

  it('names 8 blocks of different kinds in units spread over the document', () => {
    expect(targets).toHaveLength(SCOPES);
    expect(targets.map((id) => typeOf(body, id))).toEqual([
      'var.change',
      'io.print',
      'var.update',
      'control.for_range',
      'control.if',
      'var.get',
      'var.declare',
      'math.arithmetic',
    ]);
    expect(targets[0]).toBe(unitBlockId(0, 'change'));
    expect(targets.at(-1)).toBe(unitBlockId(123, 'sum'));
  });

  it('moves to another scope with every click, round the list', () => {
    const scopes = targets.map((id) => visibleAt(body, id, []));
    expect(scopes.every((scope) => scope !== null)).toBe(true);
    expect(new Set(scopes.map((scope) => JSON.stringify(scope))).size).toBe(SCOPES);
    for (const [from, to] of steps(targets)) {
      expect(visibleAt(body, from, []), `${from} to ${to}`).not.toEqual(visibleAt(body, to, []));
    }
  });

  it('needs a document of at least 8 units', () => {
    expect(() => selectionTargets(8 * 7 + 1)).toThrow('fewer than 8 units');
    expect(selectionTargets(8 * 8 + 1)).toHaveLength(SCOPES);
  });
});

describe('GUESSING_TARGETS', () => {
  const body = mainBody(
    readFileSync(path.join(REPOSITORY_ROOT, 'examples', 'guessing_game.b2c'), 'utf8'),
    GUESSING_MAIN_ID,
  );

  it('are blocks of the guessing game, each click into another scope', () => {
    expect(GUESSING_TARGETS.map((id) => typeOf(body, id))).toEqual([
      'math.random_int',
      'io.print',
      'var.declare',
      'io.print',
    ]);
    expect(visibleAt(body, 'b001', [])).toEqual([]);
    expect(visibleAt(body, 'b002', [])).toEqual(['secret']);
    expect(visibleAt(body, 'b006', [])).toEqual(['secret', 'guess']);
    for (const [from, to] of steps(GUESSING_TARGETS)) {
      expect(visibleAt(body, from, []), `${from} to ${to}`).not.toEqual(visibleAt(body, to, []));
    }
  });
});
