/**
 * Bringing the page to rest before a screenshot of the visual diff (README.md here): nothing has
 * the focus and no text caret shows, the pointer rests where it hovers nothing, overlay scroll bars
 * are hidden, the fonts have loaded, no animation or transition runs and the page has drawn a run
 * of frames on time. The block canvas (canvas.visual.e2e.ts) and the chrome scenes
 * (chrome.visual.e2e.ts) share it.
 */
import { Origin, type WebDriver } from 'selenium-webdriver';

import { QUIET_SCRIPT } from '../bench/page';

/** How many frames in a row must come on time, and how late one may be, in milliseconds. */
export const QUIET_FRAMES = 20;
export const QUIET_FRAME_MS = 40;

/** How long the page may take to come to rest, in milliseconds. */
export const STILL_TIMEOUT_MS = 30_000;

/**
 * Runs in the webview: takes focus away from whatever has it (no focus ring, no text caret, and
 * the console's cursor drawn as the steady outline of an unfocused terminal), hides the caret
 * everywhere in case an editor opens, and answers whether the page prefers the dark theme. Inline
 * style properties set from script are allowed by the app's style-src.
 */
export const STILL_SCRIPT = `
  const active = document.activeElement;
  if (active instanceof HTMLElement || active instanceof SVGElement) {
    active.blur();
  }
  document.documentElement.style.setProperty('caret-color', 'transparent');
  return window.matchMedia('(prefers-color-scheme: dark)').matches;
`;

/** Runs in the webview (async): calls back once the page's fonts have loaded. */
export const FONTS_SCRIPT = `
  const done = arguments[arguments.length - 1];
  document.fonts.ready.then(() => done(true), () => done(false));
`;

/**
 * Runs in the webview (`executeAsyncScript(ANIMATIONS_SCRIPT, timeoutMs)`): calls back with `[]`
 * once no CSS animation or transition (nor any other Web Animation) runs on the page, checking
 * every frame, or with up to 5 descriptions of those still running after `timeoutMs`. A page
 * without `document.getAnimations` counts as having none.
 */
export const ANIMATIONS_SCRIPT = `
  const timeoutMs = arguments[0];
  const done = arguments[arguments.length - 1];
  const begin = performance.now();
  let finished = false;
  const finish = (result) => {
    if (!finished) {
      finished = true;
      done(result);
    }
  };
  const running = () => {
    if (typeof document.getAnimations !== 'function') {
      return [];
    }
    return document.getAnimations()
      .filter((animation) => animation.playState === 'running' || animation.pending === true)
      .slice(0, 5)
      .map((animation) => {
        const name = animation.animationName || animation.transitionProperty || animation.id || 'an animation';
        const effect = animation.effect;
        const target = effect && effect.target;
        const where = target && target.tagName
          ? target.tagName.toLowerCase() + (target.getAttribute('class') ? '.' + String(target.getAttribute('class')).trim().split(/\\s+/).join('.') : '')
          : 'the page';
        return String(name) + ' on ' + where;
      });
  };
  const step = () => {
    if (finished) {
      return;
    }
    const now = running();
    if (now.length === 0) {
      finish([]);
    } else if (performance.now() - begin > timeoutMs) {
      finish(now);
    } else {
      requestAnimationFrame(step);
    }
  };
  step();
  // Without animation frames (a hidden window) nothing would ever call back.
  setTimeout(() => finish(running()), timeoutMs + 1000);
`;

/** The window global that marks the page whose overlay scroll bars are hidden. */
export const SCROLLBARS_KEY = '__B2C_VISUAL_SCROLLBARS__';

/**
 * Runs in the webview (`executeScript(OVERLAY_SCROLLBARS_SCRIPT, SCROLLBARS_KEY)`): when the web
 * view's scroll bars take no room (overlay scroll bars, as WebKitGTK draws them), hides them all
 * with an adopted style sheet (`scrollbar-width: none`), once per page; classic scroll bars, which
 * take room and stay as they are, are left alone. Overlay scroll bars appear when the pointer
 * passes over a scrolling area or it scrolls, and fade out over the next few seconds, by a timer
 * of the web view that the page cannot see, so a screenshot would catch them at any stage. They
 * take no room, so hiding them moves nothing. Returns `'hidden'`, `'already'` or `'classic'`.
 */
export const OVERLAY_SCROLLBARS_SCRIPT = `
  const key = arguments[0];
  if (window[key] === true) {
    return 'already';
  }
  const probe = document.createElement('div');
  probe.style.setProperty('position', 'absolute');
  probe.style.setProperty('left', '-200px');
  probe.style.setProperty('top', '0');
  probe.style.setProperty('width', '100px');
  probe.style.setProperty('height', '100px');
  probe.style.setProperty('overflow', 'scroll');
  document.body.appendChild(probe);
  const room = probe.offsetWidth - probe.clientWidth;
  probe.remove();
  if (room > 0) {
    return 'classic';
  }
  const sheet = new CSSStyleSheet();
  sheet.replaceSync('* { scrollbar-width: none !important; }');
  document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];
  window[key] = true;
  return 'hidden';
`;

/** Hides the web view's overlay scroll bars, if it has those ({@link OVERLAY_SCROLLBARS_SCRIPT}). */
export async function hideOverlayScrollbars(driver: WebDriver): Promise<void> {
  const result: unknown = await driver.executeScript(OVERLAY_SCROLLBARS_SCRIPT, SCROLLBARS_KEY);
  if (result !== 'hidden' && result !== 'already' && result !== 'classic') {
    throw new Error('The page did not say what its scroll bars are');
  }
}

/** Writes a GitHub Actions warning annotation (plain text elsewhere). */
export function warn(message: string): void {
  process.stdout.write(`::warning title=Visual diff::${message.replace(/\r?\n/g, ' ')}\n`);
}

/** The page's viewport size, in CSS pixels. */
export async function viewportSize(driver: WebDriver): Promise<{ width: number; height: number }> {
  const size: unknown = await driver.executeScript(
    'return [window.innerWidth, window.innerHeight];',
  );
  if (
    !Array.isArray(size) ||
    size.length !== 2 ||
    !size.every((side) => typeof side === 'number' && side > 0)
  ) {
    throw new Error('The page did not report its size');
  }
  const [width, height] = size as [number, number];
  return { width, height };
}

/**
 * Moves the pointer to the window's bottom-left corner, in the status bar's padding, where it
 * hovers nothing (no hover colour, no hint).
 */
export async function parkPointer(driver: WebDriver): Promise<void> {
  const viewport = await viewportSize(driver);
  await driver
    .actions({ async: true })
    .move({ x: 2, y: viewport.height - 4, origin: Origin.VIEWPORT })
    .perform();
}

/**
 * Takes the focus away and hides the caret ({@link STILL_SCRIPT}); returns whether the page
 * prefers the dark theme.
 */
export async function blurAll(driver: WebDriver): Promise<boolean> {
  const dark: unknown = await driver.executeScript(STILL_SCRIPT);
  return dark === true;
}

/**
 * Waits until the page has its fonts, runs no animation and has drawn {@link QUIET_FRAMES} frames
 * in a row on time (the first preview, a flyout's scrolling and the like have ended).
 *
 * @throws Error naming what still moves after {@link STILL_TIMEOUT_MS}.
 */
export async function waitUntilStill(driver: WebDriver): Promise<void> {
  const fonts: unknown = await driver.executeAsyncScript(FONTS_SCRIPT);
  if (fonts !== true) {
    throw new Error('The page fonts did not load');
  }
  const moving: unknown = await driver.executeAsyncScript(ANIMATIONS_SCRIPT, STILL_TIMEOUT_MS);
  if (Array.isArray(moving) && moving.length > 0) {
    throw new Error(
      `Animations still run after ${String(STILL_TIMEOUT_MS / 1000)} s: ${moving.map(String).join('; ')}`,
    );
  }
  const quiet: unknown = await driver.executeAsyncScript(
    QUIET_SCRIPT,
    QUIET_FRAMES,
    QUIET_FRAME_MS,
    STILL_TIMEOUT_MS,
  );
  if (quiet !== 'quiet') {
    throw new Error(`The page did not come to rest within ${String(STILL_TIMEOUT_MS / 1000)} s`);
  }
}
