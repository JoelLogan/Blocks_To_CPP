/**
 * Layout rules of the panels' style sheet that happy-dom cannot check by rendering (it computes
 * no layout): the console fits its terminal inside its padding and nothing around it scrolls
 * (04 §4.5), and a copy confirmation never moves the code (04 §4.3). The end-to-end tests check
 * the same in the real webviews (e2e/specs/flows/console.e2e.ts).
 */
import { describe, expect, it } from 'vitest';

import { declarationsOf, parseRules } from '../app/layout/cssAudit';
import panelsCss from './panels.css?raw';

const RULES = parseRules(panelsCss);

/** The declarations for `selector`, outside any `@media` block. */
function style(selector: string): Map<string, string> {
  return declarationsOf(RULES, selector);
}

describe('the console', () => {
  it("puts the terminal's padding on xterm's own element, which the fit add-on subtracts", () => {
    // Padding on the host would make the fit add-on propose one row too many.
    const host = style('.b2c-console-terminal');
    expect(host.get('padding')).toBeUndefined();
    expect([...host.keys()].filter((name) => name.startsWith('padding'))).toEqual([]);
    expect(style('.b2c-console-terminal .xterm')).toEqual(
      new Map([
        ['height', '100%'],
        ['padding', '4px 0 0 6px'],
      ]),
    );
  });

  it('lets nothing around the terminal scroll', () => {
    expect(style('.b2c-console-terminal').get('overflow')).toBe('hidden');
    expect(style('.dock-panel:has(> .b2c-console-panel)').get('overflow')).toBe('hidden');
  });

  it('scrolls a header taller than its panel by itself, so its buttons stay reachable', () => {
    // An open notice in the smallest bottom dock: the dock panel clips, so the header must scroll.
    const header = style('.b2c-console-header');
    expect(header.get('overflow-y')).toBe('auto');
    expect(header.get('min-height')).toBe('0');
    expect(header.get('max-height')).toBe('100%');
  });
});

describe('the status next to panel buttons', () => {
  it('never wraps inside itself', () => {
    expect(style('.b2c-panel-status').get('white-space')).toBe('nowrap');
  });

  it("floats the code panel's copy confirmation over the code, out of the bar's flow", () => {
    expect(style('.b2c-code-panel').get('isolation')).toBe('isolate');
    expect(style('.b2c-code-panel .b2c-panel-bar').get('position')).toBe('relative');
    const status = style('.b2c-code-copy-status');
    expect(status.get('position')).toBe('absolute');
    expect(status.get('top')).toBe('calc(100% + 4px)');
    expect(status.get('pointer-events')).toBe('none');
    expect(Number(status.get('z-index'))).toBeGreaterThan(0);
    // CodeMirror's own layers (z-index up to 500) stay inside the editor.
    expect(style('.b2c-code-editor').get('isolation')).toBe('isolate');
  });
});
