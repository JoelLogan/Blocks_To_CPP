/**
 * The rows of the Problems panel and how they sort (docs/spec/04-user-interface.md §4.4).
 */
import { SEVERITY_RANK } from '../shared/severity';
import type { Diagnostic } from '../types';

/** One row of the Problems panel. */
export interface ProblemItem {
  /** Stable and unique among the items (React key and focus identity). */
  key: string;
  diagnostic: Diagnostic;
  /** From the live preview, or from the last build (compiler and linker diagnostics). */
  origin: 'live' | 'build';
  /** A build diagnostic whose build no longer matches the project: shown dimmed. */
  stale: boolean;
  /** The module's display name, for example `main`. */
  modulePath: string;
  /** The block path, for example `main › repeat until › if`; empty without a block. */
  blockPath: string;
}

/** The sortable columns. */
export type ProblemColumn = 'severity' | 'message' | 'module' | 'block' | 'code';

/** The column order of the grid. */
export const PROBLEM_COLUMNS: readonly ProblemColumn[] = [
  'severity',
  'message',
  'module',
  'block',
  'code',
];

/** Column headings. */
export const PROBLEM_COLUMN_TITLES: Readonly<Record<ProblemColumn, string>> = {
  severity: 'Severity',
  message: 'Message',
  module: 'Module',
  block: 'Block',
  code: 'Code',
};

/** How the grid is sorted. Severity ascending means least serious first. */
export interface ProblemSort {
  column: ProblemColumn;
  direction: 'ascending' | 'descending';
}

/** The default order: the most serious first. */
export const DEFAULT_PROBLEM_SORT: ProblemSort = { column: 'severity', direction: 'descending' };

/** The most rows the panel shows; it says how many more there are. */
export const MAX_PROBLEM_ROWS = 1000;

const collator = new Intl.Collator('en', { numeric: true, sensitivity: 'base' });

function compareColumn(a: ProblemItem, b: ProblemItem, column: ProblemColumn): number {
  switch (column) {
    case 'severity':
      return SEVERITY_RANK[a.diagnostic.severity] - SEVERITY_RANK[b.diagnostic.severity];
    case 'message':
      return collator.compare(a.diagnostic.message, b.diagnostic.message);
    case 'module':
      return collator.compare(a.modulePath, b.modulePath);
    case 'block':
      return collator.compare(a.blockPath, b.blockPath);
    case 'code':
      return collator.compare(a.diagnostic.code, b.diagnostic.code);
  }
}

/**
 * The items in display order: by the chosen column, then the most serious first, then by module,
 * block path, message and code, and finally in the order they came, so the order is stable.
 */
export function sortProblems(items: readonly ProblemItem[], sort: ProblemSort): ProblemItem[] {
  const sign = sort.direction === 'ascending' ? 1 : -1;
  return items
    .map((item, index) => ({ item, index }))
    .sort(
      (a, b) =>
        sign * compareColumn(a.item, b.item, sort.column) ||
        compareColumn(b.item, a.item, 'severity') ||
        compareColumn(a.item, b.item, 'module') ||
        compareColumn(a.item, b.item, 'block') ||
        compareColumn(a.item, b.item, 'message') ||
        compareColumn(a.item, b.item, 'code') ||
        a.index - b.index,
    )
    .map(({ item }) => item);
}

/** The sort after the person activates a column heading: the same column flips direction. */
export function nextSort(current: ProblemSort, column: ProblemColumn): ProblemSort {
  if (current.column === column) {
    return { column, direction: current.direction === 'ascending' ? 'descending' : 'ascending' };
  }
  // Severity starts with the most serious, the text columns from A to Z.
  return { column, direction: column === 'severity' ? 'descending' : 'ascending' };
}

/** Whether a diagnostic has compiler text worth revealing: compiler, linker and toolchain ones. */
export function hasRawText(diagnostic: Diagnostic): boolean {
  return (
    (diagnostic.source === 'compiler' ||
      diagnostic.source === 'linker' ||
      diagnostic.source === 'toolchain') &&
    diagnostic.raw !== undefined &&
    diagnostic.raw !== ''
  );
}

/** "2 errors, 1 warning" (and infos), or "No problems". */
export function problemSummary(items: readonly ProblemItem[]): string {
  const counts = { error: 0, warning: 0, info: 0 };
  for (const item of items) {
    counts[item.diagnostic.severity]++;
  }
  const parts = [
    plural(counts.error, 'error', 'errors'),
    plural(counts.warning, 'warning', 'warnings'),
    plural(counts.info, 'info message', 'info messages'),
  ].filter((part) => part !== null);
  return parts.length === 0 ? 'No problems' : parts.join(', ');
}

function plural(count: number, one: string, many: string): string | null {
  if (count === 0) {
    return null;
  }
  return `${count.toLocaleString('en-US')} ${count === 1 ? one : many}`;
}
