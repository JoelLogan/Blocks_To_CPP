/**
 * The blocks' CSS (src/blocks/css.ts) is added even when the first workspace is injected before
 * registration: Blockly then accepts no more rules, so they get a `<style>` element of their own.
 */
import { describe, expect, it } from 'vitest';

import { B2C_CSS_CLASS, registerB2cCss } from '../src';
import { renderedWorkspace } from './helpers';

describe('registerB2cCss', () => {
  it('falls back to its own style element after Blockly has injected its CSS, once', () => {
    renderedWorkspace();
    registerB2cCss();
    registerB2cCss();
    const styles = document.head.querySelectorAll('style[data-b2c="blocks"]');
    expect(styles).toHaveLength(1);
    expect(styles[0]?.textContent).toContain(B2C_CSS_CLASS.tokenHighlight);
  });
});
