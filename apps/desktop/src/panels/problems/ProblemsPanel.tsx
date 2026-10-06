import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from 'react';

import '../panels.css';
import { visibleInvisibles } from '../shared/invisibles';
import { SeverityLabel } from '../shared/severity';
import {
  DEFAULT_PROBLEM_SORT,
  hasRawText,
  MAX_PROBLEM_ROWS,
  nextSort,
  PROBLEM_COLUMN_TITLES,
  PROBLEM_COLUMNS,
  problemSummary,
  sortProblems,
  type ProblemItem,
  type ProblemSort,
} from './problems';

export type { ProblemItem } from './problems';

/** The most characters of a compiler message shown; the rest is summarised. */
export const MAX_RAW_DISPLAY_CHARS = 64 * 1024;

/** The props of {@link ProblemsPanel}. */
export interface ProblemsPanelProps {
  /** The live and build diagnostics, already merged. */
  items: readonly ProblemItem[];
  /** The person activated a row (click or Enter): select and centre its block. */
  onActivate: (item: ProblemItem) => void;
  /** The person revealed a row's C++ compiler message. */
  onShowRaw: (item: ProblemItem) => void;
  /** The person asked for the diagnostics reference. */
  onLearnMore: () => void;
}

/** The grid's columns: the sortable ones, then the details column. */
const COLUMN_COUNT = PROBLEM_COLUMNS.length + 1;
const DETAILS_COLUMN = PROBLEM_COLUMNS.length;
/** How many rows Page Up and Page Down move. */
const PAGE_ROWS = 10;

/** A cell of the grid: row 0 is the header row, rows 1… are the shown items. */
interface CellPosition {
  readonly row: number;
  readonly col: number;
}

/**
 * The Problems tab (docs/spec/04-user-interface.md §4.4): every diagnostic of the live preview and
 * the last build in a sortable grid with severity (icon and word), message, module, block path and
 * code. It follows the ARIA grid pattern: one Tab stop, arrow keys between cells, Home and End,
 * Ctrl+Home and Ctrl+End, Page Up and Page Down; Enter or a click activates a row. Compiler, linker
 * and toolchain rows can reveal the original compiler text.
 */
export function ProblemsPanel({ items, onActivate, onShowRaw, onLearnMore }: ProblemsPanelProps) {
  const [sort, setSort] = useState<ProblemSort>(DEFAULT_PROBLEM_SORT);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set());
  const [focus, setFocus] = useState<CellPosition>({ row: 1, col: 0 });
  const targets = useRef(new Map<string, HTMLElement>());
  const moveFocus = useRef(false);
  const baseId = useId();

  const sorted = useMemo(() => sortProblems(items, sort), [items, sort]);
  const shown = sorted.slice(0, MAX_PROBLEM_ROWS);
  const active: CellPosition = {
    row: Math.min(focus.row, shown.length),
    col: Math.min(focus.col, COLUMN_COUNT - 1),
  };

  useEffect(() => {
    if (moveFocus.current) {
      moveFocus.current = false;
      targets.current.get(cellKey(active))?.focus();
    }
  });

  const register = (position: CellPosition) => (element: HTMLElement | null) => {
    const key = cellKey(position);
    if (element === null) {
      targets.current.delete(key);
    } else {
      targets.current.set(key, element);
    }
  };

  const tabIndex = (position: CellPosition) =>
    position.row === active.row && position.col === active.col ? 0 : -1;

  const onCellKeyDown = (event: KeyboardEvent, position: CellPosition, item?: ProblemItem) => {
    const next = navigate(event, position, shown.length);
    if (next !== null) {
      event.preventDefault();
      moveFocus.current = true;
      setFocus(next);
      return;
    }
    if (event.key === 'Enter' && item !== undefined && event.target === event.currentTarget) {
      event.preventDefault();
      onActivate(item);
    }
  };

  const activate = (item: ProblemItem, position: CellPosition) => {
    setFocus(position);
    onActivate(item);
  };

  const toggleRaw = (item: ProblemItem) => {
    const willShow = !expanded.has(item.key);
    setExpanded((current) => {
      const next = new Set(current);
      if (willShow) {
        next.add(item.key);
      } else {
        next.delete(item.key);
      }
      return next;
    });
    if (willShow) {
      onShowRaw(item);
    }
  };

  const truncated = sorted.length > shown.length;

  return (
    <div className="b2c-panel b2c-problems-panel" data-testid="problems-panel">
      <div className="b2c-panel-bar">
        <span data-testid="problems-summary">{problemSummary(items)}</span>
        <span className="b2c-panel-status">
          <button type="button" onClick={onLearnMore}>
            Learn more
          </button>
        </span>
      </div>
      {items.length === 0 ? (
        <p className="b2c-panel-empty">No problems. Errors and warnings will appear here.</p>
      ) : (
        <div className="b2c-problems-scroll">
          <div
            className="b2c-problems-grid"
            role="grid"
            aria-label="Problems"
            {...(truncated ? { 'aria-rowcount': sorted.length + 1 } : {})}
          >
            <div className="b2c-problems-head" role="rowgroup">
              <div className="b2c-problems-row" role="row">
                {PROBLEM_COLUMNS.map((column, col) => {
                  const position = { row: 0, col };
                  return (
                    <div
                      key={column}
                      className="b2c-problems-cell"
                      role="columnheader"
                      {...(sort.column === column ? { 'aria-sort': sort.direction } : {})}
                    >
                      <button
                        type="button"
                        ref={register(position)}
                        tabIndex={tabIndex(position)}
                        onClick={() => {
                          setFocus(position);
                          setSort((current) => nextSort(current, column));
                        }}
                        onKeyDown={(event) => {
                          onCellKeyDown(event, position);
                        }}
                      >
                        {PROBLEM_COLUMN_TITLES[column]}
                        <span aria-hidden="true">{sortArrow(sort, column)}</span>
                      </button>
                    </div>
                  );
                })}
                <div
                  className="b2c-problems-cell"
                  role="columnheader"
                  ref={register({ row: 0, col: DETAILS_COLUMN })}
                  tabIndex={tabIndex({ row: 0, col: DETAILS_COLUMN })}
                  onKeyDown={(event) => {
                    onCellKeyDown(event, { row: 0, col: DETAILS_COLUMN });
                  }}
                >
                  Details
                </div>
              </div>
            </div>
            <div className="b2c-problems-body" role="rowgroup">
              {shown.map((item, index) => {
                const row = index + 1;
                const rawId = `${baseId}-raw-${String(index)}`;
                const showRaw = expanded.has(item.key) && hasRawText(item.diagnostic);
                const cell = (col: number) => {
                  const position = { row, col };
                  return {
                    className: 'b2c-problems-cell',
                    role: 'gridcell',
                    ref: register(position),
                    tabIndex: tabIndex(position),
                    onClick: () => {
                      activate(item, position);
                    },
                    onKeyDown: (event: KeyboardEvent) => {
                      onCellKeyDown(event, position, item);
                    },
                  } as const;
                };
                const { diagnostic } = item;
                return (
                  <div
                    key={item.key}
                    role="row"
                    className={
                      item.stale ? 'b2c-problems-row b2c-problems-stale' : 'b2c-problems-row'
                    }
                    data-testid="problem-row"
                    {...(truncated ? { 'aria-rowindex': row + 1 } : {})}
                  >
                    <div {...cell(0)}>
                      <SeverityLabel severity={diagnostic.severity} />
                    </div>
                    <div {...cell(1)}>
                      {visibleInvisibles(diagnostic.message)}
                      {item.stale && (
                        <span className="b2c-problems-stale-note"> (from the last build)</span>
                      )}
                      {showRaw && (
                        <pre id={rawId} className="b2c-problems-raw">
                          {rawForDisplay(diagnostic.raw ?? '')}
                        </pre>
                      )}
                    </div>
                    <div {...cell(2)}>{visibleInvisibles(item.modulePath)}</div>
                    <div {...cell(3)}>{visibleInvisibles(item.blockPath)}</div>
                    <div {...cell(4)} className="b2c-problems-cell b2c-problems-code">
                      {visibleInvisibles(diagnostic.code)}
                    </div>
                    {hasRawText(diagnostic) ? (
                      <div className="b2c-problems-cell" role="gridcell">
                        <button
                          type="button"
                          ref={register({ row, col: DETAILS_COLUMN })}
                          tabIndex={tabIndex({ row, col: DETAILS_COLUMN })}
                          aria-expanded={showRaw}
                          {...(showRaw ? { 'aria-controls': rawId } : {})}
                          onClick={(event) => {
                            event.stopPropagation();
                            setFocus({ row, col: DETAILS_COLUMN });
                            toggleRaw(item);
                          }}
                          onKeyDown={(event) => {
                            onCellKeyDown(event, { row, col: DETAILS_COLUMN });
                          }}
                        >
                          {showRaw ? 'Hide C++ compiler message' : 'Show C++ compiler message'}
                        </button>
                      </div>
                    ) : (
                      <div {...cell(DETAILS_COLUMN)} />
                    )}
                  </div>
                );
              })}
            </div>
          </div>
          {truncated && (
            <p className="b2c-panel-empty">
              Showing the first {MAX_PROBLEM_ROWS.toLocaleString('en-US')} of{' '}
              {sorted.length.toLocaleString('en-US')} problems.
            </p>
          )}
        </div>
      )}
    </div>
  );
}

function cellKey(position: CellPosition): string {
  return `${String(position.row)}:${String(position.col)}`;
}

/** The arrow shown after the sorted column's heading (the state itself is in `aria-sort`). */
function sortArrow(sort: ProblemSort, column: string): string {
  if (sort.column !== column) {
    return '';
  }
  return sort.direction === 'ascending' ? ' ▲' : ' ▼';
}

/** Where a grid navigation key moves focus from `from`, or `null` for any other key. */
function navigate(event: KeyboardEvent, from: CellPosition, rows: number): CellPosition | null {
  const lastRow = rows;
  const lastCol = COLUMN_COUNT - 1;
  const ctrl = event.ctrlKey || event.metaKey;
  switch (event.key) {
    case 'ArrowRight':
      return { row: from.row, col: Math.min(from.col + 1, lastCol) };
    case 'ArrowLeft':
      return { row: from.row, col: Math.max(from.col - 1, 0) };
    case 'ArrowDown':
      return { row: Math.min(from.row + 1, lastRow), col: from.col };
    case 'ArrowUp':
      return { row: Math.max(from.row - 1, 0), col: from.col };
    case 'PageDown':
      return { row: Math.min(from.row + PAGE_ROWS, lastRow), col: from.col };
    case 'PageUp':
      return { row: Math.max(from.row - PAGE_ROWS, 0), col: from.col };
    case 'Home':
      return ctrl ? { row: 0, col: 0 } : { row: from.row, col: 0 };
    case 'End':
      return ctrl ? { row: lastRow, col: lastCol } : { row: from.row, col: lastCol };
    default:
      return null;
  }
}

/** Compiler text for display: hidden characters made visible, very long text shortened. */
function rawForDisplay(raw: string): string {
  const visible = visibleInvisibles(raw);
  if (visible.length <= MAX_RAW_DISPLAY_CHARS) {
    return visible;
  }
  const rest = visible.length - MAX_RAW_DISPLAY_CHARS;
  return `${visible.slice(0, MAX_RAW_DISPLAY_CHARS)}\n… (${rest.toLocaleString('en-US')} more characters)`;
}
