import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { type RefObject, useEffect, useId, useMemo, useRef, useState } from 'react';

import { startSpan } from '../../lib/timing';
import '../panels.css';
import { copyPlainText } from '../shared/clipboard';
import { useTransientStatus } from '../shared/useTransientStatus';
import type { CompactSourceMap, Diagnostic, GeneratedFile, SourceMap } from '../types';
import { codeViewExtensions, setCodeViewData, type CodeViewCallbacks } from './codeView';
import { buildSourceMapIndex } from './sourcemap';

/** The IDE init unit (07 §7.6.3): compiled into IDE runs, never shown or exported. */
export const IDE_INIT_UNIT_PATH = 'ide/b2c_ide_init.cpp';

/** Whether a generated file is hidden from the code panel: anything under `ide/`. */
export function isHiddenFile(path: string): boolean {
  return path === IDE_INIT_UNIT_PATH || path.startsWith('ide/');
}

/** The props of {@link CodePanel}. */
export interface CodePanelProps {
  /** The preview's generated files. */
  files: readonly GeneratedFile[];
  /** The file to show; the first file when it is `null` or not among `files`. */
  activePath: string | null;
  /** The person picked another file. */
  onActivePathChange: (path: string) => void;
  /** The preview's source map, or `null` when the preview has none (it failed to load). */
  sourceMap: SourceMap | CompactSourceMap | null;
  /** Whether the preview could be built; if not, the code carries a "Not buildable" label. */
  buildable: boolean;
  /** Diagnostics to show as gutter markers and underlines (those with a block). */
  diagnostics: readonly Diagnostic[];
  /** The hovered or selected block: its C++ is highlighted and scrolled into view. */
  highlightBlockId: string | null;
  /** The person clicked the code of a block (or pressed Enter on it). */
  onSelectBlock: (blockId: string) => void;
}

/**
 * The C++ tab (docs/spec/04-user-interface.md §4.3): the live preview's generated code in a
 * read-only CodeMirror view, with a file switcher, *Copy all* and *Copy selection*, two-way
 * highlighting through the source map and diagnostics in the gutter. It only displays what it is
 * given and reports what the person does; it never reads the app's store.
 *
 * A copy says *Copied* for 3 s, or *Could not copy* until the next copy, just below the bar's
 * right end, over the code, so the message never moves the code (panels.css).
 *
 * While the panel cannot be seen (its dock folded, or another tab shown), it pauses (M3-5): new
 * code, diagnostics and highlights only wait, and the source map is not indexed; when it is shown
 * again it takes the latest at once, in one update.
 */
export function CodePanel(props: CodePanelProps) {
  const {
    files,
    activePath,
    onActivePathChange,
    sourceMap,
    buildable,
    diagnostics,
    highlightBlockId,
    onSelectBlock,
  } = props;

  const root = useRef<HTMLDivElement>(null);
  const shown = useShown(root);
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView | null>(null);
  const shownHighlight = useRef<string | null>(null);
  const shownPath = useRef<string | null>(null);
  const [hasSelection, setHasSelection] = useState(false);
  const copyStatus = useTransientStatus();
  const fileSelectId = useId();

  // The editor is created once; it reads the latest callbacks through this ref.
  const callbacks = useRef<CodeViewCallbacks>({
    onSelectBlock,
    onSelectionChange: setHasSelection,
  });
  useEffect(() => {
    callbacks.current = { onSelectBlock, onSelectionChange: setHasSelection };
  }, [onSelectBlock]);

  const visibleFiles = useMemo(() => files.filter((file) => !isHiddenFile(file.path)), [files]);
  const activeFile =
    visibleFiles.find((file) => file.path === activePath) ?? visibleFiles[0] ?? null;
  // Indexed only while the panel is shown, and once for each preview.
  const index = useMemo(
    () => (shown ? buildSourceMapIndex(sourceMap, files) : null),
    [shown, sourceMap, files],
  );

  useEffect(() => {
    const parent = host.current;
    if (parent === null) {
      return;
    }
    const editor = new EditorView({
      parent,
      state: EditorState.create({
        doc: '',
        extensions: codeViewExtensions(() => callbacks.current),
      }),
    });
    view.current = editor;
    return () => {
      view.current = null;
      editor.destroy();
    };
  }, []);

  const text = activeFile?.contents ?? '';
  const path = activeFile?.path ?? null;
  // Shows new code, diagnostics or a highlight: the User Timing span `b2c:code-panel`.
  useEffect(() => {
    const editor = view.current;
    if (editor === null || index === null) {
      return;
    }
    const span = startSpan('b2c:code-panel');
    const doc = editor.state.doc.toString();
    const highlightChanged = shownHighlight.current !== highlightBlockId;
    shownHighlight.current = highlightBlockId;
    // Another file starts at its top; the same file keeps its place while the preview updates.
    const fileChanged = shownPath.current !== path;
    shownPath.current = path;
    const highlightTarget =
      highlightChanged && highlightBlockId !== null
        ? index.rangesForBlock(highlightBlockId).find((range) => range.path === path)
        : undefined;
    const change = fileChanged ? { from: 0, to: doc.length, insert: text } : textChange(doc, text);
    const scrollTo = highlightTarget?.from ?? (fileChanged ? 0 : undefined);
    editor.dispatch({
      ...(change === undefined ? {} : { changes: change }),
      ...(fileChanged ? { selection: { anchor: 0 } } : {}),
      effects: [
        setCodeViewData.of({ path, index, diagnostics, highlightBlockId }),
        ...(scrollTo === undefined
          ? []
          : [EditorView.scrollIntoView(Math.min(scrollTo, text.length), { y: 'nearest' })]),
      ],
    });
    span.end();
  }, [text, path, index, diagnostics, highlightBlockId]);

  const copy = (value: string) => {
    void copyPlainText(value).then((ok) => {
      if (ok) {
        copyStatus.show('success', 'Copied');
      } else {
        copyStatus.show('failure', 'Could not copy');
      }
    });
  };

  const copySelection = () => {
    const editor = view.current;
    if (editor === null) {
      return;
    }
    const { state } = editor;
    const selected = state.selection.ranges
      .filter((range) => !range.empty)
      .map((range) => state.sliceDoc(range.from, range.to))
      .join('\n');
    if (selected !== '') {
      copy(selected);
    }
  };

  return (
    <div className="b2c-panel b2c-code-panel" data-testid="code-panel" ref={root}>
      <div className="b2c-panel-bar">
        <label className="b2c-code-file" htmlFor={fileSelectId}>
          File
        </label>
        <select
          id={fileSelectId}
          value={activeFile?.path ?? ''}
          disabled={visibleFiles.length === 0}
          onChange={(event) => {
            onActivePathChange(event.target.value);
          }}
        >
          {visibleFiles.map((file) => (
            <option key={file.path} value={file.path}>
              {file.path}
            </option>
          ))}
        </select>
        <button
          type="button"
          disabled={activeFile === null}
          onClick={() => {
            if (activeFile !== null) {
              copy(activeFile.contents);
            }
          }}
        >
          Copy all
        </button>
        <button type="button" disabled={!hasSelection} onClick={copySelection}>
          Copy selection
        </button>
        <span className="b2c-panel-status b2c-code-copy-status" role="status">
          {copyStatus.shown?.text}
        </span>
      </div>
      {activeFile !== null && !buildable && (
        <p className="b2c-code-not-buildable" data-testid="code-not-buildable">
          <span aria-hidden="true">⚠</span> Not buildable: fix the errors first
        </p>
      )}
      {activeFile === null && (
        <p className="b2c-panel-empty">The C++ for your blocks will appear here.</p>
      )}
      <div className="b2c-code-editor" ref={host} hidden={activeFile === null} />
    </div>
  );
}

/**
 * Whether `element` can be seen: not inside a folded dock or a tab that is not shown. Watched with
 * an `IntersectionObserver`; without one (as in tests) it always counts as shown.
 */
function useShown(element: RefObject<HTMLElement | null>): boolean {
  const [shown, setShown] = useState(true);
  useEffect(() => {
    const target = element.current;
    if (target === null || typeof IntersectionObserver !== 'function') {
      return;
    }
    const observer = new IntersectionObserver((entries) => {
      const last = entries[entries.length - 1];
      if (last !== undefined) {
        setShown(last.isIntersecting);
      }
    });
    observer.observe(target);
    return () => {
      observer.disconnect();
    };
  }, [element]);
  return shown;
}

/**
 * The smallest single change that turns `before` into `after` (common prefix and suffix kept), so
 * a preview update leaves the scroll position and an unrelated selection where they were.
 */
export function textChange(
  before: string,
  after: string,
): { from: number; to: number; insert: string } | undefined {
  if (before === after) {
    return undefined;
  }
  const limit = Math.min(before.length, after.length);
  let prefix = 0;
  while (prefix < limit && before.charCodeAt(prefix) === after.charCodeAt(prefix)) {
    prefix++;
  }
  let suffix = 0;
  while (
    suffix < limit - prefix &&
    before.charCodeAt(before.length - 1 - suffix) === after.charCodeAt(after.length - 1 - suffix)
  ) {
    suffix++;
  }
  // Never split a surrogate pair: move the edges outwards onto code point boundaries.
  if (prefix > 0 && isHighSurrogate(before.charCodeAt(prefix - 1))) {
    prefix--;
  }
  if (suffix > 0 && isLowSurrogate(before.charCodeAt(before.length - suffix))) {
    suffix--;
  }
  return {
    from: prefix,
    to: before.length - suffix,
    insert: after.slice(prefix, after.length - suffix),
  };
}

function isHighSurrogate(code: number): boolean {
  return code >= 0xd800 && code <= 0xdbff;
}

function isLowSurrogate(code: number): boolean {
  return code >= 0xdc00 && code <= 0xdfff;
}
