/**
 * The shapes of the app's state (docs/spec/04-user-interface.md §4.1): one slice per concern, all
 * plain data. Everything that comes from the backend arrives through the typed IPC client, and
 * every document comes from the compiler core's loader, never from a plain `JSON.parse`.
 */
import type { BdmDocument, PreviewResult } from '@blocks2cpp/b2c-core-wasm';
import type {
  AppInfo,
  BuildConfig,
  BuildId,
  BuildStage,
  Containment,
  Diagnostic,
  Handle,
  RunEvent,
  RunId,
  Settings,
  SettingsNotice,
  Toolchain,
  ToolchainSetupInfo,
  Trust,
} from '@blocks2cpp/ipc-types';

/** The open project (one per window in M2, 04 §4.10). */
export interface ProjectState {
  /** The backend's handle for the project. */
  handle: Handle;
  /** The file's name for display, or `null` for a project that was never saved. */
  fileName: string | null;
  /** The document as the editor currently has it (loaded by the compiler core). */
  document: BdmDocument;
  /** The canonical text of {@link document}: what a save would write. */
  canonicalText: string;
  /** The content hash of {@link canonicalText} (64 lower-case hex digits, 05 §5.11). */
  contentHash: string;
  /** The canonical text of the last save or load, or `null` when it was never saved. */
  savedCanonicalText: string | null;
  /** When it was last saved (RFC 3339 UTC), or `null`. */
  savedAt: string | null;
  /** Whether there are unsaved changes (`canonicalText !== savedCanonicalText`). */
  dirty: boolean;
  /** The trust state (08 §8.3); Restricted Mode disables Build and Run. */
  trust: Trust;
  /** The module whose canvas is shown. */
  activeModuleId: string;
  /** The format version the file had when it was migrated in memory, or `null`. */
  migratedFrom: number | null;
}

/** A notice about the live analysis that the editor shows. */
export type AnalysisNotice = 'trapRecovered' | 'syncFailed';

/** The live preview of the open project (06 §6.13). */
export interface AnalysisState {
  /** The sequence number of the latest preview request; older results are discarded. */
  seq: number;
  /** The latest preview, or `null` before the first one. */
  preview: PreviewResult | null;
  /** A notice about the analysis, or `null`. */
  notice: AnalysisNotice | null;
}

/** Where a build is. */
export type BuildStatus = 'idle' | 'building' | 'succeeded' | 'failed' | 'cancelled';

/** The progress of the running build. */
export interface BuildProgress {
  stage: BuildStage;
  done: number;
  total: number;
}

/** One line of the Build output tab. */
export interface BuildOutputLine {
  /** Progress, a toolchain note (such as B2C-T1011) or raw compiler text. */
  kind: 'progress' | 'note' | 'raw';
  text: string;
}

/** The last build that produced a program, which Run can start without building again. */
export interface BuildSuccess {
  buildId: BuildId;
  /** The content hash of the project that was built. */
  projectHash: string;
  config: BuildConfig;
}

/** The build session of the open project. */
export interface BuildState {
  /** The current or last build, or `null`. */
  buildId: BuildId | null;
  status: BuildStatus;
  /** The configuration for this session: Debug or Release, never saved (04 §4.1). */
  config: BuildConfig;
  progress: BuildProgress | null;
  /** The diagnostics of the last build (compiler and linker ones are mapped to blocks). */
  diagnostics: Diagnostic[];
  /** The `projectHash` those diagnostics belong to, so they can be marked stale. */
  diagnosticsHash: string | null;
  /** The Build output tab's lines, oldest first, at most {@link MAX_BUILD_OUTPUT_LINES}. */
  output: BuildOutputLine[];
  lastSuccess: BuildSuccess | null;
}

/** The `exit` event of a run. */
export type RunExitEvent = Extract<RunEvent, { kind: 'exit' }>;

/** Where the program is. */
export type RunStatus = 'idle' | 'starting' | 'running' | 'exited';

/** The run session of the open project. */
export interface RunState {
  runId: RunId | null;
  status: RunStatus;
  /** When it started (`Date.now()`), or `null`. */
  startedAt: number | null;
  /** How it ended, once it has. */
  exit: RunExitEvent | null;
  /** How its process tree is contained, once it has started. */
  containment: Containment | null;
  /** Whether the IDE helper unit was linked in. */
  ideHelpers: boolean;
}

/** The compilers the backend found (07 §7.2). */
export interface ToolchainsState {
  /** In discovery order. */
  list: Toolchain[];
  /** Whether background discovery is still running. */
  discovering: boolean;
  /** What the setup page needs, once asked for. */
  setupInfo: ToolchainSetupInfo | null;
}

/** The machine settings (05 §5.9). */
export interface SettingsState {
  /** The settings in effect, or `null` before they were read. */
  value: Settings | null;
  /** What was reset or left alone when they were read. */
  notices: SettingsNotice[];
}

/** The bottom dock's tabs. */
export type BottomTab = 'console' | 'problems' | 'buildOutput';

/** The full-window pages; `editor` is the block editor itself. */
export type ScreenId = 'start' | 'editor' | 'toolchainSetup' | 'settings';

/** Window-level user-interface state. */
export interface UiState {
  /** The selected block's ID, or `null`. */
  selection: string | null;
  /** The block under the pointer (for two-way highlighting), or `null`. */
  hoverBlock: string | null;
  bottomTab: BottomTab;
  bottomCollapsed: boolean;
  rightCollapsed: boolean;
  screen: ScreenId;
}

/** The most lines the Build output tab keeps; older lines are dropped first. */
export const MAX_BUILD_OUTPUT_LINES = 10_000;

/** The longest Build output line kept, in UTF-16 code units; longer lines are cut. */
export const MAX_BUILD_OUTPUT_LINE_LENGTH = 16_384;

/** The data of every slice, without the actions. */
export interface AppData {
  /** What `app_info` reported, or `null` before it answered (or outside the desktop app). */
  appInfo: AppInfo | null;
  project: ProjectState | null;
  analysis: AnalysisState;
  build: BuildState;
  run: RunState;
  toolchains: ToolchainsState;
  settings: SettingsState;
  ui: UiState;
}

/** The analysis slice with nothing analysed yet. */
export function initialAnalysis(): AnalysisState {
  return { seq: 0, preview: null, notice: null };
}

/** The build slice with no build yet. */
export function initialBuild(config: BuildConfig = 'debug'): BuildState {
  return {
    buildId: null,
    status: 'idle',
    config,
    progress: null,
    diagnostics: [],
    diagnosticsHash: null,
    output: [],
    lastSuccess: null,
  };
}

/** The run slice with no program started. */
export function initialRun(): RunState {
  return {
    runId: null,
    status: 'idle',
    startedAt: null,
    exit: null,
    containment: null,
    ideHelpers: false,
  };
}

/** The state of a freshly started window. */
export function initialAppData(): AppData {
  return {
    appInfo: null,
    project: null,
    analysis: initialAnalysis(),
    build: initialBuild(),
    run: initialRun(),
    toolchains: { list: [], discovering: false, setupInfo: null },
    settings: { value: null, notices: [] },
    ui: {
      selection: null,
      hoverBlock: null,
      bottomTab: 'console',
      bottomCollapsed: false,
      rightCollapsed: false,
      screen: 'start',
    },
  };
}
