/**
 * The limits of a project (docs/spec/05-project-format.md §5.6) that a person can reach by
 * editing, as the compiler core's loader reports them, and what to say about them. When the
 * editor's own document breaks one of these, the person went past a limit, which they can undo;
 * any other loader problem with the editor's document is a bug in Blocks2Cpp.
 *
 * Used by Save (./lifecycle.ts) and by the banner shown while the canvas does not load
 * (src/features/analysis).
 */
import { MAX_DOCUMENT_BYTES } from '@blocks2cpp/b2c-core-wasm';

/** A limit a person can reach by editing. */
export type ReachableLimit = 'nesting' | 'size' | 'blocks';

/** The loader's code for each limit (docs/reference/diagnostics/loader-and-catalog.md). */
export const LIMIT_CODES: Readonly<Record<ReachableLimit, string>> = {
  nesting: 'B2C-E0104',
  size: 'B2C-E0101',
  blocks: 'B2C-E0121',
};

/** The largest project file, in MiB, for messages. */
const MAX_MIB = MAX_DOCUMENT_BYTES / (1024 * 1024);

/** What went past the limit: one sentence, naming its code. */
const LIMIT_SENTENCES: Readonly<Record<ReachableLimit, string>> = {
  nesting: `Blocks are nested more deeply than a project allows (${LIMIT_CODES.nesting}).`,
  size: `The project file would be larger than ${String(MAX_MIB)} MiB, the most a project file can be (${LIMIT_CODES.size}).`,
  blocks: `The project would have more than 100,000 blocks, the most a project can have (${LIMIT_CODES.blocks}).`,
};

/** How to get back within the limit. */
const REMEDIES: Readonly<Record<ReachableLimit, string>> = {
  nesting: 'Undo the last change (Ctrl+Z), or move some of the innermost blocks into a function.',
  size: 'Undo the last change (Ctrl+Z), or split the program into several projects.',
  blocks: 'Undo the last change (Ctrl+Z), or split the program into several projects.',
};

/** The first limit among the loader's problem `codes`, or `null` when none of them is one. */
export function reachedLimit(codes: readonly string[]): ReachableLimit | null {
  for (const limit of ['nesting', 'size', 'blocks'] as const) {
    if (codes.includes(LIMIT_CODES[limit])) {
      return limit;
    }
  }
  return null;
}

/** What went past `limit` and how to get back within it, as two sentences. */
export function limitMessage(limit: ReachableLimit): string {
  return `${LIMIT_SENTENCES[limit]} ${REMEDIES[limit]}`;
}
