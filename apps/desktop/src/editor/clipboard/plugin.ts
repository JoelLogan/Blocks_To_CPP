/**
 * The clipboard editor plugin (docs/spec/05-project-format.md §5.12, 04 §4.7): copy, cut, paste
 * and duplicate through the compiler core's validated clipboard format, on the keyboard
 * (`Ctrl+C`, `Ctrl+X`, `Ctrl+V`, `Ctrl+D`), in the block and canvas menus, through the webview's
 * DOM clipboard events, and as the app commands `edit.copy`, `edit.cut` and `edit.paste`.
 *
 * - Copies go to the system clipboard (`application/x-blocks2cpp+json` and the blocks' C++ as
 *   `text/plain`) when the webview fires a copy event, and always to the in-app copy.
 * - `Ctrl+V` pastes the system clipboard's payload when the webview gives one (another window of
 *   the app, for example), otherwise the in-app copy; the menus and `edit.paste` paste the in-app
 *   copy, since the system clipboard can only be read in a paste event.
 * - Nothing on the clipboard is trusted: every payload is validated by the core like a project
 *   file, and a refused one is reported with the loader's problems.
 *
 * See ./controller.ts for what each action does, ./anchor.ts for where a paste goes, and
 * ./bridge.ts for the DOM events.
 */
import { randomSeedHex } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';

import { type CommandRegistry, commands as appCommands } from '../../app/commands';
import { dialogs as appDialogs } from '../../app/dialogs/instance';
import type { EditorContext, EditorPlugin } from '../../app/editor-types';
import { appCoreHost } from '../preview/coreHost';
import { canvasFocus, SelectionTracker } from '../toolbox/selection';
import { anchorFor, anchorForBlock, copyableBlock, ON_CANVAS, type PasteAnchor } from './anchor';
import { ClipboardBridge, type Scheduler } from './bridge';
import { ClipboardController } from './controller';
import { appClipboardMemory, type ClipboardMemory } from './memory';
import { type ClipboardNotifier, dialogNotifier } from './notices';
import { type ClipboardKeyTarget, installClipboardRegistries } from './registry';

/** Options of {@link createClipboardPlugin}. */
export interface ClipboardPluginOptions {
  /** The in-app copy; the window's shared one by default. */
  readonly memory?: ClipboardMemory;
  /** Tells the user why an action did nothing; an alert of the app's dialogs by default. */
  readonly notify?: ClipboardNotifier;
  /** Where `edit.copy`, `edit.cut` and `edit.paste` are registered; the app's by default. */
  readonly commands?: CommandRegistry | null;
  /** Fresh randomness for each paste (64 hex digits); `randomSeedHex()` by default. */
  readonly seed?: () => string;
  /**
   * Starts a new compiler core after a trap, the app's core host by default; `null` leaves it to
   * the preview pipeline.
   */
  readonly restartCore?: (() => Promise<unknown>) | null;
  /** Runs the DOM bridge's fallbacks after the current task (tests). */
  readonly schedule?: Scheduler;
  /** Receives each editor's clipboard once it is attached (tests and tools). */
  readonly onAttached?: (clipboard: EditorClipboard) => void;
}

/** The node that has the focus, or `null`. Never throws. */
function focusedNode(): unknown {
  try {
    return Blockly.getFocusManager().getFocusedNode();
  } catch (error: unknown) {
    console.warn('The focus could not be read', error);
    return null;
  }
}

/** Whether a key event is a held key repeating. */
function isRepeat(event: Event): boolean {
  return event instanceof KeyboardEvent && event.repeat;
}

/** The clipboard of one attached editor: its keys, menu items, DOM events and commands. */
export class EditorClipboard implements ClipboardKeyTarget {
  readonly workspace: Blockly.WorkspaceSvg;
  readonly controller: ClipboardController;
  readonly bridge: ClipboardBridge;
  private readonly tracker: SelectionTracker;

  constructor(
    workspace: Blockly.WorkspaceSvg,
    controller: ClipboardController,
    schedule: Scheduler | undefined,
  ) {
    this.workspace = workspace;
    this.controller = controller;
    this.tracker = new SelectionTracker(workspace);
    this.bridge = new ClipboardBridge({
      document: workspace.getInjectionDiv().ownerDocument,
      container: () => workspace.getInjectionDiv(),
      copyFocused: (cut) => {
        const block = copyableBlock(workspace, focusedNode());
        if (block === null) {
          return null;
        }
        return cut ? this.controller.cut(block) : this.controller.copy(block);
      },
      pasteFocused: (payload) => {
        this.controller.paste(payload, anchorFor(workspace, focusedNode()));
      },
      ...(schedule === undefined ? {} : { schedule }),
    });
  }

  /** Takes note of a selection on the canvas (for the commands, which run from elsewhere). */
  noteSelected(event: Blockly.Events.Selected): void {
    this.tracker.noteSelected(event);
  }

  /**
   * Forgets the block selected last (another project was opened: a block of it may have the same
   * ID, but the user never selected it).
   */
  forgetSelection(): void {
    this.tracker.forget();
  }

  /** Stops handling DOM events. */
  dispose(): void {
    this.bridge.dispose();
  }

  // Keys (see ./registry.ts).

  canCopy(focused: unknown): boolean {
    return this.controller.canCopy(copyableBlock(this.workspace, focused));
  }

  canCut(focused: unknown): boolean {
    return this.controller.canCut(copyableBlock(this.workspace, focused));
  }

  canPaste(): boolean {
    return this.controller.canEdit();
  }

  hasCopy(): boolean {
    return this.controller.memory.get() !== null;
  }

  canDuplicate(focused: unknown): boolean {
    return this.controller.canDuplicate(copyableBlock(this.workspace, focused));
  }

  /**
   * `Ctrl+C`: copies now (the in-app copy) and arms the bridge, leaving the key to the webview so
   * that it fires its copy event, which gets the data.
   */
  copyKey(focused: unknown, event: Event): boolean {
    return this.copyOrCutKey(focused, event, false);
  }

  /** `Ctrl+X`: as {@link copyKey}, and deletes what was copied. */
  cutKey(focused: unknown, event: Event): boolean {
    return this.copyOrCutKey(focused, event, true);
  }

  /**
   * `Ctrl+V`: notes where the paste goes and arms the bridge, leaving the key to the webview so
   * that it fires its paste event (whose payload is pasted); without one, the in-app copy is.
   */
  pasteKey(focused: unknown, event: Event): boolean {
    if (isRepeat(event)) {
      event.preventDefault();
      return true;
    }
    const anchor = anchorFor(this.workspace, focused);
    this.bridge.armPaste((payload) => {
      this.controller.paste(payload, anchor);
    });
    return true;
  }

  /** `Ctrl+D`: duplicates the focused block (the clipboards are left alone). */
  duplicateKey(focused: unknown, event: Event): boolean {
    const block = copyableBlock(this.workspace, focused);
    if (block === null) {
      return false;
    }
    // The webview's own meaning of the key (a bookmark) never applies.
    event.preventDefault();
    if (!isRepeat(event)) {
      this.controller.duplicate(block);
    }
    return true;
  }

  // Menu items (see ./registry.ts).

  copyBlock(block: Blockly.Block): void {
    const data = this.controller.copy(block);
    if (data !== null) {
      this.bridge.writeNow(data);
    }
  }

  cutBlock(block: Blockly.Block): void {
    const data = this.controller.cut(block);
    if (data !== null) {
      this.bridge.writeNow(data);
    }
  }

  pasteAt(anchor: PasteAnchor): void {
    this.controller.paste(null, anchor);
  }

  anchorFor(block: Blockly.BlockSvg): PasteAnchor {
    return anchorForBlock(block);
  }

  duplicateBlock(block: Blockly.BlockSvg): void {
    this.controller.duplicate(block);
  }

  canCutBlock(block: Blockly.BlockSvg): boolean {
    return this.controller.canCut(block);
  }

  canCopyBlock(block: Blockly.BlockSvg): boolean {
    return this.controller.canCopy(block);
  }

  canDuplicateBlock(block: Blockly.BlockSvg): boolean {
    return this.controller.canDuplicate(block);
  }

  // Commands (`edit.copy`, `edit.cut`, `edit.paste`): they run from a button or menu, where the
  // focus may have left the canvas, so they use the canvas's last selection.

  /** `edit.copy`: copies the selected block to the in-app copy and, if allowed, the system's. */
  commandCopy(): void {
    const block = this.tracker.current();
    if (block !== null && this.controller.canCopy(block)) {
      this.copyBlock(block);
    }
  }

  /** `edit.cut`: as {@link commandCopy}, and deletes the selected block. */
  commandCut(): void {
    const block = this.tracker.current();
    if (block !== null && this.controller.canCut(block)) {
      this.cutBlock(block);
    }
  }

  /** `edit.paste`: pastes the in-app copy at the selection (on the canvas without one). */
  commandPaste(): void {
    const focus = canvasFocus(this.workspace);
    let anchor: PasteAnchor;
    if (focus.kind === 'elsewhere') {
      const remembered = this.tracker.current();
      anchor = remembered === null ? ON_CANVAS : anchorForBlock(remembered);
    } else {
      anchor = anchorFor(this.workspace, focusedNode());
    }
    this.controller.paste(null, anchor);
  }

  private copyOrCutKey(focused: unknown, event: Event, cut: boolean): boolean {
    if (isRepeat(event)) {
      event.preventDefault();
      return true;
    }
    const block = copyableBlock(this.workspace, focused);
    if (block === null) {
      return false;
    }
    const data = cut ? this.controller.cut(block) : this.controller.copy(block);
    if (data === null) {
      // Nothing was copied (the notice says why): the webview must not copy anything else.
      event.preventDefault();
      return true;
    }
    this.bridge.armWrite(data);
    return true;
  }
}

/** The clipboard plugin with its dependencies replaced (see {@link ClipboardPluginOptions}). */
export function createClipboardPlugin(options: ClipboardPluginOptions = {}): EditorPlugin {
  return {
    name: 'clipboard',
    attach(ctx: EditorContext): () => void {
      const workspace = ctx.workspace;
      const restartCore =
        options.restartCore === undefined ? () => appCoreHost().restart() : options.restartCore;
      const controller = new ClipboardController({
        workspace,
        store: ctx.store,
        core: ctx.core,
        activeModuleId: () => ctx.activeModuleId(),
        memory: options.memory ?? appClipboardMemory,
        notify: options.notify ?? dialogNotifier(appDialogs),
        seed: options.seed ?? (() => randomSeedHex()),
        restartCore,
      });
      const clipboard = new EditorClipboard(workspace, controller, options.schedule);
      const onEvent = (event: Blockly.Events.Abstract): void => {
        if (event instanceof Blockly.Events.Selected) {
          clipboard.noteSelected(event);
        }
      };
      workspace.addChangeListener(onEvent);
      const unsubscribe = ctx.store.subscribe((state, previous) => {
        if (state.project?.handle !== previous.project?.handle) {
          clipboard.forgetSelection();
        }
      });
      const uninstall = installClipboardRegistries(clipboard);
      const registry = options.commands === undefined ? appCommands : options.commands;
      const unregister =
        registry === null
          ? []
          : [
              registry.registerCommand('edit.copy', () => {
                clipboard.commandCopy();
              }),
              registry.registerCommand('edit.cut', () => {
                clipboard.commandCut();
              }),
              registry.registerCommand('edit.paste', () => {
                clipboard.commandPaste();
              }),
            ];
      options.onAttached?.(clipboard);
      return () => {
        for (const dispose of unregister) {
          dispose();
        }
        uninstall();
        unsubscribe();
        workspace.removeChangeListener(onEvent);
        clipboard.dispose();
      };
    },
  };
}

/** The clipboard editor plugin (append it to `EDITOR_PLUGINS`). */
export const clipboardPlugin: EditorPlugin = createClipboardPlugin();
