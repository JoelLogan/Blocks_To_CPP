/**
 * Bringing the page to rest (settle.ts): the page scripts that wait for the fonts and animations
 * and hide overlay scroll bars, run here against a fake page; and the page's own time limits,
 * which must answer before WebDriver's script timeout does.
 */
import { describe, expect, it } from 'vitest';

import { QUIET_SCRIPT } from '../bench/page';
import { SCRIPT_TIMEOUT_MS } from '../support/app';
import { FakeLoop, runScript } from '../support/fake-page';
import {
  ANIMATIONS_SCRIPT,
  FONTS_SCRIPT,
  OVERLAY_SCROLLBARS_SCRIPT,
  QUIET_FRAME_MS,
  QUIET_FRAMES,
  SCROLLBARS_KEY,
  STILL_FALLBACK_MS,
  STILL_TIMEOUT_MS,
} from './settle';

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

  it('names them before WebDriver’s script timeout, with frames or without', () => {
    for (const frameEvery of [16, null]) {
      const loop = new FakeLoop();
      loop.frameEvery = frameEvery;
      let result: unknown = null;
      runScript(
        ANIMATIONS_SCRIPT,
        animatedPage(loop, Infinity),
        [STILL_TIMEOUT_MS, STILL_FALLBACK_MS],
        (value) => {
          result = value;
        },
      );
      loop.run(() => result !== null);
      expect(result, String(frameEvery)).toEqual(['fade on div.hint.open']);
      expect(loop.now).toBeGreaterThan(STILL_TIMEOUT_MS);
      expect(loop.now).toBeLessThanOrEqual(STILL_TIMEOUT_MS + STILL_FALLBACK_MS + 1);
      expect(loop.now).toBeLessThan(SCRIPT_TIMEOUT_MS);
    }
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

describe('the page’s own time limits', () => {
  it('leave WebDriver’s script timeout room to carry the answer', () => {
    // The page answers at the first frame after STILL_TIMEOUT_MS, or by its timer
    // STILL_FALLBACK_MS later; WebDriver waits SCRIPT_TIMEOUT_MS for it (both launchers).
    expect(STILL_TIMEOUT_MS + STILL_FALLBACK_MS).toBeLessThanOrEqual(SCRIPT_TIMEOUT_MS - 5_000);
    expect(STILL_FALLBACK_MS).toBeGreaterThan(0);
  });

  it('end the wait for quiet frames on a page that stays busy before WebDriver’s', () => {
    for (const frameEvery of [QUIET_FRAME_MS + 20, null]) {
      const loop = new FakeLoop();
      loop.frameEvery = frameEvery;
      let result: unknown = null;
      runScript(
        QUIET_SCRIPT,
        loop.globals(),
        [QUIET_FRAMES, QUIET_FRAME_MS, STILL_TIMEOUT_MS],
        (value) => {
          result = value;
        },
      );
      loop.run(() => result !== null);
      expect(result, String(frameEvery)).toBe('busy');
      expect(loop.now).toBeLessThanOrEqual(STILL_TIMEOUT_MS + STILL_FALLBACK_MS + 1);
      expect(loop.now).toBeLessThan(SCRIPT_TIMEOUT_MS);
    }
  });
});

describe('FONTS_SCRIPT', () => {
  /** A page whose fonts are ready after `ready` ms on the loop's clock (never: `null`). */
  function fontsPage(loop: FakeLoop, ready: number | null) {
    const fonts = {
      ready: new Promise<void>((resolve) => {
        if (ready !== null) {
          loop.post(resolve, ready);
        }
      }),
    };
    return loop.globals({ document: { fonts } });
  }

  /** Runs FONTS_SCRIPT until it answers (promise callbacks run between the loop's tasks). */
  async function fontsResult(loop: FakeLoop, ready: number | null): Promise<unknown> {
    let result: unknown = null;
    runScript(FONTS_SCRIPT, fontsPage(loop, ready), [STILL_TIMEOUT_MS], (value) => {
      result = value;
    });
    while (result === null) {
      const before = loop.now;
      loop.run(() => loop.now > before);
      await Promise.resolve();
      if (loop.now === before) {
        break;
      }
    }
    return result;
  }

  it('calls back once the fonts have loaded', async () => {
    const loop = new FakeLoop();
    expect(await fontsResult(loop, 500)).toBe(true);
    expect(loop.now).toBeLessThan(1_000);
  });

  it('gives up at the time limit, before WebDriver’s script timeout', async () => {
    const loop = new FakeLoop();
    expect(await fontsResult(loop, null)).toBe(false);
    expect(loop.now).toBeGreaterThanOrEqual(STILL_TIMEOUT_MS);
    expect(loop.now).toBeLessThan(SCRIPT_TIMEOUT_MS);
  });
});
