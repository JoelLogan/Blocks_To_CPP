/**
 * What the docks show (docs/spec/04-user-interface.md §4.1): the C++ code panel and Problems,
 * connected to the app's state and the editor. The console and the build output arrive with the
 * build and run feature (milestone M2, wave 4); until then their slots hold a short note.
 *
 * The shell never loads Blockly itself (as with the compiler core, see ./core.ts): the panels use
 * the Blockly-free parts of the editor's diagnostics and highlighting, and reach the workspace only
 * through the editor handle.
 */
import type { GeneratedFile } from '@blocks2cpp/b2c-core-wasm';
import { type ReactNode, useMemo, useState, useSyncExternalStore } from 'react';
import { useShallow } from 'zustand/react/shallow';

import { pathCatalog, subscribePathCatalog } from '../editor/diagnostics/catalog';
import {
  codePanelDiagnostics,
  type DiagnosticInputs,
  diagnosticInputsFrom,
  diagnosticSources,
} from '../editor/diagnostics/inputs';
import { buildProblemItems } from '../editor/diagnostics/problemItems';
import { selectBlockFromCode, selectBlockFromProblem } from '../editor/highlight/reveal';
import { ipc } from '../lib/ipc';
import { CodePanel, isHiddenFile, type ProblemItem, ProblemsPanel } from '../panels';
import { getEditorHandle } from './editor-types';
import { useAppStore } from './store';

/** The content of each dock slot. */
export interface DockPanelContents {
  /** The right dock: the generated C++. */
  code: ReactNode;
  /** The bottom dock's Problems tab. */
  problems: ReactNode;
  /** The bottom dock's Console tab. */
  console: ReactNode;
  /** The bottom dock's Build output tab. */
  buildOutput: ReactNode;
}

const NO_FILES: readonly GeneratedFile[] = Object.freeze([]);

/** A note in an empty panel. */
function EmptyPanel({ children }: { children: ReactNode }) {
  return <p className="empty-panel">{children}</p>;
}

/** The diagnostics to show; recomputed only when one of their sources changes. */
function useDiagnosticInputs(): DiagnosticInputs {
  const { preview, buildDiagnostics, diagnosticsHash, contentHash } = useAppStore(
    useShallow(diagnosticSources),
  );
  return useMemo(
    () => diagnosticInputsFrom({ preview, buildDiagnostics, diagnosticsHash, contentHash }),
    [preview, buildDiagnostics, diagnosticsHash, contentHash],
  );
}

/** Selects the block that produced the clicked code (or the collapsed block around it). */
function selectFromCode(blockId: string): void {
  selectBlockFromCode(getEditorHandle(), blockId);
}

/**
 * The C++ tab: the live preview's files and source map, the diagnostics in the gutter, the hovered
 * (or else the selected) block's code highlighted, and a click selecting the block that produced
 * the code. Before there is any code it shows a note.
 */
export function ConnectedCodePanel() {
  const { preview, hover, selection } = useAppStore(
    useShallow((state) => ({
      preview: state.analysis.preview,
      hover: state.ui.hoverBlock,
      selection: state.ui.selection,
    })),
  );
  const inputs = useDiagnosticInputs();
  const diagnostics = useMemo(() => codePanelDiagnostics(inputs), [inputs]);
  const [activePath, setActivePath] = useState<string | null>(null);
  const files = preview?.files ?? NO_FILES;
  const hasCode = useMemo(() => files.some((file) => !isHiddenFile(file.path)), [files]);

  if (preview === null || !hasCode) {
    return <EmptyPanel>The C++ for your blocks will appear here.</EmptyPanel>;
  }
  return (
    <CodePanel
      files={files}
      activePath={activePath}
      onActivePathChange={setActivePath}
      sourceMap={preview.sourceMap}
      buildable={preview.buildable}
      diagnostics={diagnostics}
      highlightBlockId={hover ?? selection}
      onSelectBlock={selectFromCode}
    />
  );
}

/** Opens the diagnostics reference (the published one in M2; per-code pages come in M5). */
function openDiagnosticsReference(): void {
  ipc.openHelpLink({ linkId: 'diagnosticsReference' }).catch((error: unknown) => {
    console.warn('The diagnostics reference could not be opened', error);
  });
}

/** Activating a problem selects and centres its block (or the collapsed block around it). */
function activateProblem(item: ProblemItem): void {
  const block = item.diagnostic.primary.block;
  if (block !== undefined) {
    selectBlockFromProblem(getEditorHandle(), block);
  }
}

/** Revealing a compiler message changes nothing else: the panel shows the text itself. */
function showRawMessage(): void {
  // Nothing to do in M2; quick fixes (M3) may react here.
}

/**
 * The Problems tab: the live preview's diagnostics and the last build's, with module and block
 * path. Without a project it shows a note.
 */
export function ConnectedProblemsPanel() {
  const document = useAppStore((state) => state.project?.document ?? null);
  const inputs = useDiagnosticInputs();
  // Block paths name blocks with the catalog the editor provides; recompute when it arrives.
  const catalog = useSyncExternalStore(subscribePathCatalog, pathCatalog);
  const items = useMemo(
    () =>
      document === null
        ? []
        : buildProblemItems(inputs.live, inputs.build, inputs.stale, document, catalog),
    [document, inputs, catalog],
  );

  if (document === null) {
    return <EmptyPanel>Problems in your blocks will be listed here.</EmptyPanel>;
  }
  return (
    <ProblemsPanel
      items={items}
      onActivate={activateProblem}
      onShowRaw={showRawMessage}
      onLearnMore={openDiagnosticsReference}
    />
  );
}

/**
 * The dock slots' content. It is called during the layout's render; the connected panels follow
 * the store themselves, so the layout does not render again when the preview changes.
 */
export function DockPanels(): DockPanelContents {
  return {
    code: <ConnectedCodePanel />,
    problems: <ConnectedProblemsPanel />,
    console: <EmptyPanel>Your program&apos;s output will appear here.</EmptyPanel>,
    buildOutput: <EmptyPanel>The compiler&apos;s messages will appear here.</EmptyPanel>,
  };
}
