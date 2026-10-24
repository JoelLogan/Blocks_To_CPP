/**
 * Long statement chains. Blockly links the statements of a list or stack one below the other, each
 * block's next block being its child, and its own walks follow those links recursively: `dispose`,
 * and the serialisation that every create and delete event does for undo (XML, the descendant
 * IDs and JSON). A chain of a few thousand blocks can therefore overflow the JavaScript stack, at a
 * length that depends on the engine and on how deep the caller already is.
 *
 * The clipboard never lets that happen halfway: everything that serialises runs before anything
 * changes, an overflow there is reported as a limit of the editor (`RangeError` is what V8 and
 * JavaScriptCore throw for it), and blocks are removed with {@link disposeTree}, which never
 * recurses along a chain.
 */
import type { BdmBlock, Diagnostic } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';

import { forEachNode } from '../sync/bdmTree';
import { withoutEvents } from '../sync/bdmToWorkspace';
import { descendantsOf } from '../sync/traverse';

/**
 * Thrown when blocks could not be built, announced for undo or deleted because a chain of
 * statements is longer than Blockly can serialise here. Nothing was changed.
 */
export class ChainTooLongError extends Error {
  /** The most statements chained one below the other among the blocks concerned. */
  readonly longestChain: number;

  constructor(longestChain: number, cause: unknown) {
    super(`a chain of ${String(longestChain)} statements is too long for the editor`, { cause });
    this.name = 'ChainTooLongError';
    this.longestChain = longestChain;
  }
}

/**
 * Whether `error` means that the JavaScript stack ran out (V8 and JavaScriptCore throw a
 * `RangeError`, "Maximum call stack size exceeded"), which for the clipboard's blocks means a
 * chain too long to serialise.
 */
export function isStackOverflow(error: unknown): boolean {
  return error instanceof RangeError;
}

/**
 * The longest statement chain among `nodes` and everything nested in them: a statement list, or
 * a loose stack with its head. Runs of separate nodes that are chained when inserted count too:
 * pass their length as `run`.
 */
export function longestChainOf(nodes: readonly BdmBlock[], run = 0): number {
  let longest = run;
  forEachNode(nodes, (node) => {
    if (node.stack !== undefined) {
      longest = Math.max(longest, node.stack.length + 1);
    }
    for (const list of Object.values(node.statements ?? {})) {
      longest = Math.max(longest, list.length);
    }
  });
  return longest;
}

/** The longest statement chain on the canvas that starts at `root` or is nested in it. */
export function longestChainFrom(root: Blockly.Block): number {
  let longest = 0;
  for (const block of descendantsOf(root)) {
    const previous = block.getPreviousBlock();
    // A chain starts at a block that no block has as its next one (the first of a list too).
    if (previous !== null && previous.getNextBlock() === block) {
      continue;
    }
    let length = 0;
    for (let next: Blockly.Block | null = block; next !== null; next = next.getNextBlock()) {
      length += 1;
    }
    longest = Math.max(longest, length);
  }
  return longest;
}

/**
 * The problem shown for a {@link ChainTooLongError}. It has the loader's code for nesting that is
 * too deep (`B2C-E0104`): Blockly nests each statement of a chain inside the one before it, so a
 * long chain is deep nesting there, although the project format keeps lists flat.
 */
export function chainTooLongDiagnostic(longestChain: number): Diagnostic {
  const statements = longestChain === 1 ? 'statement' : 'statements';
  return {
    code: 'B2C-E0104',
    severity: 'error',
    message: `A chain of ${String(longestChain)} ${statements} is longer than the editor can handle in one piece, because the block editor nests each statement inside the one before it. Move or paste fewer statements at a time, or put some of them into a function.`,
    primary: { part: { kind: 'whole' } },
    source: 'loader',
  };
}

/**
 * Removes `root` with everything nested in it or chained below it, with Blockly's events off.
 * Blocks are disposed children first (shadows with their parent), so Blockly's recursive
 * `dispose` never goes deeper than a block's shadows. With `animate`, a rendered root block goes
 * with Blockly's delete animation.
 */
export function disposeTree(root: Blockly.Block, animate = false): void {
  withoutEvents(() => {
    const blocks = descendantsOf(root).filter((block) => block !== root && !block.isShadow());
    for (let index = blocks.length - 1; index >= 0; index -= 1) {
      const block = blocks[index];
      if (block !== undefined && !block.isDeadOrDying()) {
        block.dispose(false);
      }
    }
    if (root.isDeadOrDying()) {
      return;
    }
    if (animate && root instanceof Blockly.BlockSvg) {
      root.dispose(false, true);
    } else {
      root.dispose(false);
    }
  });
}
