/**
 * Diagnostics on blocks (docs/spec/04-user-interface.md §4.4): the `b2c_diagnostic` badge at the
 * block's top-right corner, the severity outline, and the marks on the field, input or tokens a
 * diagnostic points at.
 *
 * ```ts
 * registerDiagnosticIcon();
 * setBlockDiagnostics(block, diagnosticsOfThatBlock); // [] removes them
 * ```
 */
export { BADGE_FILL, DIAGNOSTIC_CSS_CLASS, registerDiagnosticCss } from './css';
export {
  BADGE_SIZE,
  DIAGNOSTIC_ICON_NAME,
  DIAGNOSTIC_ICON_TYPE,
  DiagnosticIcon,
  getBlockDiagnostics,
  registerDiagnosticIcon,
  setBlockDiagnostics,
} from './diagnosticIcon';
export { partMarkKeys } from './parts';
export {
  MAX_MESSAGE_CHARS,
  MAX_TOOLTIP_LINES,
  SEVERITY_GLYPH,
  SEVERITY_LABEL,
  SEVERITY_ORDER,
  isSeverity,
  moreSerious,
  summariseDiagnostics,
  type BlockDiagnosticItem,
  type DiagnosticBadgeSummary,
} from './summary';
