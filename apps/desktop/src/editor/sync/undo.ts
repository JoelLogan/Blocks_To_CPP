/**
 * Undo and Redo on the block editor's canvas act only once Blockly has delivered the events of the
 * changes made before them (docs/spec/04-user-interface.md §4.1).
 *
 * Blockly delivers a change's events in a task after the next animation frame
 * (`requestAnimationFrame`, then `setTimeout`), and only then records them for undo. An undo that
 * runs before that undoes the step before the change, and the change's events, delivered
 * afterwards, clear the redo history: neither Undo nor Redo brings the change back. A `Ctrl+Z`
 * right after a drop on the toolbox lost the dropped block that way (Blockly undid the drag's start
 * on a block that was gone). The key comes first when the frame is late (a long render or preview
 * of a large program) or when it is pressed within a frame of the change (a test driver).
 *
 * So every undo and redo waits for the next animation frame and a task after it
 * ({@link undoAfterDelivery}): Blockly's own `Ctrl+Z`, `Ctrl+Shift+Z` and `Ctrl+Y` and the canvas
 * menu's *Undo* and *Redo*, whose actions are replaced while an editing session shows a canvas
 * ({@link installUndoItems}), and the session's own undo (the window's keys, the ≡ menu, the
 * banner's *Undo last change*). A frame callback asked for after a change runs after Blockly's,
 * and its task after Blockly's, so the change is on the undo stack by then. Blockly has no public
 * way to tell whether events wait to be delivered, so every undo waits; the cost is one frame (two
 * right after a change): what an undo changes is drawn that much later than before.
 *
 * A listener that hears of a change may make one of its own in the change's event group (a start
 * value that follows a new type, ../toolbox/reshape.ts), which Blockly delivers a frame later
 * still. So an undo acts only after a frame in which Blockly recorded nothing new for undo; until
 * then it waits another frame, and one undo still takes back a whole event group.
 *
 * Undos wait in line and act one at a time, each after a frame of its own, so they act in the
 * order they were asked for (a held key undoes step by step), and a change made between two of
 * them is recorded before the second acts. Blockly's checks are made when the key is pressed (a
 * canvas that can be edited, no drag, no field editor or drop-down open), and its pop-ups close
 * then. An undo does nothing if, by the time it would act, its canvas has been disposed or a block
 * is dragged on it (Blockly never undoes during a drag).
 *
 * The registries are global, while each editor has its own canvas: the actions are replaced when
 * the first session installs them, and Blockly's own are put back when the last one is removed.
 */
import * as Blockly from 'blockly/core';

/** Blockly's undo and redo shortcuts, and whether each redoes. */
const SHORTCUTS = [
  { name: Blockly.ShortcutItems.names.UNDO, redo: false },
  { name: Blockly.ShortcutItems.names.REDO, redo: true },
] as const;

/** The IDs of Blockly's *Undo* and *Redo* items of the canvas menu. */
export const UNDO_MENU_ITEM_IDS = { undo: 'undoWorkspace', redo: 'redoWorkspace' } as const;

/** A menu action (an item that is not a separator). */
type ActionItem = Exclude<Blockly.ContextMenuRegistry.RegistryItem, { separator: true }>;

/** An undo or redo asked for and not made yet. */
interface WaitingUndo {
  readonly workspace: Blockly.Workspace;
  readonly redo: boolean;
  /** The event on top of the workspace's undo stack when the undo last looked. */
  lastRecorded: Blockly.Events.Abstract | undefined;
}

/** The undos and redos asked for and not made yet, the first asked for first. */
const waiting: WaitingUndo[] = [];

/** The event Blockly recorded for undo last on `workspace` (the top of its undo stack). */
function lastRecorded(workspace: Blockly.Workspace): Blockly.Events.Abstract | undefined {
  return workspace.getUndoStack().at(-1);
}

/** Calls `next` after the next animation frame and a task, as Blockly delivers its events. */
function afterNextFrame(next: () => void): void {
  requestAnimationFrame(() => {
    setTimeout(next, 0);
  });
}

/**
 * Undoes (or, with `redo`, redoes) the last change on `workspace` once Blockly has delivered and
 * recorded the events fired so far, and those its listeners fired in turn, and once the undos asked
 * for before it have acted (see the module comment). It does nothing if by then `workspace` has
 * been disposed or a block is dragged on it.
 */
export function undoAfterDelivery(workspace: Blockly.Workspace, redo: boolean): void {
  waiting.push({ workspace, redo, lastRecorded: lastRecorded(workspace) });
  if (waiting.length === 1) {
    afterNextFrame(undoFirstWaiting);
  }
}

/** How many undos and redos wait to act (tests). */
export function undosWaiting(): number {
  return waiting.length;
}

/**
 * Makes the first waiting undo, unless Blockly has recorded events on its canvas since it last
 * looked: their listeners may have made changes of their own, which come a frame later, so it looks
 * again after the next frame. The undo after it waits for a frame of its own, also when this one
 * fails (it is logged).
 */
function undoFirstWaiting(): void {
  const first = waiting[0];
  if (first === undefined) {
    return;
  }
  const { workspace, redo } = first;
  const dragging = workspace instanceof Blockly.WorkspaceSvg && workspace.isDragging();
  if (Blockly.Workspace.getById(workspace.id) === workspace && !dragging) {
    const last = lastRecorded(workspace);
    if (last !== first.lastRecorded) {
      first.lastRecorded = last;
      afterNextFrame(undoFirstWaiting);
      return;
    }
    try {
      workspace.undo(redo);
    } catch (error: unknown) {
      console.error('An undo failed', error);
    }
  }
  waiting.shift();
  const next = waiting[0];
  if (next !== undefined) {
    next.lastRecorded = lastRecorded(next.workspace);
    afterNextFrame(undoFirstWaiting);
  }
}

/** Blockly's shortcut `original` on `keyCodes`, acting through {@link undoAfterDelivery}. */
function delayedShortcut(
  original: Blockly.ShortcutRegistry.KeyboardShortcut,
  keyCodes: string[],
  redo: boolean,
): Blockly.ShortcutRegistry.KeyboardShortcut {
  return {
    ...original,
    keyCodes,
    callback(workspace, event) {
      workspace.hideChaff();
      undoAfterDelivery(workspace, redo);
      event.preventDefault();
      return true;
    },
  };
}

/** Blockly's menu item `original`, acting through {@link undoAfterDelivery}. */
function delayedMenuItem(original: ActionItem, redo: boolean): ActionItem {
  return {
    ...original,
    callback(scope) {
      if (scope.workspace !== undefined) {
        undoAfterDelivery(scope.workspace, redo);
      }
    },
  };
}

/** The registry's action item `id`, or `null`. */
function actionItem(id: string): ActionItem | null {
  const item = Blockly.ContextMenuRegistry.registry.getItem(id);
  return item === null || item.separator === true ? null : item;
}

/** Replaces the actions of Blockly's items; returns the function that puts Blockly's back. */
function replaceBlocklyItems(): () => void {
  const shortcuts = Blockly.ShortcutRegistry.registry;
  const known = shortcuts.getRegistry();
  const savedShortcuts: Blockly.ShortcutRegistry.KeyboardShortcut[] = [];
  for (const { name, redo } of SHORTCUTS) {
    const original = known[name];
    if (original === undefined) {
      continue;
    }
    const keyCodes = shortcuts.getKeyCodesByShortcutName(name);
    savedShortcuts.push({ ...original, keyCodes });
    shortcuts.unregister(name);
    shortcuts.register(delayedShortcut(original, keyCodes, redo));
  }

  const menu = Blockly.ContextMenuRegistry.registry;
  const savedItems: ActionItem[] = [];
  for (const [id, redo] of [
    [UNDO_MENU_ITEM_IDS.undo, false],
    [UNDO_MENU_ITEM_IDS.redo, true],
  ] as const) {
    const original = actionItem(id);
    if (original === null) {
      continue;
    }
    savedItems.push(original);
    menu.unregister(id);
    menu.register(delayedMenuItem(original, redo));
  }

  return () => {
    const registered = shortcuts.getRegistry();
    for (const original of savedShortcuts) {
      if (registered[original.name] !== undefined) {
        shortcuts.unregister(original.name);
      }
      shortcuts.register(original, true);
    }
    for (const original of savedItems) {
      if (menu.getItem(original.id) !== null) {
        menu.unregister(original.id);
      }
      menu.register(original);
    }
  };
}

let installs = 0;
let restoreBlockly: (() => void) | null = null;

/**
 * Makes Blockly's undo and redo keys and canvas menu items undo with {@link undoAfterDelivery},
 * replacing their actions on first use. Returns the function that stops it (and, for the last
 * install, puts Blockly's own actions back).
 */
export function installUndoItems(): () => void {
  installs += 1;
  restoreBlockly ??= replaceBlocklyItems();
  let installed = true;
  return () => {
    if (!installed) {
      return;
    }
    installed = false;
    installs -= 1;
    if (installs === 0 && restoreBlockly !== null) {
      const restore = restoreBlockly;
      restoreBlockly = null;
      restore();
    }
  };
}
