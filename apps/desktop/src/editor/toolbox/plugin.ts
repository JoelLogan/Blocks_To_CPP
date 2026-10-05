/**
 * The toolbox editor plugin (docs/spec/04-user-interface.md §4.2): installs the Blocks2Cpp toolbox
 * on the workspace, registers the dynamic categories (Variables, Loops, Functions with *My
 * Blocks*) and *Make a variable*, and rebuilds the dynamic categories when the selection changes
 * and after each analysis.
 *
 * The plugin works with either toolbox the workspace was injected with: the continuous toolbox
 * (see register.ts, `toolboxInjectOptions`) or Blockly's category toolbox. The workspace needs a
 * category toolbox to start with; `toolboxInjectOptions()` gives one.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';

import { getCore } from '../../app/core';
import { dialogs as appDialogs } from '../../app/dialogs/instance';
import type { DialogService, DialogTextOptions } from '../../app/dialogs/service';
import type { EditorContext, EditorPlugin } from '../../app/editor-types';
import {
  LOOPS_CATEGORY,
  MAKE_VARIABLE_BUTTON,
  MY_BLOCKS_CATEGORY,
  VARIABLES_CATEGORY,
  functionsContents,
  initialToolboxDefinition,
  loopsContents,
  toolboxDefinition,
  variablesContents,
  type ContentsContext,
  type ModuleInfo,
} from './contents';
import { makeVariable, type MakeVariableResult } from './makeVariable';
import { registerToolboxComponents } from './register';
import { installStartValueReshaping } from './reshape';
import { MAIN_TYPE, type SymbolSource } from './scope';
import { SelectionTracker } from './selection';
import './toolbox.css';

/** The app store, as editor plugins get it. */
type Store = EditorContext['store'];

/** Options of {@link createToolboxPlugin}. */
export interface ToolboxPluginOptions {
  /** The dialogs *Make a variable* uses; the app's by default. */
  readonly dialogs?: Pick<DialogService, 'prompt' | 'alert'>;
  /** Called with the outcome of each *Make a variable* (tests, logging). */
  readonly onMakeVariable?: (result: MakeVariableResult) => void;
}

/**
 * The symbols of the latest analysis: the scope query of the running compiler core (the shell's
 * current core, which replaces a trapped one, else the one the plugin was attached with) and the
 * preview's symbol list. Before the project's first analysis there are none. Never throws.
 */
export function analysisSymbolSource(store: Store, core: () => CoreWasm | null): SymbolSource {
  return {
    symbolsAt(blockId, input) {
      const current = core();
      if (store.getState().analysis.preview === null || current === null) {
        return [];
      }
      try {
        return current.symbolsInScope(blockId, input);
      } catch {
        // A trapped core answers again once the preview pipeline has replaced it.
        return [];
      }
    },
    allSymbols() {
      return store.getState().analysis.preview?.symbols ?? [];
    },
  };
}

/** The project's modules, in document order. */
function modulesOf(store: Store): readonly ModuleInfo[] {
  return (store.getState().project?.document.modules ?? []).map(({ id, name }) => ({ id, name }));
}

/** Whether a module other than `active` has `main`. */
function mainInOtherModule(store: Store, active: string): boolean {
  const modules = store.getState().project?.document.modules ?? [];
  return modules.some(
    (module) =>
      module.id !== active && module.workspace.blocks.some((block) => block.type === MAIN_TYPE),
  );
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
function withBlocklyFocus(
  dialogs: Pick<DialogService, 'prompt' | 'alert'>,
): Pick<DialogService, 'prompt' | 'alert'> {
  return {
    prompt: (options) => dialogs.prompt({ ...LEND_BLOCKLY_FOCUS, ...options }),
    alert: (options) => dialogs.alert({ ...LEND_BLOCKLY_FOCUS, ...options }),
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

/** Attaches the toolbox to the editor's workspace; returns the function that detaches it. */
function attachToolbox(context: EditorContext, options: ToolboxPluginOptions): () => void {
  registerToolboxComponents();
  const { workspace, store } = context;
  const dialogs = withBlocklyFocus(options.dialogs ?? appDialogs);
  const symbols = analysisSymbolSource(store, () => getCore() ?? context.core);
  const selection = new SelectionTracker(workspace);
  /** The selection the dynamic categories were last built for (`undefined`: not built yet). */
  let listedFor: string | null | undefined;
  const contentsContext: ContentsContext = {
    workspace,
    symbols,
    selected: () => {
      const block = selection.current();
      listedFor = block?.id ?? null;
      return block;
    },
    modules: () => modulesOf(store),
  };
  let detached = false;
  let refreshTimer: ReturnType<typeof setTimeout> | null = null;
  let selectionTimer: ReturnType<typeof setTimeout> | null = null;
  let making = false;

  const refresh = (): void => {
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
   * Rebuilds the dynamic categories when the focus moved and the selection they list for is no
   * longer the selection (a block was selected, or the canvas background was clicked). Focus moves
   * that keep the selection, such as into the toolbox, change nothing.
   */
  const scheduleSelectionCheck = (): void => {
    if (detached || selectionTimer !== null) {
      return;
    }
    selectionTimer = setTimeout(() => {
      selectionTimer = null;
      if (!detached && (selection.current()?.id ?? null) !== listedFor) {
        scheduleRefresh();
      }
    }, 0);
  };

  workspace.registerToolboxCategoryCallback(
    VARIABLES_CATEGORY,
    guarded('Variables', variablesContents, contentsContext),
  );
  workspace.registerToolboxCategoryCallback(
    LOOPS_CATEGORY,
    guarded('Loops', loopsContents, contentsContext),
  );
  workspace.registerToolboxCategoryCallback(
    MY_BLOCKS_CATEGORY,
    guarded('Functions', functionsContents, contentsContext),
  );
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

  const installed = setToolbox(workspace, toolboxDefinition());
  if (installed) {
    refresh();
  }
  const stopReshaping = installStartValueReshaping(workspace);
  const onEvent = (event: Blockly.Events.Abstract): void => {
    if (event instanceof Blockly.Events.Selected) {
      selection.noteSelected(event);
      scheduleSelectionCheck();
    }
  };
  workspace.addChangeListener(onEvent);
  // Focus moves that select nothing (onto the canvas background) send no selection event.
  const focusArea = workspace.getInjectionDiv() as HTMLElement | null;
  focusArea?.addEventListener('focusin', scheduleSelectionCheck);
  const unsubscribe = store.subscribe((state, previous) => {
    if (
      state.analysis.preview !== previous.analysis.preview ||
      state.project?.document.modules !== previous.project?.document.modules
    ) {
      scheduleRefresh();
    }
  });

  return () => {
    detached = true;
    for (const timer of [refreshTimer, selectionTimer]) {
      if (timer !== null) {
        clearTimeout(timer);
      }
    }
    refreshTimer = null;
    selectionTimer = null;
    unsubscribe();
    focusArea?.removeEventListener('focusin', scheduleSelectionCheck);
    workspace.removeChangeListener(onEvent);
    stopReshaping();
    // Without the callbacks the dynamic categories cannot be shown: go back to static ones.
    if (installed && setToolbox(workspace, initialToolboxDefinition())) {
      refresh();
    }
    workspace.removeToolboxCategoryCallback(VARIABLES_CATEGORY);
    workspace.removeToolboxCategoryCallback(LOOPS_CATEGORY);
    workspace.removeToolboxCategoryCallback(MY_BLOCKS_CATEGORY);
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
