/**
 * The blocks the selection benchmarks click (targets.ts): they exist in their documents, are of
 * the kinds the benchmarks name, and every click (also from the last block back to the first)
 * moves to a block that sees other variables, so the toolbox's listing changes each time. What
 * each target says the toolbox lists is what the block sees, and tells its listing from the one
 * of the target before (so a click after which the toolbox was not rebuilt is caught).
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

import { type BlockNode, parseDocument } from '../support/bdm';
import { REPOSITORY_ROOT } from '../support/env';
import { generateDocument } from './document';
import { listingMismatch } from './interaction';
import {
  type ClickTarget,
  counterSymbol,
  GUESSING_MAIN_ID,
  GUESSING_TARGETS,
  SCOPES,
  selectionTargets,
  targetAt,
  unitBlockId,
  valueSymbol,
} from './targets';

/** The symbol a symbol field declares (`{ sym, name }`), or `null`. */
function declaredSymbol(block: BlockNode, field: string): string | null {
  const value = block.fields?.[field];
  return typeof value === 'object' && value !== null && 'sym' in value ? String(value.sym) : null;
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
 * The symbols of the variables the toolbox lists for a selected block, as the editor's listing
 * point puts it (src/editor/toolbox/scope.ts): just after a selected statement (its own variable
 * included), at a selected value block (what its statement sees); a loop's counter only inside the
 * loop.
 */
function visibleAt(
  list: readonly BlockNode[],
  id: string,
  outer: readonly string[],
): string[] | null {
  const declared = [...outer];
  for (const statement of list) {
    const own = statement.type === 'var.declare' ? declaredSymbol(statement, 'NAME') : null;
    if (statement.id === id) {
      return own === null ? declared : [...declared, own];
    }
    if (holdsValue(statement, id)) {
      return declared;
    }
    const counter =
      statement.type === 'control.for_range' ? declaredSymbol(statement, 'VAR') : null;
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
function steps(targets: readonly ClickTarget[]): [ClickTarget, ClickTarget][] {
  return targets.map((target, index) => [target, targetAt(targets, index + 1)]);
}

/** What the toolbox lists for each target, which must be what its block sees. */
function checkListings(body: readonly BlockNode[], targets: readonly ClickTarget[]): void {
  for (const target of targets) {
    const visible = new Set(visibleAt(body, target.id, []) ?? []);
    expect(listingMismatch(target, visible), target.id).toBeNull();
  }
  for (const [from, to] of steps(targets)) {
    // The listing for the block before, kept by a toolbox that was not rebuilt, is caught.
    const before = new Set(visibleAt(body, from.id, []) ?? []);
    expect(listingMismatch(to, before), `${from.id} to ${to.id}`).not.toBeNull();
  }
}

describe('selectionTargets', () => {
  const body = mainBody(
    JSON.stringify(generateDocument({ blocks: 1_000, dragHandle: false })),
    'b0',
  );
  const targets = selectionTargets(1_000);

  it('names 8 blocks in units spread over the document, the two changes in the loop', () => {
    expect(targets).toHaveLength(SCOPES);
    expect(targets.map(({ id }) => typeOf(body, id))).toEqual([
      'var.change',
      'io.print',
      'var.change',
      'control.for_range',
      'control.if',
      'var.get',
      'var.declare',
      'math.arithmetic',
    ]);
    expect(targets[0]?.id).toBe(unitBlockId(0, 'change'));
    expect(targets.at(-1)?.id).toBe(unitBlockId(123, 'sum'));
  });

  it('moves to another scope with every click, round the list', () => {
    const scopes = targets.map(({ id }) => visibleAt(body, id, []));
    expect(scopes.every((scope) => scope !== null)).toBe(true);
    expect(new Set(scopes.map((scope) => JSON.stringify(scope))).size).toBe(SCOPES);
    for (const [from, to] of steps(targets)) {
      expect(visibleAt(body, from.id, []), `${from.id} to ${to.id}`).not.toEqual(
        visibleAt(body, to.id, []),
      );
    }
  });

  it('says what the toolbox lists for each, which tells it from the click before', () => {
    expect(targets[0]).toEqual({
      id: unitBlockId(0, 'change'),
      lists: [valueSymbol(0), counterSymbol(0)],
      omits: [valueSymbol(1)],
    });
    expect(targets.at(-1)).toEqual({
      id: unitBlockId(123, 'sum'),
      lists: [valueSymbol(123)],
      omits: [counterSymbol(123)],
    });
    checkListings(body, targets);
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
    expect(GUESSING_TARGETS.map(({ id }) => typeOf(body, id))).toEqual([
      'math.random_int',
      'io.print',
      'var.declare',
      'io.print',
    ]);
    expect(visibleAt(body, 'b001', [])).toEqual([]);
    expect(visibleAt(body, 'b002', [])).toEqual(['s_secret']);
    expect(visibleAt(body, 'b006', [])).toEqual(['s_secret', 's_guess']);
    for (const [from, to] of steps(GUESSING_TARGETS)) {
      expect(visibleAt(body, from.id, []), `${from.id} to ${to.id}`).not.toEqual(
        visibleAt(body, to.id, []),
      );
    }
  });

  it('say what the toolbox lists for each, which tells it from the click before', () => {
    checkListings(body, GUESSING_TARGETS);
  });
});

describe('targetAt', () => {
  it('goes round the list from any index', () => {
    const targets = selectionTargets(1_000);
    expect(targetAt(targets, 0)).toBe(targets[0]);
    expect(targetAt(targets, SCOPES + 2)).toBe(targets[2]);
    // The A/B job's second round of 5 starts at 5: 5, 6, 7, 0, 1, as a nightly run's last ones.
    expect([5, 6, 7, 8, 9].map((index) => targetAt(targets, index).id)).toEqual(
      [5, 6, 7, 0, 1].map((index) => targets[index]?.id),
    );
    expect(() => targetAt([], 0)).toThrow('No target 0');
  });
});
