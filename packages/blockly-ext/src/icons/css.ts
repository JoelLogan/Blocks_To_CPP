/**
 * The CSS of the diagnostics on blocks (docs/spec/04-user-interface.md §4.4): the badge, the
 * severity outline of the block, and the marks on the field or input a diagnostic points at. Every
 * class name is a fixed string from this file and the rules are static text, never built from
 * project data (docs/security/custom-field-review-checklist.md).
 *
 * Colour is never the only signal (§4.8): the badge has a different shape and glyph per severity,
 * and the outlines and underlines have a different line style per severity (solid, dashed,
 * dotted).
 */
import * as Blockly from 'blockly/core';

import type { Severity } from './summary';

/** The CSS classes of the diagnostics on blocks. */
export const DIAGNOSTIC_CSS_CLASS = Object.freeze({
  /** The badge's group (it also has Blockly's `blocklyIconGroup`). */
  badge: 'b2cDiagBadge',
  /** The badge's group, by the severity it shows. */
  badgeSeverity: Object.freeze({
    error: 'b2cDiagBadgeError',
    warning: 'b2cDiagBadgeWarning',
    info: 'b2cDiagBadgeInfo',
  } satisfies Record<Severity, string>),
  /** The badge's filled shape (circle or triangle). */
  badgeShape: 'b2cDiagBadgeShape',
  /** The badge's glyph (✖, !, i), drawn as strokes. */
  badgeGlyph: 'b2cDiagBadgeGlyph',
  /** A badge that shows only compiler messages from an older build: dimmed. */
  badgeStale: 'b2cDiagBadgeStale',
  /** The block's own group: its outline is tinted by the most serious severity. */
  block: Object.freeze({
    error: 'b2cDiagBlockError',
    warning: 'b2cDiagBlockWarning',
    info: 'b2cDiagBlockInfo',
  } satisfies Record<Severity, string>),
  /** The block's own group when every diagnostic on it is from an older build. */
  blockStale: 'b2cDiagBlockStale',
  /** A field a diagnostic points at (on the field's group). */
  field: Object.freeze({
    error: 'b2cDiagFieldError',
    warning: 'b2cDiagFieldWarning',
    info: 'b2cDiagFieldInfo',
  } satisfies Record<Severity, string>),
  /** The block (or expression shadow) in an input a diagnostic points at. */
  input: Object.freeze({
    error: 'b2cDiagInputError',
    warning: 'b2cDiagInputWarning',
    info: 'b2cDiagInputInfo',
  } satisfies Record<Severity, string>),
});

/** The badge fills: dark enough for the white glyph in both themes (contrast over 4.5:1). */
export const BADGE_FILL: Readonly<Record<Severity, string>> = Object.freeze({
  error: '#b42318',
  warning: '#8a5300',
  info: '#1a56db',
});

const C = DIAGNOSTIC_CSS_CLASS;
/** Blockly's selection outline is a copy of the block's path; it keeps its own look. */
const OWN_PATH = '> .blocklyPath:not(.blocklyPathSelected)';

const CSS = `
.blocklySvg {
  --b2c-diag-error: #b42318;
  --b2c-diag-warning: #8a5300;
  --b2c-diag-info: #1a56db;
}
@media (prefers-color-scheme: dark) {
  .blocklySvg {
    --b2c-diag-error: #ff8a80;
    --b2c-diag-warning: #ffcc66;
    --b2c-diag-info: #8ab4ff;
  }
}
.blocklyIconGroup.${C.badge},
.blocklyIconGroup.${C.badge}:not(:hover):not(:focus) {
  opacity: 1;
  cursor: default;
}
.blocklyIconGroup.${C.badge}.${C.badgeStale},
.blocklyIconGroup.${C.badge}.${C.badgeStale}:not(:hover):not(:focus) {
  opacity: 0.55;
}
.${C.badge} .${C.badgeShape} {
  stroke: #ffffff;
  stroke-width: 1.5px;
}
.${C.badgeSeverity.error} .${C.badgeShape} {
  fill: ${BADGE_FILL.error};
}
.${C.badgeSeverity.warning} .${C.badgeShape} {
  fill: ${BADGE_FILL.warning};
}
.${C.badgeSeverity.info} .${C.badgeShape} {
  fill: ${BADGE_FILL.info};
}
.${C.badge} .${C.badgeGlyph} {
  fill: none;
  stroke: #ffffff;
  stroke-width: 2px;
  stroke-linecap: round;
}
.${C.block.error} ${OWN_PATH} {
  stroke: var(--b2c-diag-error);
  stroke-width: 3px;
}
.${C.block.warning} ${OWN_PATH} {
  stroke: var(--b2c-diag-warning);
  stroke-width: 3px;
  stroke-dasharray: 8 4;
}
.${C.block.info} ${OWN_PATH} {
  stroke: var(--b2c-diag-info);
  stroke-width: 2px;
  stroke-dasharray: 2 4;
}
.${C.blockStale} ${OWN_PATH} {
  stroke-opacity: 0.5;
}
.${C.input.error} ${OWN_PATH} {
  stroke: var(--b2c-diag-error);
  stroke-width: 2px;
}
.${C.input.warning} ${OWN_PATH} {
  stroke: var(--b2c-diag-warning);
  stroke-width: 2px;
  stroke-dasharray: 6 3;
}
.${C.input.info} ${OWN_PATH} {
  stroke: var(--b2c-diag-info);
  stroke-width: 2px;
  stroke-dasharray: 2 3;
}
.${C.field.error} > .blocklyFieldRect {
  stroke: var(--b2c-diag-error);
  stroke-width: 2px;
}
.${C.field.warning} > .blocklyFieldRect {
  stroke: var(--b2c-diag-warning);
  stroke-width: 2px;
  stroke-dasharray: 6 3;
}
.${C.field.info} > .blocklyFieldRect {
  stroke: var(--b2c-diag-info);
  stroke-width: 2px;
  stroke-dasharray: 2 3;
}
.${C.field.error} .blocklyText {
  text-decoration: underline wavy var(--b2c-diag-error);
}
.${C.field.warning} .blocklyText {
  text-decoration: underline dashed var(--b2c-diag-warning);
}
.${C.field.info} .blocklyText {
  text-decoration: underline dotted var(--b2c-diag-info);
}
`;

let registered = false;

/**
 * Adds the diagnostics' CSS once. Before the first workspace is injected it joins Blockly's
 * stylesheet; afterwards (Blockly accepts no more) it goes into a `<style>` element of its own.
 */
export function registerDiagnosticCss(): void {
  if (registered) {
    return;
  }
  registered = true;
  try {
    Blockly.Css.register(CSS);
  } catch {
    if (typeof document !== 'undefined') {
      const style = document.createElement('style');
      style.dataset['b2c'] = 'diagnostics';
      style.textContent = CSS;
      document.head.append(style);
    }
  }
}
