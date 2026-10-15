/**
 * Bringing the page to rest (settle.ts): the page scripts that wait for animations and hide overlay
 * scroll bars, run here against a fake page.
 */
import { describe, expect, it } from 'vitest';

import { FakeLoop, runScript } from '../support/fake-page';
import { ANIMATIONS_SCRIPT, OVERLAY_SCROLLBARS_SCRIPT, SCROLLBARS_KEY } from './settle';

describe('ANIMATIONS_SCRIPT', () => {
  /** A page whose animations run until `until` on the loop's clock. */
  function animatedPage(loop: FakeLoop, until: number, getAnimations = true) {
    const animation = {
      animationName: 'fade',
      get playState() {
        return loop.now < until ? 'running' : 'finished';
      },
      pending: false,
      effect: {
        target: {
          tagName: 'DIV',
          getAttribute: (name: string) => (name === 'class' ? 'hint open' : null),
        },
      },
    };
    return loop.globals({
      document: getAnimations ? { getAnimations: () => [animation] } : {},
    });
  }

  it('calls back once no animation runs', () => {
    const loop = new FakeLoop();
    let result: unknown = null;
    runScript(ANIMATIONS_SCRIPT, animatedPage(loop, 300), [5_000], (value) => {
      result = value;
    });
    loop.run(() => result !== null);
    expect(result).toEqual([]);
    expect(loop.now).toBeGreaterThanOrEqual(300);
    expect(loop.now).toBeLessThan(340);
  });

  it('names the animations still running at the time limit', () => {
    const loop = new FakeLoop();
    let result: unknown = null;
    runScript(ANIMATIONS_SCRIPT, animatedPage(loop, Infinity), [1_000], (value) => {
      result = value;
    });
    loop.run(() => result !== null);
    expect(result).toEqual(['fade on div.hint.open']);
  });

  it('counts a page without getAnimations as still', () => {
    const loop = new FakeLoop();
    let result: unknown = null;
    runScript(ANIMATIONS_SCRIPT, animatedPage(loop, Infinity, false), [1_000], (value) => {
      result = value;
    });
    loop.run(() => result !== null);
    expect(result).toEqual([]);
  });
});

describe('OVERLAY_SCROLLBARS_SCRIPT', () => {
  /** A page whose scroll bars take `room` pixels, recording the style sheets it adopts. */
  function scrollbarPage(room: number) {
    const sheets: string[] = [];
    const appended: unknown[] = [];
    class FakeSheet {
      text = '';
      replaceSync(text: string) {
        this.text = text;
      }
    }
    const document = {
      adoptedStyleSheets: [] as FakeSheet[],
      body: {
        appendChild: (element: unknown) => {
          appended.push(element);
        },
      },
      createElement: () => {
        const style = new Map<string, string>();
        return {
          style: { setProperty: (name: string, value: string) => style.set(name, value) },
          get offsetWidth() {
            return style.get('overflow') === 'scroll' ? 100 : 0;
          },
          get clientWidth() {
            return 100 - room;
          },
          remove: () => {
            appended.pop();
          },
        };
      },
    };
    const window: Record<string, unknown> = {};
    const page = { window, document, CSSStyleSheet: FakeSheet };
    const adopted = () => {
      sheets.splice(0, sheets.length, ...document.adoptedStyleSheets.map((sheet) => sheet.text));
      return sheets;
    };
    return { page, adopted, appended };
  }

  it('hides overlay scroll bars once, and leaves the probe out of the page', () => {
    const { page, adopted, appended } = scrollbarPage(0);
    expect(runScript(OVERLAY_SCROLLBARS_SCRIPT, page, [SCROLLBARS_KEY])).toBe('hidden');
    expect(adopted()).toEqual(['* { scrollbar-width: none !important; }']);
    expect(appended).toEqual([]);
    expect(runScript(OVERLAY_SCROLLBARS_SCRIPT, page, [SCROLLBARS_KEY])).toBe('already');
    expect(adopted()).toHaveLength(1);
  });

  it('leaves classic scroll bars, which take room', () => {
    const { page, adopted } = scrollbarPage(17);
    expect(runScript(OVERLAY_SCROLLBARS_SCRIPT, page, [SCROLLBARS_KEY])).toBe('classic');
    expect(adopted()).toEqual([]);
  });
});
