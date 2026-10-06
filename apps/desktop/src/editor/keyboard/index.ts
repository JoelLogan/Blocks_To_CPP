/**
 * Keyboard use of the block editor (docs/spec/04-user-interface.md §4.7, §4.8): the keyboard
 * editor plugin and its parts. See ./plugin.ts.
 */
export { Announcer, MAX_ANNOUNCEMENT_CHARS } from './announcer';
export {
  CANVAS_DESCRIPTION,
  CANVAS_LABEL,
  FLYOUT_DESCRIPTION,
  FLYOUT_LABEL,
  KEY_MAP,
  type KeyHelp,
  type KeyScope,
} from './help';
export { describeBlock, describeNode, describeTarget } from './labels';
export {
  attachReducedMotion,
  prefersReducedMotion,
  REDUCED_MOTION_QUERY,
  type MatchMedia,
} from './motion';
export { KeyboardMover, MOVING_CLASS, type MoveOptions } from './mover';
export { EditorKeyboard, type CursorStep, type FocusArea } from './navigation';
export {
  createKeyboardPlugin,
  keyboardPlugin,
  MOVE_KEYS_HINT,
  type KeyboardPluginOptions,
} from './plugin';
export { KEYBOARD_SHORTCUT_NAMES } from './shortcuts';
export { dropTargets, MAX_DROP_TARGETS, type DropTarget } from './targets';
