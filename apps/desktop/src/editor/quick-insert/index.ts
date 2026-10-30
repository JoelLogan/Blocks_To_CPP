/** Quick Insert and toolbox search (docs/plans/m3-contracts/w4-editor-tools.md §2.4). */
export {
  currentTarget,
  insertBlock,
  type InsertTarget,
  type Inserted,
  type InsertedAt,
} from './insert';
export {
  MAX_LINE_CHARS,
  lineNames,
  parseLine,
  valueTokens,
  type LineNames,
  type ParsedLine,
} from './parseLine';
export {
  QUICK_INSERT_IDS,
  QUICK_INSERT_MENU_TEXT,
  TOOLBOX_SEARCH_MENU_TEXT,
  openQuickInsertOn,
  quickInsertPlugin,
} from './plugin';
export { QuickInsert, SIMPLE_LINES_NOTE } from './QuickInsert';
export {
  closeQuickInsert,
  openQuickInsert,
  quickInsertSession,
  subscribeQuickInsert,
  type QuickInsertSession,
} from './store';
export { SEARCH_DELAY_MS, ToolboxSearch } from './ToolboxSearch';
export {
  closeToolboxSearch,
  openToolboxSearch,
  subscribeToolboxSearch,
  toolboxSearchOpen,
} from './toolboxSearchStore';
