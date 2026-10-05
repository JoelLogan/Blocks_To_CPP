/**
 * What a block's diagnostics badge says (docs/spec/04-user-interface.md §4.4): the most serious
 * severity (it wins the icon and the outline), whether the badge is dimmed, and the text of its
 * tooltip and accessible name. Pure functions, so the rules are tested without Blockly.
 */
import type { Part, Severity } from '@blocks2cpp/b2c-core-wasm';

import { truncateForDisplay, visibleInvisibles } from '../text';

export type { Severity } from '@blocks2cpp/b2c-core-wasm';

/** One diagnostic as a block shows it. A `Diagnostic` of the pipeline is one. */
export interface BlockDiagnosticItem {
  /** The stable code, for example `B2C-E0201`. */
  readonly code: string;
  readonly severity: Severity;
  /** The friendly message. */
  readonly message: string;
  /** The part of the block it points at; the whole block when absent. */
  readonly primary?: { readonly part: Part };
  /**
   * A compiler or linker message of the last build while the project has changed since: shown
   * dimmed and marked "from the last build" (docs/spec/04-user-interface.md §4.4).
   */
  readonly stale?: boolean;
  /**
   * Reported on a block inside this one that does not show (this block is collapsed, or it is a
   * placeholder that keeps the block as data): listed, but no part of this block is marked.
   */
  readonly nested?: boolean;
}

/** What a badge shows, from {@link summariseDiagnostics}. */
export interface DiagnosticBadgeSummary {
  /** The most serious severity: the badge's icon and the block's outline. */
  readonly severity: Severity;
  /** Every diagnostic is from an older build: the badge and the outline are dimmed. */
  readonly dimmed: boolean;
  /** How many diagnostics the block has. */
  readonly count: number;
  /** The tooltip: one line per diagnostic, the most serious first (plain text). */
  readonly tooltip: string;
  /** The accessible name: the first diagnostic and how many more there are. */
  readonly label: string;
}

/** The icon of each severity (04 §4.4). */
export const SEVERITY_GLYPH: Readonly<Record<Severity, string>> = Object.freeze({
  error: '✖',
  warning: '⚠',
  info: 'ℹ',
});

/** The word for each severity, so colour and icon are never the only signal. */
export const SEVERITY_LABEL: Readonly<Record<Severity, string>> = Object.freeze({
  error: 'Error',
  warning: 'Warning',
  info: 'Info',
});

/** Higher is more serious. */
export const SEVERITY_ORDER: Readonly<Record<Severity, number>> = Object.freeze({
  info: 1,
  warning: 2,
  error: 3,
});

/** The most diagnostics one badge's tooltip lists before "and N more". */
export const MAX_TOOLTIP_LINES = 10;

/** The longest message shown in a tooltip line or a label, in code points. */
export const MAX_MESSAGE_CHARS = 240;

/** The longest code shown, in code points (codes are short; anything longer is not ours). */
const MAX_CODE_CHARS = 32;

/** Whether `value` is one of the three severities (diagnostics are data: check before use). */
export function isSeverity(value: unknown): value is Severity {
  return value === 'error' || value === 'warning' || value === 'info';
}

/** The more serious of two severities. */
export function moreSerious(a: Severity, b: Severity): Severity {
  return SEVERITY_ORDER[b] > SEVERITY_ORDER[a] ? b : a;
}

/** Message or code text for display: hidden characters made visible, one line, shortened. */
function displayText(text: string, max: number): string {
  return truncateForDisplay(visibleInvisibles(text, { lineBreaks: true }), max);
}

/** The diagnostics a block shows: those with a known severity and text, most serious first. */
export function usableItems(items: readonly BlockDiagnosticItem[]): BlockDiagnosticItem[] {
  return items
    .filter(
      (item) =>
        isSeverity(item.severity) &&
        typeof item.message === 'string' &&
        typeof item.code === 'string',
    )
    .map((item, index) => ({ item, index }))
    .sort(
      (a, b) =>
        SEVERITY_ORDER[b.item.severity] - SEVERITY_ORDER[a.item.severity] || a.index - b.index,
    )
    .map(({ item }) => item);
}

/** One tooltip line: `✖ Error: message (B2C-E0201)`, with where it comes from when that matters. */
function tooltipLine(item: BlockDiagnosticItem): string {
  const notes = [
    item.nested === true ? 'in a block inside this one' : null,
    item.stale === true ? 'from the last build' : null,
  ].filter((note) => note !== null);
  const suffix = notes.length === 0 ? '' : ` – ${notes.join(', ')}`;
  return (
    `${SEVERITY_GLYPH[item.severity]} ${SEVERITY_LABEL[item.severity]}: ` +
    `${displayText(item.message, MAX_MESSAGE_CHARS)} (${displayText(item.code, MAX_CODE_CHARS)})` +
    suffix
  );
}

/**
 * What the badge of a block with `items` shows, or null when there is nothing to show (no items,
 * or none with a known severity). The most serious severity wins; the badge is dimmed only when
 * every item is stale.
 */
export function summariseDiagnostics(
  items: readonly BlockDiagnosticItem[],
): DiagnosticBadgeSummary | null {
  const usable = usableItems(items);
  const first = usable[0];
  if (first === undefined) {
    return null;
  }
  const shown = usable.slice(0, MAX_TOOLTIP_LINES).map(tooltipLine);
  const hidden = usable.length - shown.length;
  if (hidden > 0) {
    shown.push(`and ${hidden.toLocaleString('en-US')} more`);
  }
  const dimmed = usable.every((item) => item.stale === true);
  const more = usable.length - 1;
  const label =
    `${SEVERITY_LABEL[first.severity]}: ${displayText(first.message, MAX_MESSAGE_CHARS)}` +
    (first.stale === true ? ' (from the last build)' : '') +
    (more > 0
      ? `, and ${more.toLocaleString('en-US')} more ${more === 1 ? 'problem' : 'problems'}`
      : '');
  return {
    severity: first.severity,
    dimmed,
    count: usable.length,
    tooltip: shown.join('\n'),
    label,
  };
}

/** Whether two summaries show the same thing (so the badge need not be redrawn). */
export function sameSummary(a: DiagnosticBadgeSummary, b: DiagnosticBadgeSummary): boolean {
  return (
    a.severity === b.severity &&
    a.dimmed === b.dimmed &&
    a.count === b.count &&
    a.tooltip === b.tooltip &&
    a.label === b.label
  );
}
