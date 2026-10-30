/**
 * The toolbox editor plugin (docs/spec/04-user-interface.md §4.2): installs the Blocks2Cpp toolbox
 * on the workspace, registers the dynamic categories (Program, Variables, Functions with *Your
 * functions*) and *Make a variable*, settles the blocks dragged out of it where they land
 * (./landing.ts), gives the menus of blocks still in its flyout the module's variables, and keeps
 * start values in step with types, on the canvas and in its flyout (./reshape.ts).
 *
 * The dynamic categories do not depend on the selection (decision D2 of the M2 polish plan). They
 * are rebuilt only when what they show changes ({@link dynamicContentsKey}: where the program
 * starts, the module's variables and functions, the modules), which is checked after each analysis
 * and when the project's modules or the module shown change. Selecting blocks never rebuilds them.
 *
 * Blockly refreshes no toolbox while a gesture is in progress, so a rebuild asked for while the
 * pointer is pressed waits until the gesture has ended (the pointer released, or the press
 * cancelled) instead of being lost. The continuous toolbox also waits while a block is dragged, a
 * field editor is open or the flyout scrolls to a category (./continuous.ts).
 *
 * The plugin works with either toolbox the workspace was injected with: the continuous toolbox
 * (see register.ts, `toolboxInjectOptions`) or Blockly's category toolbox. The workspace needs a
 * category toolbox to start with; `toolboxInjectOptions()` gives one.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { setToolboxSymbols } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import { dialogs as appDialogs } from '../../app/dialogs/instance';
import type { DialogService, DialogTextOptions } from '../../app/dialogs/service';
import type { EditorContext, EditorPlugin } from '../../app/editor-types';
import { editorMode, modeKey } from '../gating';
import { searchBlocks, searchEntries, type SearchEntry } from '../search';
import { analysisScope } from '../services/symbols';
import { handleBlockDrops } from '../sync/drag';
import {
  B2C_CATEGORY_KIND,
  MAKE_VARIABLE_BUTTON,
  MY_BLOCKS_CATEGORY,
  PROGRAM_CATEGORY,
  VARIABLES_CATEGORY,
  dynamicContentsKey,
  failedCategoryNote,
  functionsContents,
  initialToolboxDefinition,
  programContents,
  toolboxDefinition,
  toolboxSymbolGroups,
  variablesContents,
  withLabelMode,
  type ContentsContext,
  type ModuleInfo,
} from './contents';
import { B2cContinuousToolbox } from './continuous';
import { settleDroppedBlock, symbolType } from './landing';
import { makeVariable, type MakeVariableResult } from './makeVariable';
import { registerToolboxComponents } from './register';
import { installStartValueReshaping } from './reshape';
import { MAIN_TYPE, type SymbolSource } from './scope';
import { SelectionTracker } from './selection';
import './toolbox.css';

/** The app store, as editor plugins get it. */
type Store = EditorContext['store'];

/** The dialogs the toolbox asks with. */
type ToolboxDialogs = Pick<DialogService, 'prompt' | 'alert' | 'choose'>;

/** Options of {@link createToolboxPlugin}. */
export interface ToolboxPluginOptions {
  /** The dialogs *Make a variable* uses; the app's by default. */
  readonly dialogs?: ToolboxDialogs;
  /** Called with the outcome of each *Make a variable* (tests, logging). */
  readonly onMakeVariable?: (result: MakeVariableResult) => void;
}

/**
 * The symbols of the latest analysis, as the editor's symbol provider reads them
 * (`analysisScope`, ../services/symbols.ts): the scope query of the running compiler core (the
 * shell's current core, which replaces a trapped one) and the preview's symbol list. Before the
 * project's first analysis there are none. Never throws.
 *
 * The toolbox resolves the positions it asks about itself (./scope.ts), so it reads the analysis
 * without the provider's own resolution of each block.
 */
export function analysisSymbolSource(store: Store, core: () => CoreWasm | null): SymbolSource {
  return analysisScope({ core, preview: () => store.getState().analysis.preview });
}

/** The project's modules, in document order. */
function modulesOf(store: Store): readonly ModuleInfo[] {
  return (store.getState().project?.document.modules ?? []).map(({ id, name }) => ({ id, name }));
}

/** Whether a module other than `active` has `main`. */
function mainInOtherModule(store: Store, active: string): boolean {
  return programModuleElsewhere(store, active) !== null;
}

/** The first module other than `active` whose top-level blocks include `main`, or `null`. */
function programModuleElsewhere(store: Store, active: string): string | null {
  const modules = store.getState().project?.document.modules ?? [];
  return (
    modules.find(
      (module) =>
        module.id !== active && module.workspace.blocks.some((block) => block.type === MAIN_TYPE),
    )?.id ?? null
  );
}

/**
 * The module that has *when program starts*: the module shown when its canvas has one (enabled or
 * not), else the first other module whose document has one; `null` while the project has none.
 */
function programModule(store: Store, workspace: Blockly.Workspace, active: string): string | null {
  if (workspace.getTopBlocks(false).some((block) => block.type === MAIN_TYPE)) {
    return active;
  }
  return programModuleElsewhere(store, active);
}

/**
 * Lends Blockly's keyboard focus to the open dialog and gets it back when it closes, as Blockly
 * expects of dialogs opened from its own controls (the *Make a variable* button).
 */
const LEND_BLOCKLY_FOCUS: Pick<DialogTextOptions, 'onOpen'> = {
  onOpen(content) {
    const manager = Blockly.getFocusManager();
    if (manager.ephemeralFocusTaken()) {
      return undefined;
    }
    try {
      return manager.takeEphemeralFocus(content);
    } catch (error: unknown) {
      console.warn('Blockly did not lend its focus to the dialog', error);
      return undefined;
    }
  },
};

/** Dialogs that lend Blockly's focus to each dialog they open. */
function withBlocklyFocus(dialogs: ToolboxDialogs): ToolboxDialogs {
  return {
    prompt: (options) => dialogs.prompt({ ...LEND_BLOCKLY_FOCUS, ...options }),
    alert: (options) => dialogs.alert({ ...LEND_BLOCKLY_FOCUS, ...options }),
    choose: (options) => dialogs.choose({ ...LEND_BLOCKLY_FOCUS, ...options }),
  };
}

/**
 * Builds a dynamic category, its blocks marked with the label mode ({@link withLabelMode}), showing
 * a short note instead of failing the whole flyout.
 */
function guarded(
  name: string,
  build: (context: ContentsContext) => Blockly.utils.toolbox.FlyoutItemInfo[],
  context: ContentsContext,
): () => Blockly.utils.toolbox.FlyoutItemInfoArray {
  return () => {
    try {
      return withLabelMode(build(context), context.mode?.() ?? null);
    } catch (error: unknown) {
      console.error(`The toolbox category ${name} could not be built`, error);
      return failedCategoryNote();
    }
  };
}

/** Puts a toolbox definition on the workspace; false (with a log) when it cannot. */
function setToolbox(
  workspace: Blockly.WorkspaceSvg,
  definition: Blockly.utils.toolbox.ToolboxInfo,
): boolean {
  if (workspace.getToolbox() === null) {
    console.warn(
      'The workspace was injected without a category toolbox (see toolboxInjectOptions), so the ' +
        'Blocks2Cpp toolbox cannot be shown.',
    );
    return false;
  }
  try {
    workspace.updateToolbox(definition);
    return true;
  } catch (error: unknown) {
    console.error('The toolbox could not be updated', error);
    return false;
  }
}

/** The dynamic categories and their callback keys. */
const DYNAMIC_CATEGORIES: readonly {
  readonly key: string;
  readonly name: string;
  readonly build: (context: ContentsContext) => Blockly.utils.toolbox.FlyoutItemInfo[];
}[] = [
  { key: PROGRAM_CATEGORY, name: 'Program', build: programContents },
  { key: VARIABLES_CATEGORY, name: 'Variables', build: variablesContents },
  { key: MY_BLOCKS_CATEGORY, name: 'Functions', build: functionsContents },
];

/** The contents context of each workspace with the toolbox attached (search, Quick Insert). */
const CONTEXTS = new WeakMap<Blockly.Workspace, ContentsContext>();

/**
 * What the toolbox of `workspace` builds its categories from (the module shown, its symbols, the
 * mode), while the toolbox is attached; `null` otherwise. Quick Insert and toolbox search read the
 * same blocks the toolbox offers through it (../search/index.ts).
 */
export function toolboxContentsContext(workspace: Blockly.Workspace): ContentsContext | null {
  return CONTEXTS.get(workspace) ?? null;
}

/** The toolbox search of each workspace with the toolbox attached ({@link ToolboxSearch}). */
const SEARCHES = new WeakMap<Blockly.Workspace, ToolboxSearch>();

/**
 * Toolbox search (docs/spec/04-user-interface.md §4.2, docs/plans/m3-contracts/w4-editor-tools.md
 * §2.4): while a query is set, the toolbox shows one category, *Search results*, with the blocks
 * that match it, best first (the toolbox's own items, so they are dragged out as from their
 * category); without one, its categories again. The results follow what the toolbox offers: when
 * the module's variables or functions, the module shown or the mode change while a query is set,
 * the search runs again (once a gesture in progress has ended), so a result never names a symbol
 * that is gone.
 */
export interface ToolboxSearch {
  /** Shows the blocks that match `query`, or with `null` the categories again. */
  setQuery(query: string | null): void;
  /** The query shown, or `null`. */
  query(): string | null;
  /** How many blocks the results show (0 without a query). */
  resultCount(): number;
  /**
   * Calls `listener` each time the results are built again for the same query (what the toolbox
   * offers changed); returns the function that stops it.
   */
  onResultsChanged(listener: () => void): () => void;
}

/** The toolbox search of `workspace`, while its toolbox is attached. */
export function toolboxSearch(workspace: Blockly.Workspace): ToolboxSearch | null {
  return SEARCHES.get(workspace) ?? null;
}

/** The name of the category that shows the search results. */
export const SEARCH_RESULTS_NAME = 'Search results';
/** What the results say when no block matches. */
export const NO_RESULTS_NOTE = 'No block matches.';
/** The most blocks the results show. */
export const MAX_SEARCH_RESULTS = 40;

/** Attaches the toolbox to the editor's workspace; returns the function that detaches it. */
function attachToolbox(context: EditorContext, options: ToolboxPluginOptions): () => void {
  registerToolboxComponents();
  const { workspace, store } = context;
  const dialogs = withBlocklyFocus(options.dialogs ?? appDialogs);
  const symbols = analysisSymbolSource(store, () => context.core());
  const selection = new SelectionTracker(workspace);
  const contentsContext: ContentsContext = {
    workspace,
    symbols,
    modules: () => modulesOf(store),
    activeModuleId: () => context.activeModuleId(),
    programModuleId: () => programModule(store, workspace, context.activeModuleId()),
    mode: () => editorMode(store.getState()),
  };
  CONTEXTS.set(workspace, contentsContext);
  /** What the dynamic categories show depends on, now; never throws. */
  const contentsKey = (): string | null => {
    try {
      return dynamicContentsKey(contentsContext);
    } catch (error: unknown) {
      console.error('The toolbox could not tell whether its contents changed', error);
      return null;
    }
  };
  /** The key the dynamic categories were last built for (`null`: build them at the next check). */
  let builtKey: string | null = null;
  let detached = false;
  let refreshTimer: ReturnType<typeof setTimeout> | null = null;
  let making = false;
  /** The document whose pointer releases end a gesture (the workspace's). */
  const pointerDocument =
    (workspace.getInjectionDiv() as HTMLElement | null)?.ownerDocument ?? document;
  /** Whether a refresh waits for the gesture in progress to end ({@link waitForGestureEnd}). */
  let waitingForGesture = false;
  /**
   * Whether the search results were built for contents that have changed since: the next refresh
   * runs the search again ({@link refreshIfChanged}).
   */
  let resultsStale = false;
  /** Who hears that the results were built again ({@link ToolboxSearch.onResultsChanged}). */
  const resultsListeners = new Set<() => void>();

  /** No refresh waits for a gesture any more. */
  const stopWaitingForGesture = (): void => {
    if (waitingForGesture) {
      waitingForGesture = false;
      pointerDocument.removeEventListener('pointerup', onGestureEnd, true);
      pointerDocument.removeEventListener('pointercancel', onGestureEnd, true);
    }
  };
  /**
   * The pointer was released or the press cancelled: refresh once Blockly has ended the gesture.
   * Blockly ends it in its own listener for the same event, after this one (which listens in the
   * capture phase, as Blockly stops the event there); the refresh comes in a later task, and waits
   * again if a gesture is still in progress then (another finger still down).
   */
  function onGestureEnd(): void {
    stopWaitingForGesture();
    scheduleRefresh();
  }
  /** Refreshes the toolbox once the gesture in progress has ended. */
  const waitForGestureEnd = (): void => {
    if (!waitingForGesture) {
      waitingForGesture = true;
      pointerDocument.addEventListener('pointerup', onGestureEnd, true);
      pointerDocument.addEventListener('pointercancel', onGestureEnd, true);
    }
  };

  /**
   * Rebuilds the dynamic categories: the continuous toolbox rebuilds its flyout a little later
   * and measures that as the User Timing span `b2c:toolbox-refresh` (continuous.ts), Blockly's
   * category toolbox rebuilds the open dynamic category at once. While a gesture is in progress
   * Blockly would ignore the refresh, so it waits for the gesture to end.
   */
  const refresh = (): void => {
    if (Blockly.Gesture.inProgress()) {
      if (!detached) {
        waitForGestureEnd();
      }
      return;
    }
    if (resultsStale) {
      resultsStale = false;
      searchAgain();
    }
    try {
      workspace.refreshToolboxSelection();
    } catch (error: unknown) {
      console.error('The toolbox could not be refreshed', error);
    }
  };
  /** Rebuilds the open dynamic categories once the current burst of changes is over. */
  const scheduleRefresh = (): void => {
    if (detached || refreshTimer !== null) {
      return;
    }
    refreshTimer = setTimeout(() => {
      refreshTimer = null;
      if (!detached) {
        refresh();
      }
    }, 0);
  };
  /**
   * Rebuilds the dynamic categories when what they show changed since they were last built, and
   * the search results with them while a query is set.
   */
  const refreshIfChanged = (): void => {
    const key = contentsKey();
    if (key === null || key !== builtKey) {
      builtKey = key;
      if (searchQuery !== null) {
        resultsStale = true;
      }
      scheduleRefresh();
    }
  };

  for (const category of DYNAMIC_CATEGORIES) {
    workspace.registerToolboxCategoryCallback(
      category.key,
      guarded(category.name, category.build, contentsContext),
    );
  }
  workspace.registerButtonCallback(MAKE_VARIABLE_BUTTON, () => {
    if (detached || making) {
      return;
    }
    making = true;
    makeVariable({
      workspace,
      symbols,
      dialogs,
      selected: () => selection.current(),
      mainInOtherModule: () => mainInOtherModule(store, context.activeModuleId()),
    })
      .then((result) => {
        options.onMakeVariable?.(result);
      })
      .catch((error: unknown) => {
        console.error('Make a variable failed', error);
      })
      .finally(() => {
        making = false;
      });
  });

  const toolbox = workspace.getToolbox();
  if (toolbox instanceof B2cContinuousToolbox) {
    toolbox.setContentsKey(() => dynamicContentsKey(contentsContext));
  }
  /** The mode the toolbox definition was installed for (`modeKey`). */
  let installedMode = modeKey(editorMode(store.getState()));
  /** The toolbox definition installed last, serialised. */
  let installedDefinition = '';
  /** The toolbox search's query, or `null`. */
  let searchQuery: string | null = null;
  let searchCount = 0;
  /**
   * The search entries and the contents key they were built for: the entries are built again only
   * when the key (which holds the mode) changes, not for each query (w4-editor-tools.md §2.3).
   */
  let searchIndex: { readonly key: string; readonly entries: readonly SearchEntry[] } | null = null;
  /** The search entries for what the toolbox offers now. */
  const currentEntries = (): readonly SearchEntry[] => {
    const key = contentsKey();
    if (key !== null && searchIndex?.key === key) {
      return searchIndex.entries;
    }
    const entries = searchEntries(contentsContext);
    searchIndex = key === null ? null : { key, entries };
    return entries;
  };
  /** The definition with the search results for `query`. */
  const resultsDefinition = (query: string): Blockly.utils.toolbox.ToolboxInfo => {
    const found = searchBlocks(currentEntries(), query, MAX_SEARCH_RESULTS);
    searchCount = found.length;
    const contents: Blockly.utils.toolbox.FlyoutItemInfo[] =
      found.length === 0
        ? [{ kind: 'label', text: NO_RESULTS_NOTE, id: undefined }]
        : found.map((entry) => entry.item);
    const category = {
      kind: B2C_CATEGORY_KIND,
      name: SEARCH_RESULTS_NAME,
      id: undefined,
      categorystyle: undefined,
      colour: undefined,
      cssconfig: undefined,
      hidden: undefined,
      contents,
    };
    return { kind: 'categoryToolbox', contents: [category] };
  };
  /** The definition the toolbox shows now: the search results, or the categories of the mode. */
  const currentDefinition = (): Blockly.utils.toolbox.ToolboxInfo =>
    searchQuery === null
      ? toolboxDefinition(editorMode(store.getState()))
      : resultsDefinition(searchQuery);
  /**
   * Runs the search again for the query shown, after what the toolbox offers changed: installs the
   * results when they differ and tells the listeners. Called from {@link refresh}, so never while a
   * gesture is in progress.
   */
  function searchAgain(): void {
    if (searchQuery === null || !installed) {
      return;
    }
    let definition: Blockly.utils.toolbox.ToolboxInfo;
    try {
      definition = resultsDefinition(searchQuery);
    } catch (error: unknown) {
      console.error('The toolbox search could not run again', error);
      return;
    }
    const serialised = JSON.stringify(definition);
    if (serialised !== installedDefinition) {
      installedDefinition = serialised;
      setToolbox(workspace, definition);
    }
    for (const listener of [...resultsListeners]) {
      listener();
    }
  }
  const firstDefinition = toolboxDefinition(editorMode(store.getState()));
  installedDefinition = JSON.stringify(firstDefinition);
  const installed = setToolbox(workspace, firstDefinition);
  if (installed) {
    builtKey = contentsKey();
    refresh();
  }
  setToolboxSymbols(() => toolboxSymbolGroups(contentsContext));
  const stopDrops = handleBlockDrops(workspace, (block) => {
    settleDroppedBlock(workspace, block, symbols);
  });
  const typeOf = (sym: string) => symbolType(symbols, sym);
  const stopReshaping = installStartValueReshaping(workspace, typeOf);
  // A type or a variable chosen in a block still in the flyout reshapes its start value there, so
  // the block taken out (with the pointer or the keyboard, a copy of it) shows the right one.
  const flyoutWorkspace = workspace.getFlyout()?.getWorkspace() ?? null;
  const stopFlyoutReshaping =
    flyoutWorkspace === null ? null : installStartValueReshaping(flyoutWorkspace, typeOf);
  const onEvent = (event: Blockly.Events.Abstract): void => {
    if (event instanceof Blockly.Events.Selected) {
      selection.noteSelected(event);
    }
  };
  workspace.addChangeListener(onEvent);
  /**
   * The editor's mode changed (the label or advanced settings, the project's standard or options,
   * the toolchain): the static categories' blocks and the categories shown may change, so the
   * toolbox definition is installed again when it differs from the one installed (a toolchain
   * found after the project opened changes the mode but, with no gated block, nothing shown; a
   * definition installed again would build every flyout block again); the dynamic categories follow
   * through their contents key, which holds the mode.
   */
  const followMode = (): void => {
    const mode = editorMode(store.getState());
    const key = modeKey(mode);
    if (key === installedMode || !installed) {
      return;
    }
    installedMode = key;
    const definition = currentDefinition();
    const serialised = JSON.stringify(definition);
    if (serialised === installedDefinition) {
      return;
    }
    installedDefinition = serialised;
    if (setToolbox(workspace, definition)) {
      builtKey = contentsKey();
      refresh();
    }
  };
  SEARCHES.set(workspace, {
    setQuery(query) {
      const next = query === null || query.trim() === '' ? null : query;
      if (next === searchQuery || detached) {
        return;
      }
      searchQuery = next;
      if (next === null) {
        searchCount = 0;
      }
      const definition = currentDefinition();
      installedDefinition = JSON.stringify(definition);
      if (installed && setToolbox(workspace, definition)) {
        builtKey = contentsKey();
        refresh();
      }
    },
    query: () => searchQuery,
    resultCount: () => searchCount,
    onResultsChanged(listener) {
      resultsListeners.add(listener);
      return () => {
        resultsListeners.delete(listener);
      };
    },
  });
  const unsubscribe = store.subscribe((state, previous) => {
    if (
      state.settings !== previous.settings ||
      state.toolchains !== previous.toolchains ||
      state.project?.document.project !== previous.project?.document.project
    ) {
      followMode();
    }
    // A new analysis, a notice that none is coming (the canvas did not load, the core failed),
    // other modules, or another module shown.
    if (
      state.analysis.preview !== previous.analysis.preview ||
      state.analysis.notice !== previous.analysis.notice ||
      state.project?.document.modules !== previous.project?.document.modules ||
      state.project?.activeModuleId !== previous.project?.activeModuleId
    ) {
      refreshIfChanged();
    }
  });

  return () => {
    detached = true;
    CONTEXTS.delete(workspace);
    SEARCHES.delete(workspace);
    resultsListeners.clear();
    if (refreshTimer !== null) {
      clearTimeout(refreshTimer);
      refreshTimer = null;
    }
    stopWaitingForGesture();
    unsubscribe();
    workspace.removeChangeListener(onEvent);
    stopReshaping();
    stopFlyoutReshaping?.();
    stopDrops();
    setToolboxSymbols(null);
    if (toolbox instanceof B2cContinuousToolbox) {
      toolbox.setContentsKey(null);
    }
    // Without the callbacks the dynamic categories cannot be shown: go back to static ones.
    if (installed && setToolbox(workspace, initialToolboxDefinition())) {
      refresh();
    }
    for (const category of DYNAMIC_CATEGORIES) {
      workspace.removeToolboxCategoryCallback(category.key);
    }
    workspace.removeButtonCallback(MAKE_VARIABLE_BUTTON);
  };
}

/** Makes a toolbox plugin (tests pass their own dialogs). */
export function createToolboxPlugin(options: ToolboxPluginOptions = {}): EditorPlugin {
  return {
    name: 'toolbox',
    attach: (context) => attachToolbox(context, options),
  };
}

/** The toolbox plugin of the app (append it to `EDITOR_PLUGINS`). */
export const toolboxPlugin: EditorPlugin = createToolboxPlugin();
