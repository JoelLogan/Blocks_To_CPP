/**
 * The toolbox editor plugin (docs/spec/04-user-interface.md §4.2): installs the Blocks2Cpp toolbox
 * on the workspace, registers the dynamic categories (Variables, Loops, Functions with *Your
 * functions*) and *Make a variable*, and rebuilds the dynamic categories when the selection
 * changes, after each analysis and when another module is shown.
 *
 * While the canvas has changed since the latest analysis (a preview is pending), the dynamic
 * categories keep listing for the selection they were last built for: the analysis does not know
 * the new blocks or places yet, so a listing for a new selection would be one the next analysis
 * replaces at once. The preview's arrival rebuilds them for the current selection.
 *
 * The plugin works with either toolbox the workspace was injected with: the continuous toolbox
 * (see register.ts, `toolboxInjectOptions`) or Blockly's category toolbox. The workspace needs a
 * category toolbox to start with; `toolboxInjectOptions()` gives one.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';

import { dialogs as appDialogs } from '../../app/dialogs/instance';
import type { DialogService, DialogTextOptions } from '../../app/dialogs/service';
import type { EditorContext, EditorPlugin } from '../../app/editor-types';
import { analysisScope } from '../services/symbols';
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
 * How long the dynamic categories wait for the analysis of a changed canvas before they follow the
 * selection again anyway (a preview that never comes: the canvas did not load, or the core failed).
 */
export const PREVIEW_WAIT_MS = 1_000;

/**
 * The symbols of the latest analysis, as the editor's symbol provider reads them
 * (`analysisScope`, ../services/symbols.ts): the scope query of the running compiler core (the
 * shell's current core, which replaces a trapped one) and the preview's symbol list. Before the
 * project's first analysis there are none. Never throws.
 *
 * The toolbox resolves its listing points itself (`resolveScopeAnchor`), so it reads the analysis
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
  const symbols = analysisSymbolSource(store, () => context.core());
  const selection = new SelectionTracker(workspace);
  /** The selection the dynamic categories were last built for (`undefined`: not built yet). */
  let listedFor: string | null | undefined;
  /** Whether the canvas changed after the latest analysis arrived (a preview is pending). */
  let previewPending = false;
  /**
   * The selection the dynamic categories list for: the current one, or, while a preview is
   * pending, the one they were last built for (unless that block is gone).
   */
  const listingSelection = (): Blockly.Block | null => {
    // Asked every time, so the tracker keeps up with the focus.
    const current = selection.current();
    if (previewPending && listedFor !== undefined) {
      if (listedFor === null) {
        return null;
      }
      const listed = workspace.getBlockById(listedFor);
      if (listed !== null && !listed.isDisposed()) {
        return listed;
      }
    }
    listedFor = current?.id ?? null;
    return current;
  };
  const contentsContext: ContentsContext = {
    workspace,
    symbols,
    selected: listingSelection,
    modules: () => modulesOf(store),
    activeModuleId: () => context.activeModuleId(),
  };
  let detached = false;
  let refreshTimer: ReturnType<typeof setTimeout> | null = null;
  let selectionTimer: ReturnType<typeof setTimeout> | null = null;
  let previewTimer: ReturnType<typeof setTimeout> | null = null;
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
   * that keep the selection, such as into the toolbox, change nothing, and so does a new selection
   * while a preview is pending: its arrival rebuilds them.
   */
  const scheduleSelectionCheck = (): void => {
    if (detached || selectionTimer !== null) {
      return;
    }
    selectionTimer = setTimeout(() => {
      selectionTimer = null;
      if (!detached && !previewPending && (selection.current()?.id ?? null) !== listedFor) {
        scheduleRefresh();
      }
    }, 0);
  };

  /** No preview is pending any more: the listing follows the selection again. */
  const endPreviewWait = (): void => {
    previewPending = false;
    if (previewTimer !== null) {
      clearTimeout(previewTimer);
      previewTimer = null;
    }
  };
  /**
   * The canvas changed, which starts a preview (every change that is not only a UI event does):
   * keep the listing until it arrives, or until {@link PREVIEW_WAIT_MS} have passed without it.
   */
  const notePreviewPending = (): void => {
    previewPending = true;
    if (previewTimer !== null) {
      clearTimeout(previewTimer);
    }
    previewTimer = setTimeout(() => {
      previewTimer = null;
      if (!detached && previewPending) {
        previewPending = false;
        scheduleSelectionCheck();
      }
    }, PREVIEW_WAIT_MS);
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
    if (!event.isUiEvent) {
      notePreviewPending();
    } else if (event instanceof Blockly.Events.Selected) {
      selection.noteSelected(event);
      scheduleSelectionCheck();
    }
  };
  workspace.addChangeListener(onEvent);
  // Focus moves that select nothing (onto the canvas background) send no selection event.
  const focusArea = workspace.getInjectionDiv() as HTMLElement | null;
  focusArea?.addEventListener('focusin', scheduleSelectionCheck);
  const unsubscribe = store.subscribe((state, previous) => {
    // A new preview, or a notice that none is coming (the canvas did not load, the core failed).
    const analysed =
      state.analysis.preview !== previous.analysis.preview ||
      state.analysis.notice !== previous.analysis.notice;
    if (analysed) {
      endPreviewWait();
    }
    if (
      analysed ||
      state.project?.document.modules !== previous.project?.document.modules ||
      state.project?.activeModuleId !== previous.project?.activeModuleId
    ) {
      scheduleRefresh();
    }
  });

  return () => {
    detached = true;
    for (const timer of [refreshTimer, selectionTimer, previewTimer]) {
      if (timer !== null) {
        clearTimeout(timer);
      }
    }
    refreshTimer = null;
    selectionTimer = null;
    previewTimer = null;
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
