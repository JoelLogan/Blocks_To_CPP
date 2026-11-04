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
import { BugReportDialog, type BugReportInput } from '../features/bug-report';
import { GENERATOR_BUG_LABEL } from '../features/build-run/buildOutput';
import { consoleBridge } from '../features/build-run/consoleBridge';
import { consoleHeaderFrom } from '../features/build-run/consoleHeader';
import { ipc } from '../lib/ipc';
import { visibleInvisibles } from '../panels/shared/invisibles';
import {
  BuildOutputPanel,
  CodePanel,
  type ConsoleHandle,
  ConsolePanel,
  DEFAULT_SCROLLBACK_LINES,
  isHiddenFile,
  type ProblemFix,
  type CodeFileGroup,
  type ProblemItem,
  ProblemsPanel,
} from '../panels';
import { useResolvedTheme } from './appearance';
import { triggerCommand } from './commands';
import { dialogs } from './dialogs/instance';
import { getEditorHandle } from './editor-types';
import { quickFixesOf } from './quickFixes';
import { type AppState, useAppStore } from './store';
import { selectedToolchain } from './store/selectors';
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

/**
 * Selects the block that produced the clicked code (or the collapsed block around it), switching
 * to its module when it is another's: a header's declaration selects its *define* (w3-modules.md
 * §2.5).
 */
function selectFromCode(blockId: string): void {
  selectBlockFromCode(getEditorHandle(), blockId);
}

/** A module of the open project, as the C++ tab groups its files. */
interface ModuleOfFiles {
  readonly id: string;
  readonly name: string;
}

/** The modules of the open project in module order (their IDs and names, as one string). */
function modulesKey(state: AppState): string {
  return JSON.stringify(
    (state.project?.document.modules ?? []).map((module) => [module.id, module.name]),
  );
}

/**
 * The `.cpp` of module `moduleId` among the generated files: the source whose `module` key is the
 * module (06 §6.8, w3-modules.md §2.5), or `null` when there is none. The IDE's own units are not
 * counted.
 */
export function moduleSourcePath(
  files: readonly GeneratedFile[],
  moduleId: string | null,
): string | null {
  if (moduleId === null) {
    return null;
  }
  return (
    files.find(
      (file) => file.kind === 'source' && file.module === moduleId && !isHiddenFile(file.path),
    )?.path ?? null
  );
}

/**
 * The C++ tab's file list (w3-modules.md §1.2, §2.5): the shown module's files first, then each
 * other module's under its name in module order, then the files of no module
 * (`b2c_support.hpp`). A file whose module the project does not have goes with those.
 */
export function codeFileGroups(
  files: readonly GeneratedFile[],
  modules: readonly ModuleOfFiles[],
  shown: string | null,
): CodeFileGroup[] {
  const visible = files.filter((file) => !isHiddenFile(file.path));
  const known = new Set(modules.map((module) => module.id));
  const ofModule = (id: string) => visible.filter((file) => file.module === id).map((f) => f.path);
  const groups: CodeFileGroup[] = [];
  if (shown !== null && known.has(shown)) {
    groups.push({ label: null, paths: ofModule(shown) });
  }
  for (const module of modules) {
    if (module.id !== shown) {
      const paths = ofModule(module.id);
      if (paths.length > 0) {
        groups.push({ label: `Module ${visibleInvisibles(module.name)}`, paths });
      }
    }
  }
  groups.push({
    label: null,
    paths: visible
      .filter((file) => file.module === undefined || !known.has(file.module))
      .map((file) => file.path),
  });
  return groups.filter((group) => group.paths.length > 0);
}

/** The file picked in the C++ tab's file switcher for each module of the project shown. */
interface PickedFiles {
  readonly project: string | null;
  /** The path picked while each module was shown, by module ID. */
  readonly byModule: Readonly<Record<string, string>>;
}

/**
 * The C++ tab: the live preview's files and source map, the diagnostics in the gutter, the hovered
 * (or else the selected) block's code highlighted, and a click selecting the block that produced
 * the code. It shows the `.cpp` of the module on the canvas (the file whose `module` key is it),
 * and follows the module strip; a file picked in the panel's own switcher is kept for the module
 * it was picked in, and a picked file the module no longer has (a header that went) falls back to
 * the module's `.cpp`. Its file list has the shown module's files first, then the other modules'
 * under their names, then `b2c_support.hpp`. Before there is any code it shows a note. While its
 * dock is collapsed it keeps what it showed, and takes the latest once when the dock opens.
 */
export function ConnectedCodePanel() {
  const shown = useAppStore(codeShown);
  const { preview, hover, selection, project, module, modules } = useAppStoreWhileShown(
    shown,
    (state) => ({
      preview: state.analysis.preview,
      hover: state.ui.hoverBlock,
      selection: state.ui.selection,
      project: state.project?.handle ?? null,
      module: state.project?.activeModuleId ?? null,
      modules: modulesKey(state),
    }),
  );
  const inputs = useDiagnosticInputs(shown);
  const diagnostics = useMemo(() => codePanelDiagnostics(inputs), [inputs]);
  const [picked, setPicked] = useState<PickedFiles>({ project, byModule: {} });
  // Another project: nothing picked. React renders again at once with the new state.
  if (picked.project !== project) {
    setPicked({ project, byModule: {} });
  }
  const pickFile = useCallback(
    (path: string) => {
      if (module !== null) {
        setPicked((before) => ({ project, byModule: { ...before.byModule, [module]: path } }));
      }
    },
    [project, module],
  );
  const files = preview?.files ?? NO_FILES;
  // Code clicked in another module's file selects a block of that module, which shows it: the
  // file stays shown, as that module's pick.
  const selectCode = useCallback(
    (blockId: string, path: string | null) => {
      const owner = files.find((file) => file.path === path)?.module;
      if (path !== null && owner !== undefined && owner !== module) {
        setPicked((before) => ({ project, byModule: { ...before.byModule, [owner]: path } }));
      }
      selectFromCode(blockId);
    },
    [files, module, project],
  );
  const moduleList = useMemo(
    () =>
      (JSON.parse(modules) as [string, string][]).map(([id, name]): ModuleOfFiles => ({
        id,
        name,
      })),
    [modules],
  );
  const fileGroups = useMemo(
    () => codeFileGroups(files, moduleList, module),
    [files, moduleList, module],
  );
  const moduleFile = useMemo(() => moduleSourcePath(files, module), [files, module]);
  const pickedPath = module === null ? undefined : picked.byModule[module];
  const activePath =
    pickedPath !== undefined && files.some((file) => file.path === pickedPath)
      ? pickedPath
      : moduleFile;
  const hasCode = useMemo(() => files.some((file) => !isHiddenFile(file.path)), [files]);

  if (preview === null || !hasCode) {
    return <EmptyPanel>The C++ for your blocks will appear here.</EmptyPanel>;
  }
  return (
    <CodePanel
      files={files}
      fileGroups={fileGroups}
      activePath={activePath}
      onActivePathChange={pickFile}
      sourceMap={preview.sourceMap}
      buildable={preview.buildable}
      diagnostics={diagnostics}
      highlightBlockId={hover ?? selection}
      onSelectBlock={selectCode}
    />
  );
}

/** Opens the diagnostics reference (the published one in M2; per-code pages come in M5). */
function openDiagnosticsReference(): void {
  ipc.openHelpLink({ linkId: 'diagnosticsReference' }).catch((error: unknown) => {
    console.warn('The diagnostics reference could not be opened', error);
  });
}

/**
 * Activating a problem shows its module when it is another (w3-modules.md §2.6), then selects and
 * centres its block (or the collapsed block around it).
 */
function activateProblem(item: ProblemItem): void {
  const { module, block } = item.diagnostic.primary;
  const project = useAppStore.getState().project;
  if (
    module !== undefined &&
    project !== null &&
    project.activeModuleId !== module &&
    project.document.modules.some((candidate) => candidate.id === module)
  ) {
    // The editing session follows `activeModuleId` and shows that module's canvas.
    useAppStore.getState().actions.updateProject({ activeModuleId: module });
  }
  if (block !== undefined) {
    selectBlockFromProblem(getEditorHandle(), block);
  }
}

/** Revealing a compiler message changes nothing else: the panel shows the text itself. */
function showRawMessage(): void {
  // Nothing to do: the panel shows the text, and a generator bug's row offers its report.
}

/** A live row's quick fix on the canvas shown (w4-editor-tools.md §2.5), or `null`. */
function fixOfProblem(item: ProblemItem): ProblemFix | null {
  return item.origin === 'live' ? (quickFixesOf(item.diagnostic)[0] ?? null) : null;
}

/**
 * Whether a row is a generator bug, which offers *Copy bug report…* (07 §7.5.3): not once the
 * project has changed since its build (a stale row), when the C++ the report would hold is no
 * longer the C++ g++ compiled.
 */
function reportsBug(item: ProblemItem): boolean {
  return !item.stale && item.diagnostic.message.includes(GENERATOR_BUG_LABEL);
}

/**
 * What the bug report of a row is made from: the project now (a row that reports a bug is not
 * stale, so its generated C++ is the C++ that was built) and, for a build's row, the configuration
 * it was built in.
 */
function bugReportInput(item: ProblemItem): BugReportInput {
  const state = useAppStore.getState();
  const project = state.project?.document ?? null;
  return {
    appInfo: state.appInfo,
    toolchain: selectedToolchain(state),
    standard: project?.project.language.standard ?? null,
    configuration:
      item.origin === 'build'
        ? (state.build.diagnosticsConfig ?? state.build.config)
        : state.build.config,
    diagnostic: item.diagnostic,
    document: project,
    files: state.analysis.preview?.files ?? [],
  };
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

  const [report, setReport] = useState<BugReportInput | null>(null);

  if (document === null) {
    return <EmptyPanel>Problems in your blocks will be listed here.</EmptyPanel>;
  }
  return (
    <>
      <ProblemsPanel
        items={items}
        onActivate={activateProblem}
        onShowRaw={showRawMessage}
        onLearnMore={openDiagnosticsReference}
        fixOf={fixOfProblem}
        reportsBug={reportsBug}
        onCopyBugReport={(item) => {
          setReport(bugReportInput(item));
        }}
      />
      {report !== null && (
        <BugReportDialog
          input={report}
          onClose={() => {
            setReport(null);
          }}
        />
      )}
    </>
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

function endInput(): void {
  triggerCommand('run.endInput');
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
      onEndInput={endInput}
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
