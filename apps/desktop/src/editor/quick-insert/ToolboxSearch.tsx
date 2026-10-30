/**
 * The toolbox search field (docs/spec/04-user-interface.md §4.2,
 * docs/plans/m3-contracts/w4-editor-tools.md §2.4): over the toolbox while it is open, it filters
 * the flyout live to the blocks that match (the toolbox plugin's *Search results*); the blocks are
 * dragged out, or added with the keyboard, as from any category. Escape, or leaving it empty,
 * gives the categories back. Opened with `Ctrl+F` on the canvas or the canvas menu's *Search
 * blocks…*.
 */
import * as Blockly from 'blockly/core';
import { useEffect, useId, useRef, useState, useSyncExternalStore } from 'react';

import { toolboxSearch } from '../toolbox/plugin';
import {
  closeToolboxSearch,
  subscribeToolboxSearch,
  toolboxSearchOpen,
  toolboxSearchRequests,
} from './toolboxSearchStore';
import './quickInsert.css';

/** How long typing waits before the flyout is filtered again, in milliseconds. */
export const SEARCH_DELAY_MS = 120;

/** The toolbox search of the canvas `workspace`, while it is open. */
export function ToolboxSearch({ workspace }: { readonly workspace: Blockly.WorkspaceSvg }) {
  const open = useSyncExternalStore(subscribeToolboxSearch, toolboxSearchOpen) === workspace;
  const requests = useSyncExternalStore(subscribeToolboxSearch, toolboxSearchRequests);
  const [query, setQuery] = useState('');
  const [count, setCount] = useState<number | null>(null);
  const field = useRef<HTMLInputElement>(null);
  const statusId = useId();

  useEffect(() => {
    if (open) {
      field.current?.focus();
      field.current?.select();
    }
  }, [open, requests]);

  useEffect(() => {
    if (!open) {
      return undefined;
    }
    const timer = setTimeout(() => {
      const search = toolboxSearch(workspace);
      search?.setQuery(query);
      setCount(query.trim() === '' ? null : (search?.resultCount() ?? 0));
    }, SEARCH_DELAY_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [open, query, workspace]);

  if (!open) {
    return null;
  }

  const close = (): void => {
    toolboxSearch(workspace)?.setQuery(null);
    setQuery('');
    setCount(null);
    closeToolboxSearch();
  };

  const toolboxWidth = workspace.getToolbox()?.getWidth() ?? 0;

  return (
    <div
      className="toolbox-search"
      role="search"
      aria-label="Toolbox search"
      data-testid="toolbox-search"
      style={toolboxWidth > 120 ? { width: `${String(toolboxWidth - 8)}px` } : undefined}
    >
      <input
        ref={field}
        className="toolbox-search-field"
        type="search"
        aria-label="Search blocks"
        aria-describedby={statusId}
        placeholder="Search blocks"
        autoComplete="off"
        spellCheck={false}
        value={query}
        onChange={(event) => {
          setQuery(event.target.value);
        }}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.preventDefault();
            event.stopPropagation();
            close();
            Blockly.getFocusManager().focusTree(workspace);
          } else if (event.key === 'ArrowDown' || event.key === 'Enter') {
            // Into the results: the toolbox's keyboard takes over (Right arrow and Enter add).
            const toolbox = workspace.getToolbox();
            if (toolbox !== null && query.trim() !== '') {
              event.preventDefault();
              toolboxSearch(workspace)?.setQuery(query);
              Blockly.keyboardNavigationController.setIsActive(true);
              Blockly.getFocusManager().focusTree(toolbox);
            }
          }
        }}
      />
      <div id={statusId} role="status" aria-live="polite" className="toolbox-search-status">
        {count === null
          ? ''
          : count === 0
            ? 'No block matches.'
            : count === 1
              ? '1 block'
              : `${count.toLocaleString('en-US')} blocks`}
      </div>
    </div>
  );
}
