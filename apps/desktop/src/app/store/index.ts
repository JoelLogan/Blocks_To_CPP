/**
 * The app's state: one Zustand store with a slice per concern (docs/spec/04-user-interface.md
 * §4.1) and setters for each slice under `actions`.
 *
 * ```tsx
 * const dirty = useAppStore((state) => state.project?.dirty ?? false);
 * useAppStore.getState().actions.setUi({ bottomTab: 'problems' });
 * ```
 *
 * Selectors that build a new object on every call must be wrapped in `useShallow`
 * (`zustand/react/shallow`), or the component renders forever.
 */
import { create } from 'zustand';

import {
  type AnalysisState,
  type AppData,
  type BuildOutputLine,
  type BuildState,
  initialAnalysis,
  initialAppData,
  initialBuild,
  initialRun,
  MAX_BUILD_OUTPUT_LINE_LENGTH,
  MAX_BUILD_OUTPUT_LINES,
  type ProjectState,
  type RunState,
  type SettingsState,
  type ToolchainsState,
  type UiState,
} from './state';

export * from './state';

/** The setters, one or more per slice. Patches are shallow: nested objects are replaced whole. */
export interface AppActions {
  /** Sets what `app_info` reported. */
  setAppInfo: (appInfo: AppData['appInfo']) => void;
  /**
   * Sets (or, with `null`, clears) the open project. When the handle changes, the slices that
   * describe the previous project's session are reset too: the analysis, the build (keeping the
   * session's Debug/Release choice), the run, and the selected and hovered block.
   */
  setProject: (project: ProjectState | null) => void;
  /** Merges into the open project; does nothing when no project is open. */
  updateProject: (patch: Partial<ProjectState>) => void;
  /** Merges into the analysis slice. */
  setAnalysis: (patch: Partial<AnalysisState>) => void;
  /** Merges into the build slice. */
  setBuild: (patch: Partial<BuildState>) => void;
  /**
   * Appends lines to the Build output tab, cutting lines longer than
   * {@link MAX_BUILD_OUTPUT_LINE_LENGTH} and keeping at most {@link MAX_BUILD_OUTPUT_LINES}.
   */
  appendBuildOutput: (lines: readonly BuildOutputLine[]) => void;
  /** Merges into the run slice. */
  setRun: (patch: Partial<RunState>) => void;
  /** Merges into the toolchains slice. */
  setToolchains: (patch: Partial<ToolchainsState>) => void;
  /** Merges into the settings slice. */
  setSettings: (patch: Partial<SettingsState>) => void;
  /** Merges into the user-interface slice. */
  setUi: (patch: Partial<UiState>) => void;
}

/** The whole state: the data of every slice and the actions. */
export type AppState = AppData & { actions: AppActions };

/** Cuts a line that is too long, marking the cut with an ellipsis. */
function boundLine(line: BuildOutputLine): BuildOutputLine {
  if (line.text.length <= MAX_BUILD_OUTPUT_LINE_LENGTH) {
    return line;
  }
  return { kind: line.kind, text: `${line.text.slice(0, MAX_BUILD_OUTPUT_LINE_LENGTH - 1)}…` };
}

/** The app's store. Use it as a hook in components and with `getState()` elsewhere. */
export const useAppStore = create<AppState>()((set) => ({
  ...initialAppData(),
  actions: {
    setAppInfo: (appInfo) => {
      set({ appInfo });
    },
    setProject: (project) => {
      set((state) => {
        if (state.project?.handle === project?.handle) {
          return { project };
        }
        return {
          project,
          analysis: initialAnalysis(),
          build: initialBuild(state.build.config),
          run: initialRun(),
          ui: { ...state.ui, selection: null, hoverBlock: null },
        };
      });
    },
    updateProject: (patch) => {
      set((state) => (state.project === null ? {} : { project: { ...state.project, ...patch } }));
    },
    setAnalysis: (patch) => {
      set((state) => ({ analysis: { ...state.analysis, ...patch } }));
    },
    setBuild: (patch) => {
      set((state) => ({ build: { ...state.build, ...patch } }));
    },
    appendBuildOutput: (lines) => {
      if (lines.length === 0) {
        return;
      }
      set((state) => {
        const added = lines.slice(-MAX_BUILD_OUTPUT_LINES).map(boundLine);
        const kept = state.build.output.slice(
          Math.max(0, state.build.output.length + added.length - MAX_BUILD_OUTPUT_LINES),
        );
        return { build: { ...state.build, output: [...kept, ...added] } };
      });
    },
    setRun: (patch) => {
      set((state) => ({ run: { ...state.run, ...patch } }));
    },
    setToolchains: (patch) => {
      set((state) => ({ toolchains: { ...state.toolchains, ...patch } }));
    },
    setSettings: (patch) => {
      set((state) => ({ settings: { ...state.settings, ...patch } }));
    },
    setUi: (patch) => {
      set((state) => ({ ui: { ...state.ui, ...patch } }));
    },
  },
}));

/** Puts every slice back to its initial value (the actions stay). For tests and a fresh start. */
export function resetAppStore(): void {
  useAppStore.setState(initialAppData());
}
