/**
 * Marks on the part of a block a diagnostic points at (docs/spec/04-user-interface.md §4.4): a
 * field is outlined and underlined, the block in an input (often an expression shadow) is
 * outlined, and a token range is underlined in the read-only expression shadow that shows it. The
 * marks are kept per block and changed by difference, so an unchanged diagnostic touches nothing.
 */
import * as Blockly from 'blockly/core';

import { setTokenHighlight } from '../shadows/state';
import { isExprShadowType } from '../shadows/types';
import { DIAGNOSTIC_CSS_CLASS } from './css';
import { type BlockDiagnosticItem, SEVERITY_ORDER, type Severity } from './summary';

/** One applied mark: what it is on, and how to take it off again. */
interface AppliedMark {
  /** The field or block the mark is on; a new object under the same key is a new mark. */
  readonly target: object;
  readonly remove: () => void;
}

/** A mark wanted on a block: its key, what it goes on, and how to put it there. */
interface WantedMark {
  readonly target: object;
  readonly apply: () => () => void;
}

const marksByBlock = new WeakMap<Blockly.Block, Map<string, AppliedMark>>();

/** Whether a block has been disposed of (its marks need no undoing). */
function isGone(block: Blockly.Block): boolean {
  return block.isDeadOrDying();
}

function markField(field: Blockly.Field, severity: Severity): (() => void) | null {
  const root = field.getSvgRoot();
  if (root === null) {
    return null;
  }
  const className = DIAGNOSTIC_CSS_CLASS.field[severity];
  Blockly.utils.dom.addClass(root, className);
  return () => {
    Blockly.utils.dom.removeClass(root, className);
  };
}

function markBlock(block: Blockly.Block, severity: Severity): (() => void) | null {
  if (!(block instanceof Blockly.BlockSvg)) {
    return null;
  }
  const className = DIAGNOSTIC_CSS_CLASS.input[severity];
  block.addClass(className);
  return () => {
    if (!isGone(block)) {
      block.removeClass(className);
    }
  };
}

/** Underlines tokens `start` to `end` of an expression shadow and outlines the shadow. */
function markTokens(
  shadow: Blockly.Block,
  start: number,
  end: number,
  severity: Severity,
): () => void {
  setTokenHighlight(shadow, { start, end });
  const unmark = markBlock(shadow, severity);
  return () => {
    if (!isGone(shadow)) {
      setTokenHighlight(shadow, null);
    }
    unmark?.();
  };
}

/** The block connected to a value or statement input, or null. */
function blockIn(block: Blockly.Block, inputName: string): Blockly.Block | null {
  return block.getInput(inputName)?.connection?.targetBlock() ?? null;
}

/** A mark someone asked for, not yet made: its severity and how to make it. */
interface MarkRequest {
  readonly severity: Severity;
  readonly make: (severity: Severity) => WantedMark | null;
}

/**
 * Records a mark under `key`. When two items point at the same part, the first of the most
 * serious ones decides what is marked (for a token slot: which range), at that severity.
 */
function want(
  wanted: Map<string, MarkRequest>,
  key: string,
  severity: Severity,
  make: (severity: Severity) => WantedMark | null,
): void {
  const known = wanted.get(key);
  if (known === undefined) {
    wanted.set(key, { severity, make });
  } else if (SEVERITY_ORDER[severity] > SEVERITY_ORDER[known.severity]) {
    wanted.set(key, { severity, make });
  }
}

/**
 * The marks `items` want on `block`, by key. Items about blocks inside this one (`nested`) mark
 * nothing; a part the block does not have (a field or input that is not there) is skipped.
 */
function wantedMarks(
  block: Blockly.Block,
  items: readonly BlockDiagnosticItem[],
): Map<string, WantedMark> {
  const wanted = new Map<string, MarkRequest>();
  for (const item of items) {
    const part = item.primary?.part;
    if (item.nested === true || part === undefined || typeof part !== 'object') {
      continue;
    }
    switch (part.kind) {
      case 'field': {
        const field = block.getField(part.name);
        if (field !== null) {
          want(wanted, `field:${part.name}`, item.severity, (severity) => ({
            target: field,
            apply: () => markField(field, severity) ?? noop,
          }));
        }
        break;
      }
      case 'input': {
        const target = blockIn(block, part.name);
        if (target !== null) {
          want(wanted, `input:${part.name}`, item.severity, (severity) => ({
            target,
            apply: () => markBlock(target, severity) ?? noop,
          }));
        }
        break;
      }
      case 'tokens': {
        const target = blockIn(block, part.input);
        if (target === null) {
          break;
        }
        if (isExprShadowType(target.type)) {
          const { start, end } = part;
          // One mark per slot: a shadow underlines one range, the most serious one's.
          want(wanted, `tokens:${part.input}`, item.severity, (severity) => ({
            target,
            apply: () => markTokens(target, start, end, severity),
          }));
        } else {
          // A block in the slot instead of tokens: mark the slot as a whole.
          want(wanted, `input:${part.input}`, item.severity, (severity) => ({
            target,
            apply: () => markBlock(target, severity) ?? noop,
          }));
        }
        break;
      }
      case 'whole':
        break;
    }
  }
  const result = new Map<string, WantedMark>();
  for (const [key, { severity, make }] of wanted) {
    const mark = make(severity);
    if (mark !== null) {
      // The severity is part of the key: a changed severity is a new mark.
      result.set(`${key}:${severity}`, mark);
    }
  }
  return result;
}

function noop(): void {
  // Nothing was marked, so there is nothing to remove.
}

/**
 * Puts the part marks of `items` on `block` and takes off the ones no longer wanted. With no
 * items, every mark goes. Safe on blocks without SVG (a headless workspace marks token ranges
 * only).
 */
export function syncPartMarks(block: Blockly.Block, items: readonly BlockDiagnosticItem[]): void {
  const applied = marksByBlock.get(block) ?? new Map<string, AppliedMark>();
  const wanted = isGone(block) ? new Map<string, WantedMark>() : wantedMarks(block, items);
  for (const [key, mark] of applied) {
    if (wanted.get(key)?.target !== mark.target) {
      mark.remove();
      applied.delete(key);
    }
  }
  for (const [key, mark] of wanted) {
    if (!applied.has(key)) {
      applied.set(key, { target: mark.target, remove: mark.apply() });
    }
  }
  if (applied.size === 0) {
    marksByBlock.delete(block);
  } else {
    marksByBlock.set(block, applied);
  }
}

/** The keys of the marks on a block (`field:NAME:severity`, …), for tests and diagnostics. */
export function partMarkKeys(block: Blockly.Block): string[] {
  return [...(marksByBlock.get(block)?.keys() ?? [])].sort();
}
