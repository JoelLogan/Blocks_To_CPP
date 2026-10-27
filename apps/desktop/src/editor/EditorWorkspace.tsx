/**
 * The block editor (docs/spec/04-user-interface.md §4.1, 02 §2.4.1): Blockly with the Zelos renderer,
 * the Blocks2Cpp blocks, theme and connection checker, kept in step with the open project by an
 * editing session (./sync/session.ts) and previewed live (./preview/pipeline.ts).
 *
 * It publishes the {@link EditorHandle} features use, and attaches the editor plugins
 * (`EDITOR_PLUGINS`: the toolbox, the diagnostics, the clipboard, the keyboard navigation).
 */
import { B2C_CHECKER_NAME, B2C_PREVIEWER_NAME, b2cTheme } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

import { documentTheme, onThemeChange } from '../app/appearance';
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
import { B2cBlockDragger } from './sync/drag';
import { MAX_ZOOM, MIN_ZOOM } from './sync/limits';
import { clearWorkspace } from './sync/traverse';
import { EditorSession } from './sync/session';
import { toolboxInjectOptions } from './toolbox';
import { CanvasControls } from './view/CanvasControls';

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
    // The trash can is a place to drop blocks on and keeps nothing (04 §4.1): restoring from it
    // would bypass the validated clipboard and carry blocks from one project into the next. Undo
    // brings deleted blocks back.
    trashcan: true,
    maxTrashcanContents: 0,
    // The starting toolbox and the continuous toolbox's classes, which Blockly picks only at
    // inject time; the toolbox plugin then fills in presets, Variables and Your functions. The block
    // dragger tells the editing session when a drag starts, before Blockly moves anything. The
    // connection previewer also previews an empty C-block wrapping statements (03 §3.3).
    toolbox: toolbox.toolbox,
    plugins: {
      connectionChecker: B2C_CHECKER_NAME,
      connectionPreviewer: B2C_PREVIEWER_NAME,
      blockDragger: B2cBlockDragger,
      ...toolbox.plugins,
    },
    grid: { spacing: 24, length: 2, colour: '#8f98ab55', snap: true },
    move: { scrollbars: true, drag: true, wheel: true },
    // The project format's zoom range (05 §5.3), so a saved view is shown as it was saved. The
    // wheel scrolls; Ctrl + wheel and a pinch zoom around the pointer (04 §4.7). Blockly's SVG zoom
    // controls are replaced by the editor's named buttons (./view/CanvasControls.tsx).
    zoom: {
      controls: false,
      wheel: true,
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
    workspace,
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
    updateProjectInfo: (info) => {
      session.updateProjectInfo(info);
    },
    undo: () => {
      session.undo(false);
    },
    redo: () => {
      session.undo(true);
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

/** An injected workspace and the element it was injected into. */
interface InjectedCanvas {
  readonly workspace: Blockly.WorkspaceSvg;
  readonly host: HTMLElement;
}

/**
 * The block workspace with the module switcher above it, and the canvas controls over its top
 * right corner: they are rendered into the workspace's container after Blockly's injection `div`,
 * so they share its coordinates and follow the canvas in the Tab order.
 */
export function EditorWorkspace() {
  const host = useRef<HTMLDivElement>(null);
  const [canvas, setCanvas] = useState<InjectedCanvas | null>(null);

  useEffect(() => {
    const element = host.current;
    if (element === null) {
      return;
    }
    registerEditorBlocks();
    // The theme in use (the Appearance setting, on <html>, ../app/appearance), followed below.
    const workspace = Blockly.inject(element, editorInjectOptions(b2cTheme(documentTheme())));
    const editor = attachEditor(workspace, {
      store: useAppStore,
      host: appCoreHost(),
      dialogs: appDialogs,
      plugins: EDITOR_PLUGINS,
    });

    // Blockly sizes its SVG once; follow the panel when the window or a dock resizes, and when
    // the editor is shown (it is hidden while the start page opens a project). The session keeps
    // track of whether the user moved the view, and shows a saved view that waited for a size.
    const resizeObserver = new ResizeObserver(() => {
      editor.session.resize();
    });
    resizeObserver.observe(element);
    const stopFollowingTheme = onThemeChange((theme) => {
      workspace.setTheme(b2cTheme(theme));
    });
    setCanvas({ workspace, host: element });

    return () => {
      setCanvas(null);
      stopFollowingTheme();
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
      {canvas !== null &&
        createPortal(<CanvasControls workspace={canvas.workspace} />, canvas.host)}
    </>
  );
}
