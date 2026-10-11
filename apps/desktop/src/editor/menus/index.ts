/**
 * Blockly's block and canvas menu items as Blocks2Cpp offers them (docs/spec/04-user-interface.md
 * §4.1 "Menus"): no *Inline Inputs*, a *Delete N blocks* that counts the blocks it deletes, asks
 * first and is undone in one step, and *Clean up blocks* kept.
 *
 * - `menusPlugin` is the editor plugin (in `EDITOR_PLUGINS`).
 * - `createMenusPlugin(options)` makes one with other dialogs (tests).
 */
export {
  chainTooLongNotice,
  createMenusPlugin,
  deleteAllQuestion,
  menusPlugin,
  type MenusPluginOptions,
} from './plugin';
export {
  countShown,
  deletableStacks,
  deleteLabel,
  deleteStacks,
  installMenuItems,
  MENU_ITEM_IDS,
  type MenuTarget,
} from './registry';
