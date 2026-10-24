/**
 * The CodeMirror 6 setup of the code panel (docs/spec/04-user-interface.md §4.3): a read-only C++
 * view with Lezer highlighting (no workers, nothing loaded at run time), visible placeholders for
 * hidden characters, the highlight of the hovered or selected block, and diagnostics as gutter
 * markers with an underline. Everything is drawn from the preview's source map: g++ line numbers
 * are never used, because the panel shows the preview and not what g++ compiled.
 */
import { cpp } from '@codemirror/lang-cpp';
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language';
import {
  EditorState,
  RangeSet,
  StateEffect,
  StateField,
  type Extension,
  type Text,
} from '@codemirror/state';
import {
  Decoration,
  EditorView,
  GutterMarker,
  gutter,
  highlightSpecialChars,
  keymap,
  lineNumbers,
  type DecorationSet,
} from '@codemirror/view';
import { tags } from '@lezer/highlight';

import { hiddenCharacterPattern, placeholderFor, visibleInvisibles } from '../shared/invisibles';
import { SEVERITY_ICON, SEVERITY_RANK, SEVERITY_WORD, strongerSeverity } from '../shared/severity';
import type { Diagnostic, Severity } from '../types';
import type { CodeRange, SourceMapIndex } from './sourcemap';

/** The most diagnostics drawn in the code panel; Problems lists them all. */
export const MAX_CODE_DIAGNOSTICS = 1000;

/** The most messages one gutter marker's tooltip lists before "and N more". */
export const MAX_MESSAGES_PER_MARKER = 10;

/** What the panel draws on top of the text. */
export interface CodeViewData {
  /** The file shown, or `null` when there is none. */
  readonly path: string | null;
  /** The preview's source map, indexed against the preview's files. */
  readonly index: SourceMapIndex;
  /** The diagnostics to mark: those with a block are drawn, the others are only in Problems. */
  readonly diagnostics: readonly Diagnostic[];
  /** The hovered or selected block, whose C++ is highlighted. */
  readonly highlightBlockId: string | null;
}

/** Replaces the {@link CodeViewData}; sent with the matching text change in one transaction. */
export const setCodeViewData = StateEffect.define<CodeViewData>();

/** The decorations and markers drawn from one {@link CodeViewData} on one text. */
interface Drawn {
  readonly data: CodeViewData;
  readonly highlight: DecorationSet;
  readonly underlines: DecorationSet;
  readonly markers: RangeSet<GutterMarker>;
}

const EMPTY_INDEX: SourceMapIndex = {
  rangesForBlock: () => [],
  rangesForPart: () => [],
  blockAt: () => null,
};

const EMPTY_DATA: CodeViewData = {
  path: null,
  index: EMPTY_INDEX,
  diagnostics: [],
  highlightBlockId: null,
};

const BLOCK_HIGHLIGHT = Decoration.mark({ class: 'cm-b2c-block-highlight' });

/** The gutter marker of one line: the strongest severity's icon, every message in its tooltip. */
class DiagnosticMarker extends GutterMarker {
  readonly severity: Severity;
  readonly label: string;

  constructor(severity: Severity, label: string) {
    super();
    this.severity = severity;
    this.label = label;
  }

  override eq(other: GutterMarker): boolean {
    return (
      other instanceof DiagnosticMarker &&
      other.severity === this.severity &&
      other.label === this.label
    );
  }

  override toDOM(): Node {
    const marker = document.createElement('span');
    marker.className = `cm-b2c-gutter-marker cm-b2c-gutter-${this.severity}`;
    marker.textContent = SEVERITY_ICON[this.severity];
    marker.title = this.label;
    return marker;
  }
}

/** Keeps the panel's {@link Drawn} state in step with the data and the text. */
const drawnField = StateField.define<Drawn>({
  create: (state) => draw(EMPTY_DATA, state.doc),
  update(drawn, transaction) {
    let data: CodeViewData | null = null;
    for (const effect of transaction.effects) {
      if (effect.is(setCodeViewData)) {
        data = effect.value;
      }
    }
    if (data !== null) {
      return draw(data, transaction.state.doc);
    }
    if (transaction.docChanged) {
      return {
        data: drawn.data,
        highlight: drawn.highlight.map(transaction.changes),
        underlines: drawn.underlines.map(transaction.changes),
        markers: drawn.markers.map(transaction.changes),
      };
    }
    return drawn;
  },
  provide: (field) => [
    EditorView.decorations.from(field, (drawn) => drawn.underlines),
    EditorView.decorations.from(field, (drawn) => drawn.highlight),
  ],
});

/** The data the view currently draws. */
export function codeViewData(state: EditorState): CodeViewData {
  return state.field(drawnField).data;
}

/** The ranges of `ranges` that are in `path`, clamped to the text and not empty. */
function inFile(ranges: readonly CodeRange[], path: string | null, doc: Text): CodeRange[] {
  const length = doc.length;
  const result: CodeRange[] = [];
  for (const range of ranges) {
    if (range.path !== path) {
      continue;
    }
    const from = Math.min(Math.max(range.from, 0), length);
    const to = Math.min(Math.max(range.to, 0), length);
    if (to > from) {
      result.push({ path: range.path, from, to });
    }
  }
  return result;
}

/**
 * Where a diagnostic is drawn: its part's ranges in the current preview, or the whole block's
 * when the source map has none for the part.
 */
export function diagnosticRanges(
  diagnostic: Diagnostic,
  index: SourceMapIndex,
): readonly CodeRange[] {
  const { block, part } = diagnostic.primary;
  if (block === undefined) {
    return [];
  }
  if (part.kind !== 'whole') {
    const forPart = index.rangesForPart(block, part);
    if (forPart.length > 0) {
      return forPart;
    }
  }
  return index.rangesForBlock(block);
}

/** The tooltip text of one diagnostic, with hidden characters made visible. */
export function diagnosticLabel(diagnostic: Diagnostic): string {
  const word = SEVERITY_WORD[diagnostic.severity];
  return `${word}: ${visibleInvisibles(diagnostic.message)} (${visibleInvisibles(diagnostic.code)})`;
}

function draw(data: CodeViewData, doc: Text): Drawn {
  const highlight =
    data.highlightBlockId === null
      ? Decoration.none
      : Decoration.set(
          inFile(data.index.rangesForBlock(data.highlightBlockId), data.path, doc).map((range) =>
            BLOCK_HIGHLIGHT.range(range.from, range.to),
          ),
          true,
        );

  const underlines = [];
  const lines = new Map<number, LineDiagnostics>();
  for (const diagnostic of data.diagnostics.slice(0, MAX_CODE_DIAGNOSTICS)) {
    const ranges = inFile(diagnosticRanges(diagnostic, data.index), data.path, doc);
    if (ranges.length === 0) {
      continue;
    }
    const label = diagnosticLabel(diagnostic);
    const underline = Decoration.mark({
      class: `cm-b2c-diag cm-b2c-diag-${diagnostic.severity}`,
      attributes: { title: label },
    });
    for (const range of ranges) {
      underlines.push(underline.range(range.from, range.to));
      const lineStart = doc.lineAt(range.from).from;
      const line = lines.get(lineStart);
      const entry = { severity: diagnostic.severity, label };
      if (line === undefined) {
        lines.set(lineStart, { severity: diagnostic.severity, entries: [entry] });
      } else {
        line.severity = strongerSeverity(line.severity, diagnostic.severity);
        if (!line.entries.some((known) => known.label === label)) {
          line.entries.push(entry);
        }
      }
    }
  }

  const markers = [...lines.entries()]
    .sort(([a], [b]) => a - b)
    .map(([lineStart, line]) =>
      new DiagnosticMarker(line.severity, markerLabel(line.entries)).range(lineStart),
    );

  return {
    data,
    highlight,
    underlines: Decoration.set(underlines, true),
    markers: RangeSet.of(markers, true),
  };
}

/** The diagnostics that start on one line of the text. */
interface LineDiagnostics {
  /** The most serious of them, whose icon the gutter shows. */
  severity: Severity;
  /** Each distinct message once, in the order they came. */
  readonly entries: { readonly severity: Severity; readonly label: string }[];
}

/** The tooltip of a gutter marker: one message per line, the most serious first. */
function markerLabel(entries: LineDiagnostics['entries']): string {
  // `sort` is stable, so equally serious messages keep their order.
  const sorted = [...entries].sort((a, b) => SEVERITY_RANK[b.severity] - SEVERITY_RANK[a.severity]);
  const shown = sorted.slice(0, MAX_MESSAGES_PER_MARKER).map((entry) => entry.label);
  const hidden = sorted.length - shown.length;
  return hidden > 0 ? [...shown, `and ${String(hidden)} more`].join('\n') : shown.join('\n');
}

/** Shows a hidden character as `⟨U+200B⟩`, readable and announced, instead of CodeMirror's dot. */
function renderHiddenCharacter(code: number): HTMLElement {
  const placeholder = placeholderFor(code);
  const element = document.createElement('span');
  element.className = 'cm-b2c-hidden-char';
  element.textContent = placeholder;
  element.title = `Hidden character ${placeholder.slice(1, -1)}`;
  return element;
}

/** Colours come from CSS custom properties (panels.css), so they follow the light and dark theme. */
const cppHighlightStyle = HighlightStyle.define([
  {
    tag: [tags.keyword, tags.controlKeyword, tags.definitionKeyword, tags.modifier],
    color: 'var(--b2c-code-keyword)',
  },
  {
    tag: [tags.operatorKeyword, tags.self, tags.null, tags.bool],
    color: 'var(--b2c-code-keyword)',
  },
  { tag: [tags.typeName, tags.standard(tags.typeName)], color: 'var(--b2c-code-type)' },
  { tag: [tags.namespace], color: 'var(--b2c-code-type)' },
  {
    tag: [tags.string, tags.special(tags.string), tags.character],
    color: 'var(--b2c-code-string)',
  },
  { tag: [tags.escape], color: 'var(--b2c-code-escape)' },
  { tag: [tags.number, tags.literal], color: 'var(--b2c-code-number)' },
  {
    tag: [tags.lineComment, tags.blockComment, tags.comment],
    color: 'var(--b2c-code-comment)',
    fontStyle: 'italic',
  },
  { tag: [tags.processingInstruction, tags.meta], color: 'var(--b2c-code-preprocessor)' },
  {
    tag: [tags.function(tags.variableName), tags.function(tags.definition(tags.variableName))],
    color: 'var(--b2c-code-function)',
  },
]);

/** Layout and colours of the editor; the tokens are defined in panels.css. */
const codeTheme = EditorView.theme({
  '&': {
    height: '100%',
    color: 'var(--b2c-code-text)',
    backgroundColor: 'var(--b2c-code-bg)',
    fontSize: '13px',
  },
  '&.cm-focused': {
    outline: '2px solid var(--b2c-panel-focus)',
    outlineOffset: '-2px',
  },
  '.cm-scroller': {
    fontFamily: 'var(--b2c-mono-font)',
    lineHeight: '1.5',
  },
  '.cm-content': {
    caretColor: 'var(--b2c-code-text)',
  },
  '.cm-gutters': {
    backgroundColor: 'var(--b2c-code-gutter-bg)',
    color: 'var(--b2c-code-gutter-text)',
    borderRight: '1px solid var(--b2c-border, #c9cfdb)',
  },
  '.cm-b2c-block-highlight': {
    backgroundColor: 'var(--b2c-code-highlight)',
    outline: '1px solid var(--b2c-code-highlight-border)',
  },
  '.cm-b2c-diag': {
    textDecorationLine: 'underline',
    textDecorationThickness: '2px',
    textUnderlineOffset: '3px',
  },
  // Each severity has its own line style as well as its own colour.
  '.cm-b2c-diag-error': {
    textDecorationStyle: 'wavy',
    textDecorationColor: 'var(--b2c-panel-error)',
  },
  '.cm-b2c-diag-warning': {
    textDecorationStyle: 'dashed',
    textDecorationColor: 'var(--b2c-panel-warning)',
  },
  '.cm-b2c-diag-info': {
    textDecorationStyle: 'dotted',
    textDecorationColor: 'var(--b2c-panel-info)',
  },
  '.cm-b2c-diagnostics-gutter': {
    minWidth: '1.4em',
    textAlign: 'center',
  },
  '.cm-b2c-gutter-marker': {
    cursor: 'default',
  },
  '.cm-b2c-gutter-error': { color: 'var(--b2c-panel-error)' },
  '.cm-b2c-gutter-warning': { color: 'var(--b2c-panel-warning)' },
  '.cm-b2c-gutter-info': { color: 'var(--b2c-panel-info)' },
  '.cm-b2c-hidden-char': {
    color: 'var(--b2c-code-hidden-char)',
    border: '1px solid currentColor',
    borderRadius: '3px',
    padding: '0 1px',
    fontSize: '85%',
  },
});

/** What the code panel's editor reports back. */
export interface CodeViewCallbacks {
  /** The person clicked (or pressed Enter) in the code: select the block that produced it. */
  readonly onSelectBlock: (blockId: string) => void;
  /** Whether the editor's selection is empty changed. */
  readonly onSelectionChange: (hasSelection: boolean) => void;
}

/** Selects the block at the caret: Enter in the read-only view. */
function selectBlockAtCaret(view: EditorView, callbacks: CodeViewCallbacks): boolean {
  const data = codeViewData(view.state);
  if (data.path === null) {
    return false;
  }
  const block = data.index.blockAt(data.path, view.state.selection.main.head);
  if (block === null) {
    return false;
  }
  callbacks.onSelectBlock(block);
  return true;
}

/**
 * Every extension of the code panel's editor. The callbacks are read through a getter, so the
 * React component can pass its latest props without rebuilding the editor.
 */
export function codeViewExtensions(callbacks: () => CodeViewCallbacks): Extension[] {
  return [
    // Offsets must match the source map's lines exactly: `\n` is the only line break.
    EditorState.lineSeparator.of('\n'),
    EditorState.readOnly.of(true),
    lineNumbers(),
    gutter({
      class: 'cm-b2c-diagnostics-gutter',
      markers: (view) => view.state.field(drawnField).markers,
    }),
    highlightSpecialChars({
      specialChars: hiddenCharacterPattern(),
      render: renderHiddenCharacter,
    }),
    cpp(),
    syntaxHighlighting(cppHighlightStyle),
    codeTheme,
    drawnField,
    EditorView.contentAttributes.of((view) => {
      const { path } = codeViewData(view.state);
      return { 'aria-label': path === null ? 'Generated C++' : `Generated C++, ${path}` };
    }),
    keymap.of([{ key: 'Enter', run: (view) => selectBlockAtCaret(view, callbacks()) }]),
    EditorView.updateListener.of((update) => {
      if (!update.selectionSet) {
        return;
      }
      const selection = update.state.selection.main;
      callbacks().onSelectionChange(!selection.empty);
      // A click (not a drag that selects text) selects the block under the pointer.
      const clicked = update.transactions.some((transaction) =>
        transaction.isUserEvent('select.pointer'),
      );
      if (clicked && selection.empty) {
        const data = codeViewData(update.state);
        const block = data.path === null ? null : data.index.blockAt(data.path, selection.head);
        if (block !== null) {
          callbacks().onSelectBlock(block);
        }
      }
    }),
  ];
}
