/**
 * The rows of the Problems panel (docs/spec/04-user-interface.md §4.4): the live preview's
 * diagnostics merged with the last build's, each with its module and block path.
 */
import type { BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import type { Diagnostic, Part } from '@blocks2cpp/ipc-types';

import { DEFAULT_PROBLEM_SORT, sortProblems } from '../../panels/problems/problems';
import type { ProblemItem } from '../../panels';
import { blockPath, type DocumentIndex, indexDocument, moduleOf } from './blockPath';
import { type PathCatalog, pathCatalog } from './catalog';

/**
 * The most rows built; anything beyond is left out. The pipeline already bounds its diagnostics
 * (1,000 loader problems, 10,000 catalog problems), and Problems shows at most 1,000 rows.
 */
export const MAX_PROBLEM_ITEMS = 20_000;

/** A stable text for a part, for the rows' keys. */
function partKey(part: Part): string {
  switch (part.kind) {
    case 'whole':
      return 'whole';
    case 'field':
    case 'input':
      return `${part.kind}:${part.name}`;
    case 'tokens':
      return `tokens:${part.input}:${String(part.start)}:${String(part.end)}`;
    default:
      return '';
  }
}

/** The module's name for display: the diagnostic's module, else the module its block is in. */
function modulePath(doc: BdmDocument, index: DocumentIndex, diagnostic: Diagnostic): string {
  const { module, block } = diagnostic.primary;
  if (module !== undefined) {
    const named = doc.modules.find((candidate) => candidate.id === module);
    if (named !== undefined) {
      return named.name;
    }
  }
  return block === undefined ? '' : (moduleOf(index, block)?.name ?? '');
}

/**
 * The Problems rows for the live diagnostics (`live`, the preview's) and the last build's (`build`:
 * only what a build alone finds, see `diagnosticInputs`), sorted the panel's default way (most
 * serious first).
 *
 * - A build diagnostic whose block is no longer in `doc` is dropped (the block was deleted).
 * - Build diagnostics are `stale` (dimmed, "from the last build") when `stale` is true.
 * - Module: the diagnostic's module, or the module its block is in; empty without either.
 * - Block path: `main › repeat until › if` (see ./blockPath.ts); empty without a block.
 * - Keys are stable while the diagnostics stay the same, and unique.
 *
 * `catalog` names the blocks of the paths; it defaults to the one the editor provided
 * (./catalog.ts).
 */
export function buildProblemItems(
  live: readonly Diagnostic[],
  build: readonly Diagnostic[],
  stale: boolean,
  doc: BdmDocument,
  catalog: PathCatalog | null = pathCatalog(),
): ProblemItem[] {
  const index = indexDocument(doc);
  const items: ProblemItem[] = [];
  const seen = new Map<string, number>();
  const paths = new Map<string, string>();
  const pathOf = (block: string): string => {
    let path = paths.get(block);
    if (path === undefined) {
      path = blockPath(index, block, catalog);
      paths.set(block, path);
    }
    return path;
  };

  const add = (diagnostic: Diagnostic, origin: ProblemItem['origin']): void => {
    if (items.length >= MAX_PROBLEM_ITEMS) {
      return;
    }
    const block = diagnostic.primary.block;
    if (origin === 'build' && block !== undefined && !index.blocks.has(block)) {
      return;
    }
    const signature = [
      origin,
      diagnostic.code,
      diagnostic.primary.module ?? '',
      block ?? '',
      partKey(diagnostic.primary.part),
    ].join('|');
    const occurrence = seen.get(signature) ?? 0;
    seen.set(signature, occurrence + 1);
    items.push({
      key: `${signature}#${String(occurrence)}`,
      diagnostic,
      origin,
      stale: origin === 'build' && stale,
      modulePath: modulePath(doc, index, diagnostic),
      blockPath: block === undefined ? '' : pathOf(block),
    });
  };

  for (const diagnostic of live) {
    add(diagnostic, 'live');
  }
  for (const diagnostic of build) {
    add(diagnostic, 'build');
  }
  return sortProblems(items, DEFAULT_PROBLEM_SORT);
}
