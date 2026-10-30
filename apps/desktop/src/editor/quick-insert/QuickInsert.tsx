/**
 * The Quick Insert popup (docs/spec/03-block-language.md §3.14,
 * docs/plans/m3-contracts/w4-editor-tools.md §2.4): a search box over the blocks the toolbox
 * offers, in the ARIA combobox pattern (the box keeps the focus, Up and Down move the active
 * result, Enter inserts it, Escape closes, a polite status says how many results there are). A
 * typed line that reads as a statement (`int x = 5`) is the first result. Rendered over the canvas;
 * texts are React text, never HTML.
 */
import * as Blockly from 'blockly/core';
import { useEffect, useId, useMemo, useRef, useState, useSyncExternalStore } from 'react';

import { announceOn } from '../keyboard/plugin';
import { MAX_RESULTS, searchBlocks } from '../search';
import { insertBlock } from './insert';
import { parseLine } from './parseLine';
import { closeQuickInsert, quickInsertSession, subscribeQuickInsert } from './store';
import type { B2cBlockInfo } from '../toolbox/presets';
import './quickInsert.css';

/** One result: what it inserts and what it reads. */
interface Result {
  readonly item: B2cBlockInfo;
  readonly title: string;
  readonly detail: string;
  /** What is announced once it is inserted: *Inserted “…”*. */
  readonly name: string;
}

/** The note shown when a line looks like a statement but is not one Quick Insert reads. */
export const SIMPLE_LINES_NOTE =
  'Quick Insert reads simple lines only for now, such as int x = 5, x += 2 or print "hi".';

/** Whether typed text looks like a statement rather than a search (it has `=`, `;`, `<<`, `++`). */
function looksLikeStatement(text: string): boolean {
  return /[=;]|<<|\+\+|--/u.test(text);
}

/** The open Quick Insert of the canvas `workspace`, if any (see the module comment). */
export function QuickInsert({ workspace }: { readonly workspace: Blockly.WorkspaceSvg }) {
  const session = useSyncExternalStore(subscribeQuickInsert, quickInsertSession);
  const shown = session !== null && session.workspace === workspace ? session : null;
  const [query, setQuery] = useState('');
  const [active, setActive] = useState(0);
  const [message, setMessage] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const baseId = useId();

  useEffect(() => {
    if (shown !== null) {
      setQuery('');
      setActive(0);
      setMessage(null);
      input.current?.focus();
    }
  }, [shown]);

  const results: Result[] = useMemo(() => {
    if (shown === null) {
      return [];
    }
    const out: Result[] = [];
    const line = parseLine(query, shown.names);
    if (line !== null) {
      out.push({
        item: line.item,
        title: query.trim(),
        detail: `Insert “${line.title}”`,
        name: line.title,
      });
    }
    for (const entry of searchBlocks(shown.entries, query, MAX_RESULTS)) {
      out.push({ item: entry.item, title: entry.title, detail: entry.category, name: entry.title });
    }
    return out;
  }, [shown, query]);

  if (shown === null) {
    return null;
  }

  const close = (returnFocus: boolean): void => {
    closeQuickInsert();
    if (returnFocus) {
      try {
        Blockly.getFocusManager().focusTree(workspace);
      } catch {
        // The canvas is gone; nothing to give the focus back to.
      }
    }
  };

  const insert = (result: Result | undefined): void => {
    if (result === undefined) {
      return;
    }
    const inserted = insertBlock(workspace, result.item, shown.target, shown.symbols);
    if (inserted === null) {
      setMessage('The block could not be inserted here.');
      return;
    }
    close(false);
    announceOn(workspace, `Inserted “${result.name}”`);
  };

  const listId = `${baseId}-list`;
  const optionId = (index: number) => `${baseId}-option-${String(index)}`;
  const activeIndex = Math.min(active, Math.max(0, results.length - 1));
  const note =
    message ??
    (query.trim() !== '' && looksLikeStatement(query) && parseLine(query, shown.names) === null
      ? SIMPLE_LINES_NOTE
      : null);
  const status =
    query.trim() === ''
      ? 'Type a block’s name or a simple line of C++.'
      : results.length === 1
        ? '1 result'
        : `${results.length.toLocaleString('en-US')} results`;
  const style =
    shown.anchor === null
      ? undefined
      : { left: `${String(shown.anchor.x)}px`, top: `${String(shown.anchor.y)}px` };

  return (
    <div
      className="quick-insert"
      role="dialog"
      aria-label="Quick Insert"
      data-testid="quick-insert"
      style={style}
    >
      <input
        ref={input}
        className="quick-insert-search"
        type="text"
        role="combobox"
        aria-label="Insert a block"
        aria-expanded={results.length > 0}
        aria-controls={listId}
        aria-autocomplete="list"
        {...(results.length > 0 ? { 'aria-activedescendant': optionId(activeIndex) } : {})}
        autoComplete="off"
        spellCheck={false}
        placeholder="Type a block or a line, such as int x = 5"
        value={query}
        onBlur={(event) => {
          // Leaving the popup closes it; a click on a result keeps the focus here.
          const popup = event.currentTarget.parentElement;
          if (popup !== null && !popup.contains(event.relatedTarget)) {
            close(false);
          }
        }}
        onChange={(event) => {
          setQuery(event.target.value);
          setActive(0);
          setMessage(null);
        }}
        onKeyDown={(event) => {
          switch (event.key) {
            case 'ArrowDown':
              setActive(Math.min(activeIndex + 1, results.length - 1));
              break;
            case 'ArrowUp':
              setActive(Math.max(activeIndex - 1, 0));
              break;
            case 'Enter':
              insert(results[activeIndex]);
              break;
            case 'Escape':
              close(true);
              break;
            default:
              return;
          }
          event.preventDefault();
          event.stopPropagation();
        }}
      />
      <div id={listId} role="listbox" aria-label="Blocks" className="quick-insert-list">
        {results.map((result, index) => (
          <div
            key={`${String(index)}-${result.title}`}
            id={optionId(index)}
            role="option"
            aria-selected={index === activeIndex}
            className="quick-insert-option"
            tabIndex={-1}
            onPointerDown={(event) => {
              event.preventDefault();
            }}
            onClick={() => {
              insert(result);
            }}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                insert(result);
              }
            }}
          >
            <span className="quick-insert-title">{result.title}</span>
            <span className="quick-insert-detail">{result.detail}</span>
          </div>
        ))}
      </div>
      {note !== null && <p className="quick-insert-note">{note}</p>}
      <div role="status" aria-live="polite" className="quick-insert-status">
        {status}
      </div>
    </div>
  );
}
