/**
 * The block editor (docs/spec/04-user-interface.md §4.1, 02 §2.4.1): Blockly with the Zelos renderer,
 * the Blocks2Cpp blocks, theme and connection checker, kept in step with the open project by an
 * editing session (./sync/session.ts) and previewed live (./preview/pipeline.ts).
 *
 * It publishes the {@link EditorHandle} features use, and attaches the editor plugins
 * (`EDITOR_PLUGINS`: the toolbox, the diagnostics, the clipboard).
 */
import { B2C_CHECKER_NAME, b2cDarkTheme, b2cLightTheme } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { useEffect, useRef } from 'react';

import { dialogs as appDialogs } from '../app/dialogs';
import type { DialogService as AppDialogService } from '../app/dialogs/service';
import {
  type EditorContext,
  type EditorHandle,
  type EditorPlugin,
  getEditorHandle,
  setEditorHandle,
} from '../app/editor-types';
import { EDITOR_PLUGINS } from '../app/editorPlugins';
import { useAppStore } from '../app/store';
import { BLOCKLY_MEDIA_DIR } from './media';
import { ModuleSwitcher } from './modules/ModuleSwitcher';
import { appCoreHost, type CoreHost } from './preview/coreHost';
import type { PreviewService } from './preview/service';
import {
  createSymbolServices,
  fieldDialogs,
  installEditorServices,
  registerEditorBlocks,
  servicesSessionHooks,
} from './services';
import { withoutEvents } from './sync/bdmToWorkspace';
import { MAX_ZOOM, MIN_ZOOM } from './sync/limits';
import { clearWorkspace } from './sync/traverse';
import { EditorSession } from './sync/session';
import { toolboxInjectOptions } from './toolbox';

/**
 * Where Blockly loads its images and cursors from: the dev server serves them straight from the
 * package, the build copies them (see vite.config.ts). Never Blockly's default, which is a web
 * server.
 */
const MEDIA_PATH = import.meta.env.DEV ? '/node_modules/blockly/media/' : `/${BLOCKLY_MEDIA_DIR}/`;

/** The options the workspace is injected with, in the given theme. */
export function editorInjectOptions(theme: Blockly.Theme): Blockly.BlocklyOptions {
  const toolbox = toolboxInjectOptions();
  return {
    renderer: 'zelos',
    theme,
    media: MEDIA_PATH,
    sounds: false,
    trashcan: true,
    // The starting toolbox and the continuous toolbox's classes, which Blockly picks only at
    // inject time; the toolbox plugin then fills in presets, Variables and My Blocks.
    toolbox: toolbox.toolbox,
    plugins: { connectionChecker: B2C_CHECKER_NAME, ...toolbox.plugins },
    grid: { spacing: 24, length: 2, colour: '#8f98ab55', snap: true },
    move: { scrollbars: true, drag: true, wheel: true },
    // The project format's zoom range (05 §5.3), so a saved view is shown as it was saved.
    zoom: {
      controls: true,
      wheel: false,
      pinch: true,
      startScale: 0.9,
      maxScale: MAX_ZOOM,
      minScale: MIN_ZOOM,
    },
  };
}

/** What {@link attachEditor} needs. */
export interface EditorDeps {
  readonly store: typeof useAppStore;
  readonly host: CoreHost;
  readonly dialogs: AppDialogService;
  readonly plugins: readonly EditorPlugin[];
  /** Runs the preview; the main-thread service by default. */
  readonly service?: PreviewService;
}

/** An attached editor. */
export interface AttachedEditor {
  readonly session: EditorSession;
  readonly handle: EditorHandle;
  /** Detaches the plugins, withdraws the handle and the services, and ends the session. */
  detach(): void;
}

/**
 * Turns an injected workspace into the Blocks2Cpp editor: installs the editor services, starts an
 * editing session (which shows the open project, if any), publishes the {@link EditorHandle} and
 * attaches the plugins. The blocks must be registered first ({@link registerEditorBlocks}).
 */
export function attachEditor(workspace: Blockly.WorkspaceSvg, deps: EditorDeps): AttachedEditor {
  const services = createSymbolServices({
    core: () => deps.host.current(),
    preview: () => deps.store.getState().analysis.preview,
  });
  const uninstallServices = installEditorServices({
    symbols: services.symbols,
    types: services.types,
    dialogs: fieldDialogs(deps.dialogs),
  });
  const session = new EditorSession({
    workspace,
    store: deps.store,
    host: deps.host,
    hooks: servicesSessionHooks(services, workspace),
    ...(deps.service === undefined ? {} : { service: deps.service }),
  });
  const handle: EditorHandle = {
    loadDocument: (doc, options) => {
      session.loadDocument(doc, options);
    },
    currentDocument: () => session.currentDocument(),
    selectBlock: (id, options) => {
      session.selectBlock(id, options);
    },
    workspace,
  };
  setEditorHandle(handle);

  const context: EditorContext = {
    workspace,
    store: deps.store,
    // A getter, never an instance: after a trap the host publishes a new core.
    core: () => deps.host.current(),
    selectBlock: (id, options) => {
      session.selectBlock(id, options);
    },
    activeModuleId: () =>
      session.shownModuleId() ?? deps.store.getState().project?.activeModuleId ?? '',
  };
  const detachers: { name: string; detach: () => void }[] = [];
  for (const plugin of deps.plugins) {
    try {
      detachers.push({ name: plugin.name, detach: plugin.attach(context) });
    } catch (error: unknown) {
      console.error(`The editor plugin ${plugin.name} failed to attach`, error);
    }
  }
  // Start the compiler core now, so the first preview does not wait for it.
  deps.host.start().catch((error: unknown) => {
    console.error('The compiler core could not be started', error);
  });

  let attached = true;
  return {
    session,
    handle,
    detach() {
      if (!attached) {
        return;
      }
      attached = false;
      for (const { name, detach } of detachers.reverse()) {
        try {
          detach();
        } catch (error: unknown) {
          console.error(`The editor plugin ${name} failed to detach`, error);
        }
      }
      if (getEditorHandle() === handle) {
        setEditorHandle(null);
      }
      session.dispose();
      uninstallServices();
    },
  };
}

/** The block workspace with the module switcher above it. */
export function EditorWorkspace() {
  const host = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const element = host.current;
    if (element === null) {
      return;
    }
    registerEditorBlocks();
    const darkScheme = window.matchMedia('(prefers-color-scheme: dark)');
    const theme = (): Blockly.Theme => (darkScheme.matches ? b2cDarkTheme : b2cLightTheme);
    const workspace = Blockly.inject(element, editorInjectOptions(theme()));
    const editor = attachEditor(workspace, {
      store: useAppStore,
      host: appCoreHost(),
      dialogs: appDialogs,
      plugins: EDITOR_PLUGINS,
    });

    // Blockly sizes its SVG once; follow the panel when the window or a dock resizes.
    const resizeObserver = new ResizeObserver(() => {
      Blockly.svgResize(workspace);
    });
    resizeObserver.observe(element);
    const onSchemeChange = (): void => {
      workspace.setTheme(theme());
    };
    darkScheme.addEventListener('change', onSchemeChange);

    return () => {
      darkScheme.removeEventListener('change', onSchemeChange);
      resizeObserver.disconnect();
      editor.detach();
      // Children first and without events: Blockly's own disposal recurses along `next` chains,
      // and a delete event would serialise every block.
      clearWorkspace(workspace);
      withoutEvents(() => {
        workspace.dispose();
      });
    };
  }, []);

  return (
    <>
      <ModuleSwitcher />
      <div ref={host} className="blockly-host" />
    </>
  );
}
