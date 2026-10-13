/**
 * The toolbox editor plugin (docs/spec/04-user-interface.md §4.2): installs the Blocks2Cpp toolbox
 * on the workspace, registers the dynamic categories (Program, Variables, Functions with *Your
 * functions*) and *Make a variable*, settles the blocks dragged out of it where they land
 * (./landing.ts), gives the menus of blocks still in its flyout the module's variables, and keeps
 * start values in step with types (./reshape.ts).
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
import { analysisScope } from '../services/symbols';
import { handleBlockDrops } from '../sync/drag';
import {
  MAKE_VARIABLE_BUTTON,
  MY_BLOCKS_CATEGORY,
  PROGRAM_CATEGORY,
  VARIABLES_CATEGORY,
  dynamicContentsKey,
  functionsContents,
  initialToolboxDefinition,
  programContents,
  toolboxDefinition,
  toolboxSymbolGroups,
  variablesContents,
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

/** Builds a dynamic category, showing a short note instead of failing the whole flyout. */
function guarded(
  name: string,
  build: (context: ContentsContext) => Blockly.utils.toolbox.FlyoutItemInfoArray,
  context: ContentsContext,
): () => Blockly.utils.toolbox.FlyoutItemInfoArray {
  return () => {
    try {
      return build(context);
    } catch (error: unknown) {
      console.error(`The toolbox category ${name} could not be built`, error);
      return [{ kind: 'label', text: 'These blocks could not be shown.', id: undefined }];
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
  readonly build: (context: ContentsContext) => Blockly.utils.toolbox.FlyoutItemInfoArray;
}[] = [
  { key: PROGRAM_CATEGORY, name: 'Program', build: programContents },
  { key: VARIABLES_CATEGORY, name: 'Variables', build: variablesContents },
  { key: MY_BLOCKS_CATEGORY, name: 'Functions', build: functionsContents },
];

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
  };
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
  /** Rebuilds the dynamic categories when what they show changed since they were last built. */
  const refreshIfChanged = (): void => {
    const key = contentsKey();
    if (key === null || key !== builtKey) {
      builtKey = key;
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
  const installed = setToolbox(workspace, toolboxDefinition());
  if (installed) {
    builtKey = contentsKey();
    refresh();
  }
  setToolboxSymbols(() => toolboxSymbolGroups(contentsContext));
  const stopDrops = handleBlockDrops(workspace, (block) => {
    settleDroppedBlock(workspace, block, symbols);
  });
  const stopReshaping = installStartValueReshaping(workspace, (sym) => symbolType(symbols, sym));
  const onEvent = (event: Blockly.Events.Abstract): void => {
    if (event instanceof Blockly.Events.Selected) {
      selection.noteSelected(event);
    }
  };
  workspace.addChangeListener(onEvent);
  const unsubscribe = store.subscribe((state, previous) => {
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
    if (refreshTimer !== null) {
      clearTimeout(refreshTimer);
      refreshTimer = null;
    }
    stopWaitingForGesture();
    unsubscribe();
    workspace.removeChangeListener(onEvent);
    stopReshaping();
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
