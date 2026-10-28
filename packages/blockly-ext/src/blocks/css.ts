/**
 * The few CSS rules the Blocks2Cpp blocks add to Blockly's (fixed text, no user content). The CSP
 * allows inline styles (`style-src 'unsafe-inline'`, docs/spec/08-security.md §8.8) because Blockly
 * injects its own the same way.
 */
import * as Blockly from 'blockly/core';

/** CSS classes used by the blocks. */
export const B2C_CSS_CLASS = Object.freeze({
  /** The part of an expression a diagnostic points at. */
  tokenHighlight: 'b2cTokenHighlight',
  /** An expression kept as an unfinished draft. */
  draft: 'b2cExprDraft',
  /** The "Missing pack" badge of a placeholder block. */
  missingPack: 'b2cMissingPackBadge',
});

const CSS = `
.blocklyText.${B2C_CSS_CLASS.tokenHighlight} {
  text-decoration: underline;
  font-weight: bold;
}
.blocklyText.${B2C_CSS_CLASS.draft} {
  font-style: italic;
}
.blocklyText.${B2C_CSS_CLASS.missingPack} {
  font-weight: bold;
}
.b2c-type-picker {
  display: flex;
  flex-direction: column;
  gap: 4px;
  width: 18rem;
  max-width: calc(100vw - 32px);
  padding: 4px;
  font-family: var(--b2c-font, sans-serif);
  font-size: 0.875rem;
  color: var(--b2c-text, #1a1a1a);
}
.b2c-type-picker-path {
  color: var(--b2c-text-muted, #555);
  overflow-wrap: anywhere;
}
.b2c-type-picker-search {
  font: inherit;
  color: inherit;
  background: var(--b2c-surface, #fff);
  border: 1px solid var(--b2c-control-border, #767676);
  border-radius: 4px;
  padding: 4px 6px;
}
.b2c-type-picker-search:focus {
  outline: 2px solid var(--b2c-focus, #1a56db);
  outline-offset: 1px;
}
.b2c-type-picker-list {
  max-height: 16rem;
  overflow-y: auto;
}
.b2c-type-picker-heading {
  padding: 4px 6px 2px;
  font-size: 0.75rem;
  font-weight: bold;
  color: var(--b2c-text-muted, #555);
}
.b2c-type-picker-option {
  padding: 3px 6px;
  border-radius: 4px;
  cursor: default;
  overflow-wrap: anywhere;
}
.b2c-type-picker-option[aria-selected='true'] {
  background: var(--b2c-selected, #dbe7ff);
  outline: 1px solid var(--b2c-focus, #1a56db);
}
.b2c-type-picker-option[aria-current='true'] {
  font-weight: bold;
}
.b2c-type-picker-empty,
.b2c-type-picker-status {
  padding: 2px 6px;
  color: var(--b2c-text-muted, #555);
}
`;

let registered = false;

/**
 * Adds the rules once. Before the first workspace is injected they join Blockly's stylesheet;
 * afterwards (Blockly accepts no more) they go into a `<style>` element of their own.
 */
export function registerB2cCss(): void {
  if (registered) {
    return;
  }
  registered = true;
  try {
    Blockly.Css.register(CSS);
  } catch {
    if (typeof document !== 'undefined') {
      const style = document.createElement('style');
      style.dataset['b2c'] = 'blocks';
      style.textContent = CSS;
      document.head.append(style);
    }
  }
}
