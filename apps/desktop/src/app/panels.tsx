/**
 * What the docks show (docs/spec/04-user-interface.md §4.1): the C++ code panel, Problems, the
 * console and the build output, connected to the app's state, the editor and the build and run
 * feature. Without a project each panel holds a short note.
 *
 * The shell never loads Blockly itself (as with the compiler core, see ./core.ts): the panels use
 * the Blockly-free parts of the editor's diagnostics and highlighting, and reach the workspace only
 * through the editor handle. The console reaches the run through the build and run feature's
 * console bridge, which is Blockly-free too.
 *
 * The docks keep their panels mounted while they are hidden (./layout/EditorLayout.tsx). The C++
 * tab while its dock is collapsed, and Problems while another tab is chosen or its dock is
 * collapsed, keep what they last showed and do none of their work on an edit; each catches up
 * once when it is shown again (./store/whileShown.ts).
 */
import type { GeneratedFile } from '@blocks2cpp/b2c-core-wasm';
import {
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react';

import { pathCatalog, subscribePathCatalog } from '../editor/diagnostics/catalog';
import {
  codePanelDiagnostics,
  type DiagnosticInputs,
  diagnosticInputsFrom,
  diagnosticSources,
} from '../editor/diagnostics/inputs';
import { buildProblemItems } from '../editor/diagnostics/problemItems';
import { selectBlockFromCode, selectBlockFromProblem } from '../editor/highlight/reveal';
import { consoleBridge } from '../features/build-run/consoleBridge';
import { consoleHeaderFrom } from '../features/build-run/consoleHeader';
import { ipc } from '../lib/ipc';
import {
  BuildOutputPanel,
  CodePanel,
  type ConsoleHandle,
  ConsolePanel,
  DEFAULT_SCROLLBACK_LINES,
  isHiddenFile,
  type ProblemItem,
  ProblemsPanel,
} from '../panels';
import { useResolvedTheme } from './appearance';
import { triggerCommand } from './commands';
import { dialogs } from './dialogs/instance';
import { getEditorHandle } from './editor-types';
import { type AppState, useAppStore } from './store';
import { useAppStoreWhileShown } from './store/whileShown';

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

/** Whether the C++ dock is open, so its tab is on screen. */
function codeShown(state: AppState): boolean {
  return !state.ui.rightCollapsed;
}

/** Whether the bottom dock is open with the Problems tab chosen, so Problems is on screen. */
function problemsShown(state: AppState): boolean {
  return !state.ui.bottomCollapsed && state.ui.bottomTab === 'problems';
}

/**
 * The diagnostics to show; recomputed only when one of their sources changes while the panel is
 * `shown` (a hidden panel keeps the ones it last showed).
 */
function useDiagnosticInputs(shown: boolean): DiagnosticInputs {
  const { preview, buildDiagnostics, diagnosticsHash, contentHash } = useAppStoreWhileShown(
    shown,
    diagnosticSources,
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

/** The position of the shown module in the project's module order, or -1 without a project. */
function activeModuleIndex(state: AppState): number {
  const project = state.project;
  return project === null
    ? -1
    : project.document.modules.findIndex((module) => module.id === project.activeModuleId);
}

/**
 * The path of the generated `.cpp` of the module at `index` in the project's module order, or
 * `null` when there is none: the generator writes one source file per module, in module order,
 * before any header (`b2c_codegen::generate`). The IDE's own units are not counted.
 */
export function moduleSourcePath(files: readonly GeneratedFile[], index: number): string | null {
  if (index < 0) {
    return null;
  }
  const sources = files.filter((file) => file.kind === 'source' && !isHiddenFile(file.path));
  return sources[index]?.path ?? null;
}

/** The file picked in the C++ tab's file switcher, and the module shown when it was picked. */
interface PickedFile {
  readonly project: string | null;
  readonly module: string | null;
  readonly path: string | null;
}

/**
 * The C++ tab: the live preview's files and source map, the diagnostics in the gutter, the hovered
 * (or else the selected) block's code highlighted, and a click selecting the block that produced
 * the code. It shows the `.cpp` of the module on the canvas, and follows the module switcher: a
 * file picked in the panel's own switcher stays shown until another module is. Before there is
 * any code it shows a note. While its dock is collapsed it keeps what it showed, and takes the
 * latest once when the dock opens.
 */
export function ConnectedCodePanel() {
  const shown = useAppStore(codeShown);
  const { preview, hover, selection, project, module, moduleIndex } = useAppStoreWhileShown(
    shown,
    (state) => ({
      preview: state.analysis.preview,
      hover: state.ui.hoverBlock,
      selection: state.ui.selection,
      project: state.project?.handle ?? null,
      module: state.project?.activeModuleId ?? null,
      moduleIndex: activeModuleIndex(state),
    }),
  );
  const inputs = useDiagnosticInputs(shown);
  const diagnostics = useMemo(() => codePanelDiagnostics(inputs), [inputs]);
  const [picked, setPicked] = useState<PickedFile>({ project, module, path: null });
  // Another module (or project) shown: its own `.cpp` again, whatever was picked before. React
  // renders again at once with the new state, before anything is shown.
  if (picked.project !== project || picked.module !== module) {
    setPicked({ project, module, path: null });
  }
  const pickFile = useCallback(
    (path: string) => {
      setPicked({ project, module, path });
    },
    [project, module],
  );
  const files = preview?.files ?? NO_FILES;
  const moduleFile = useMemo(() => moduleSourcePath(files, moduleIndex), [files, moduleIndex]);
  const activePath = picked.path ?? moduleFile;
  const hasCode = useMemo(() => files.some((file) => !isHiddenFile(file.path)), [files]);

  if (preview === null || !hasCode) {
    return <EmptyPanel>The C++ for your blocks will appear here.</EmptyPanel>;
  }
  return (
    <CodePanel
      files={files}
      activePath={activePath}
      onActivePathChange={pickFile}
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
 * path. Without a project it shows a note. While it is not on screen it keeps its rows, and builds
 * them again once when it is shown.
 */
export function ConnectedProblemsPanel() {
  const shown = useAppStore(problemsShown);
  const { document } = useAppStoreWhileShown(shown, (state) => ({
    document: state.project?.document ?? null,
  }));
  const inputs = useDiagnosticInputs(shown);
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

/** How often the elapsed time of a running program is updated. */
const ELAPSED_TICK_MS = 1000;

/** `Date.now()`, updated every second while `ticking`. */
function useNow(ticking: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!ticking) {
      return;
    }
    setNow(Date.now());
    const timer = setInterval(() => {
      setNow(Date.now());
    }, ELAPSED_TICK_MS);
    return () => {
      clearInterval(timer);
    };
  }, [ticking]);
  return now;
}

function stopRun(): void {
  triggerCommand('run.stop');
}

function runAgain(): void {
  triggerCommand('run.again');
}

function consoleCleared(): void {
  consoleBridge.cleared();
}

/** Before the console's link dialog: the block editor's pop-ups close, as before any dialog. */
function closeEditorPopups(): void {
  dialogs.beforeModal();
}

/**
 * The Console tab: the running (or last) program's terminal. The header follows the run slice
 * (state, exit text, elapsed time, the *Running with IDE helpers* and *Process group only*
 * notices); ■ Stop and ⟲ Run again run their commands; the terminal's handle is attached to the
 * console bridge, through which the build and run feature writes the output and hears the
 * keystrokes and size changes.
 */
export function ConnectedConsolePanel() {
  const run = useAppStore((state) => state.run);
  const scrollbackLines = useAppStore(
    (state) => state.settings.value?.console.scrollbackLines ?? DEFAULT_SCROLLBACK_LINES,
  );
  const mode = useSyncExternalStore(consoleBridge.subscribe, consoleBridge.mode);
  const theme = useResolvedTheme();
  const now = useNow(run.status === 'running');
  const header = useMemo(() => consoleHeaderFrom(run, now), [run, now]);
  const detach = useRef<(() => void) | null>(null);
  const attach = useCallback((handle: ConsoleHandle | null) => {
    detach.current?.();
    detach.current = handle === null ? null : consoleBridge.attach(handle);
  }, []);
  useEffect(
    () => () => {
      detach.current?.();
      detach.current = null;
    },
    [],
  );

  return (
    <ConsolePanel
      header={header}
      scrollbackLines={scrollbackLines}
      onStop={stopRun}
      onRunAgain={runAgain}
      onClear={consoleCleared}
      mode={mode}
      theme={theme}
      onBeforeLinkDialog={closeEditorPopups}
      ref={attach}
    />
  );
}

/** The Build output tab: the lines of the last build, followed from each new build's start. */
export function ConnectedBuildOutputPanel() {
  const lines = useAppStore((state) => state.build.output);
  const build = useAppStore((state) => state.build.serial);
  return <BuildOutputPanel lines={lines} build={build} />;
}

/** The Console tab's slot: the console while a project is open, a note otherwise. */
function ConsoleSlot() {
  const open = useAppStore((state) => state.project !== null);
  return open ? (
    <ConnectedConsolePanel />
  ) : (
    <EmptyPanel>Your program&apos;s output will appear here.</EmptyPanel>
  );
}

/** The Build output tab's slot: the build's lines while a project is open, a note otherwise. */
function BuildOutputSlot() {
  const open = useAppStore((state) => state.project !== null);
  return open ? (
    <ConnectedBuildOutputPanel />
  ) : (
    <EmptyPanel>The compiler&apos;s messages will appear here.</EmptyPanel>
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
    console: <ConsoleSlot />,
    buildOutput: <BuildOutputSlot />,
  };
}
