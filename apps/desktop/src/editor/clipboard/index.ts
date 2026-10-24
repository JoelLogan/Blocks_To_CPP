/**
 * The editor's validated clipboard (docs/spec/05-project-format.md §5.12, 04 §4.7): copy, cut,
 * paste and duplicate through the compiler core's clipboard format, with fresh IDs and references
 * bound again at the paste target.
 *
 * - `clipboardPlugin` is the editor plugin (append it to `EDITOR_PLUGINS`). It replaces Blockly's
 *   copy, cut, paste and duplicate (keys and menu items), handles the webview's DOM clipboard
 *   events, and registers the commands `edit.copy`, `edit.cut` and `edit.paste`.
 * - `createClipboardPlugin(options)` makes one with other dependencies (tests).
 */
export {
  anchorFor,
  anchorForBlock,
  copyableBlock,
  isProjectBlock,
  ON_CANVAS,
  type PasteAnchor,
  pasteTarget,
  type WorkspacePoint,
} from './anchor';
export { documentWithPasted } from './candidate';
export {
  ClipboardBridge,
  type ClipboardBridgeOptions,
  nextTask,
  type PasteRun,
  type Scheduler,
} from './bridge';
export {
  ClipboardController,
  type ClipboardControllerOptions,
  type PasteOutcome,
} from './controller';
export {
  CLIPBOARD_MIME,
  type ClipboardData,
  MAX_PAYLOAD_CHARS,
  PLAIN_TEXT_MIME,
  readTransfer,
  writeTransfer,
} from './formats';
export { CANVAS_STEP, type InsertedBlocks, insertPasted } from './insert';
export { appClipboardMemory, ClipboardMemory } from './memory';
export {
  type ClipboardAction,
  type ClipboardNotice,
  type ClipboardNotifier,
  describeNotice,
  dialogNotifier,
  MAX_LISTED_PROBLEMS,
  MAX_PROBLEM_CHARS,
  type UnavailableReason,
} from './notices';
export {
  clipboardPlugin,
  type ClipboardPluginOptions,
  createClipboardPlugin,
  EditorClipboard,
} from './plugin';
export {
  type ClipboardKeyTarget,
  installClipboardRegistries,
  MENU_ITEM_IDS,
  MENU_LABELS,
  SHORTCUT_NAMES,
} from './registry';
