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
