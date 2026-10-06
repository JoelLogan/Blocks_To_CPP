/**
 * What the webview benchmarks run inside the app's page, through WebDriver's `executeScript` and
 * `executeAsyncScript`: timing is taken in the page, with its own clock, so the round trips of
 * WebDriver are not part of any measurement. Each script is a plain string (not a function from
 * this file), so nothing the test runner adds to compiled functions can end up in the page; each
 * one only reads the page, apart from the edit the preview benchmark makes through the test hook.
 */
import type { E2ePoint } from '../../src/e2e/contract';
import type { Rect } from '../support/canvas';

/**
 * The readiness marker of the cold-start benchmark: the start page is shown (the window has
 * started, which needs the backend's first answer), Blockly's workspace is injected and the
 * toolbox has rendered its categories. Runs as a function body: `true` once all three hold.
 */
const MARKER = `
  return (
    document.querySelector('[data-testid="start-page"]') !== null &&
    document.querySelector('[data-testid="workspace"] svg.blocklySvg') !== null &&
    document.querySelectorAll('.blocklyToolboxCategory').length > 0
  );
`;

/** What {@link READY_SCRIPT} calls back with. */
export type ReadyResult =
  | {
      readonly kind: 'ready';
      /** When the marker first held: the page clock's time origin plus its time (epoch ms). */
      readonly at: number;
      /** Whether it already held when the script started (then `at` is only an upper bound). */
      readonly already: boolean;
    }
  | { readonly kind: 'timeout' };

/** What {@link PAGE_SCRIPT} returns. */
export interface PageLook {
  /** The address of the document the window shows (`location.href`). */
  readonly href: string;
  /** The document's clock origin (epoch ms): every new document has its own. */
  readonly timeOrigin: number;
  /**
   * When the readiness marker was seen to hold (the page clock's time origin plus its time, epoch
   * ms), or `null` when it did not hold.
   */
  readonly readyAt: number | null;
}

/**
 * Runs in the webview (`executeScript(PAGE_SCRIPT)`): one quick look at the window, which document
 * it shows and whether the readiness marker holds there. The cold-start benchmark looks until the
 * window shows the app's own page before it waits with {@link READY_SCRIPT}, because that wait
 * ends with the document it runs in.
 */
export const PAGE_SCRIPT = `
  const marker = () => {${MARKER}};
  return {
    href: String(location.href),
    timeOrigin: performance.timeOrigin,
    readyAt: marker() ? performance.timeOrigin + performance.now() : null,
  };
`;

/**
 * Runs in the webview (`executeAsyncScript(READY_SCRIPT, timeoutMs)`): waits for the readiness
 * marker, checking every 4 ms, and calls back with a {@link ReadyResult}.
 */
export const READY_SCRIPT = `
  const timeoutMs = arguments[0];
  const done = arguments[arguments.length - 1];
  const marker = () => {${MARKER}};
  const begin = performance.now();
  if (marker()) {
    done({ kind: 'ready', at: performance.timeOrigin + performance.now(), already: true });
    return;
  }
  const poll = () => {
    if (marker()) {
      done({ kind: 'ready', at: performance.timeOrigin + performance.now(), already: false });
    } else if (performance.now() - begin > timeoutMs) {
      done({ kind: 'timeout' });
    } else {
      setTimeout(poll, 4);
    }
  };
  setTimeout(poll, 4);
`;

/**
 * The preview pipeline's debounce (src/editor/preview/pipeline.ts `PREVIEW_DEBOUNCE_MS`; a unit
 * test checks they agree): the time from an edit to its C++ includes it.
 */
export const PREVIEW_DEBOUNCE_MS = 50;

/** A main-thread task the page ran (page clock, milliseconds). */
export interface PageTask {
  readonly start: number;
  readonly ms: number;
}

/** What {@link EDIT_SCRIPT} calls back with (page clock, milliseconds). */
export type EditResult =
  | {
      readonly kind: 'seen';
      /** Before the insertion. */
      readonly begin: number;
      /** After the hook inserted the blocks (Blockly has handled the edit). */
      readonly inserted: number;
      /** When the preview with the marker text was first in the editor's state. */
      readonly seen: number;
      /**
       * The long task that ended just before {@link seen}: the preview pipeline's run (reading the
       * canvas, `canonical()` and `preview()` in WebAssembly, storing the result). `null` when no
       * task of at least the given length ran then.
       */
      readonly task: PageTask | null;
    }
  | { readonly kind: 'error'; readonly message: string }
  | { readonly kind: 'timeout' };

/**
 * Runs in the webview (`executeAsyncScript(EDIT_SCRIPT, hookName, parentId, input, blocks, marker,
 * timeoutMs, longTaskMs)`): inserts `blocks` through the test hook, then checks the code the
 * editor shows (`hook.code()`, from the live preview's result) until it contains `marker`, and
 * calls back with an {@link EditResult}. The check runs between the page's tasks, so it never
 * delays the preview.
 *
 * Meanwhile a heartbeat (a message the page posts to itself again and again) finds the long
 * tasks: between two beats the page ran something else, and a gap of at least `longTaskMs` is a
 * long task. The preview pipeline runs in one task (its WebAssembly calls are synchronous) and
 * puts its result in the store at its end, so the first long task after which the marker is in
 * the code is the pipeline's run: that is how its length is measured from outside the app. (The
 * beat after a long task was posted before anything that task queued, such as React's render, so
 * it runs first and the gap holds that one task.)
 */
export const EDIT_SCRIPT = `
  const [hookName, parentId, input, blocks, marker, timeoutMs, longTaskMs] = arguments;
  const done = arguments[arguments.length - 1];
  const hook = window[hookName];
  if (typeof hook !== 'object' || hook === null) {
    done({ kind: 'error', message: 'The end-to-end hook is not installed' });
    return;
  }
  let finished = false;
  let failed = null;
  const shows = () => {
    try {
      return hook.code().includes(marker);
    } catch (error) {
      failed = String(error);
      return false;
    }
  };
  let inserted = null;
  let task = null;
  const channel = new MessageChannel();
  let last = performance.now();
  channel.port1.onmessage = () => {
    if (finished) {
      return;
    }
    const now = performance.now();
    if (now - last >= longTaskMs && inserted !== null && task === null && shows()) {
      task = { start: last, ms: now - last };
    }
    last = now;
    channel.port2.postMessage(null);
  };
  channel.port2.postMessage(null);
  const finish = (result) => {
    if (!finished) {
      finished = true;
      channel.port1.close();
      done(result);
    }
  };
  const begin = performance.now();
  try {
    hook.insertBlocks(parentId, input, blocks);
  } catch (error) {
    finish({ kind: 'error', message: String(error) });
    return;
  }
  inserted = performance.now();
  const poll = () => {
    if (finished) {
      return;
    }
    const now = performance.now();
    if (shows()) {
      // No beat since the pipeline's task ended: the gap still open is that task.
      if (task === null && now - last >= longTaskMs) {
        task = { start: last, ms: now - last };
      }
      finish({ kind: 'seen', begin, inserted, seen: now, task });
    } else if (failed !== null) {
      finish({ kind: 'error', message: failed });
    } else if (now - begin > timeoutMs) {
      finish({ kind: 'timeout' });
    } else {
      setTimeout(poll, 1);
    }
  };
  setTimeout(poll, 0);
`;

/**
 * The preview pipeline's task of an edit, checked: it began after the insertion and ended by the
 * time the preview was seen; otherwise `null` (the pipeline ran shorter than the long-task
 * threshold, or the result does not fit together).
 */
export function previewTask(result: Extract<EditResult, { kind: 'seen' }>): PageTask | null {
  const { task, seen, inserted } = result;
  if (
    task === null ||
    !(task.ms > 0) ||
    task.start < inserted ||
    task.start + task.ms > seen + 0.001
  ) {
    return null;
  }
  return task;
}

/** What {@link GONE_SCRIPT} calls back with. */
export type GoneResult = 'gone' | 'timeout' | 'error';

/**
 * Runs in the webview (`executeAsyncScript(GONE_SCRIPT, hookName, marker, timeoutMs)`): calls back
 * with `'gone'` once the code the editor shows no longer contains `marker`.
 */
export const GONE_SCRIPT = `
  const [hookName, marker, timeoutMs] = arguments;
  const done = arguments[arguments.length - 1];
  const hook = window[hookName];
  const begin = performance.now();
  const poll = () => {
    let code;
    try {
      code = hook.code();
    } catch (error) {
      done('error');
      return;
    }
    if (!code.includes(marker)) {
      done('gone');
    } else if (performance.now() - begin > timeoutMs) {
      done('timeout');
    } else {
      setTimeout(poll, 5);
    }
  };
  poll();
`;

/** The window property the frame recorder keeps its state in while it runs. */
export const FRAMES_KEY = '__B2C_BENCH_FRAMES__';

/**
 * Runs in the webview (`executeScript(FRAMES_START_SCRIPT, FRAMES_KEY)`): starts recording the
 * timestamps of the animation frames that run while a pointer drags (from the first move with the
 * button down until it is released), until {@link FRAMES_STOP_SCRIPT}. Listeners on the window's
 * capture phase only observe; they change nothing. Returns `false` when a recorder is already
 * running.
 */
export const FRAMES_START_SCRIPT = `
  const key = arguments[0];
  if (Object.prototype.hasOwnProperty.call(window, key)) {
    return false;
  }
  const state = { down: false, dragging: false, stopped: false, frames: [] };
  const onDown = () => {
    state.down = true;
  };
  const onMove = () => {
    if (state.down) {
      state.dragging = true;
    }
  };
  const onUp = () => {
    state.down = false;
    state.dragging = false;
  };
  window.addEventListener('pointerdown', onDown, true);
  window.addEventListener('pointermove', onMove, true);
  window.addEventListener('pointerup', onUp, true);
  window.addEventListener('pointercancel', onUp, true);
  const loop = (time) => {
    if (state.stopped) {
      return;
    }
    if (state.dragging) {
      state.frames.push(time);
    }
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
  state.stop = () => {
    state.stopped = true;
    window.removeEventListener('pointerdown', onDown, true);
    window.removeEventListener('pointermove', onMove, true);
    window.removeEventListener('pointerup', onUp, true);
    window.removeEventListener('pointercancel', onUp, true);
  };
  Object.defineProperty(window, key, { value: state, configurable: true });
  return true;
`;

/**
 * Runs in the webview (`executeScript(FRAMES_STOP_SCRIPT, FRAMES_KEY)`): stops the recorder and
 * returns the frame timestamps it recorded (`null` when none was running).
 */
export const FRAMES_STOP_SCRIPT = `
  const key = arguments[0];
  const state = window[key];
  if (typeof state !== 'object' || state === null) {
    return null;
  }
  state.stop();
  delete window[key];
  return state.frames;
`;

/** What {@link QUIET_SCRIPT} calls back with. */
export type QuietResult = 'quiet' | 'busy';

/**
 * Runs in the webview (`executeAsyncScript(QUIET_SCRIPT, frames, maxFrameMs, timeoutMs)`): calls
 * back with `'quiet'` once `frames` animation frames in a row each came within `maxFrameMs` of the
 * previous one (the page has finished what a drop started, such as its preview), or `'busy'` after
 * `timeoutMs`.
 */
export const QUIET_SCRIPT = `
  const [frames, maxFrameMs, timeoutMs] = arguments;
  const done = arguments[arguments.length - 1];
  const begin = performance.now();
  let previous = null;
  let calm = 0;
  let finished = false;
  const finish = (result) => {
    if (!finished) {
      finished = true;
      done(result);
    }
  };
  const step = (time) => {
    if (finished) {
      return;
    }
    if (previous !== null) {
      calm = time - previous <= maxFrameMs ? calm + 1 : 0;
    }
    previous = time;
    if (calm >= frames) {
      finish('quiet');
    } else if (performance.now() - begin > timeoutMs) {
      finish('busy');
    } else {
      requestAnimationFrame(step);
    }
  };
  requestAnimationFrame(step);
  // Without animation frames (a hidden window) nothing would ever call back.
  setTimeout(() => finish('busy'), timeoutMs + 1000);
`;

/** What {@link PLACE_SCRIPT} returns: where a block is. */
export interface BlockPlace {
  /** The block's bounding box on screen (client coordinates, CSS pixels). */
  readonly box: Rect;
  /**
   * Where the block is on the canvas: the offset of its origin from the canvas's origin, in CSS
   * pixels at the canvas's zoom. Scrolling the canvas leaves it as it is; moving the block changes
   * it by as much as the block moved on screen.
   */
  readonly offset: E2ePoint;
}

/**
 * Runs in the webview (`executeScript(PLACE_SCRIPT, hookName, blockId)`): where a block is (a
 * {@link BlockPlace}), or `null` when it is not on the canvas. The block's SVG group (from the test
 * hook) is placed inside its parent, the canvas Blockly scrolls and zooms, so the difference of
 * their screen transforms is the block's place on the canvas.
 */
export const PLACE_SCRIPT = `
  const [hookName, blockId] = arguments;
  const hook = window[hookName];
  if (typeof hook !== 'object' || hook === null) {
    throw new Error('The end-to-end hook is not installed');
  }
  const block = hook.blockElement(blockId);
  const parent = block === null ? null : block.parentNode;
  if (parent === null || typeof parent.getScreenCTM !== 'function') {
    return null;
  }
  const own = block.getScreenCTM();
  const canvas = parent.getScreenCTM();
  if (own === null || canvas === null) {
    return null;
  }
  const box = block.getBoundingClientRect();
  return {
    box: { left: box.left, top: box.top, right: box.right, bottom: box.bottom },
    offset: { x: own.e - canvas.e, y: own.f - canvas.f },
  };
`;

/** Whether `value` is a list of finite numbers (frame timestamps from the page). */
export function isTimestamps(value: unknown): value is number[] {
  return (
    Array.isArray(value) && value.every((item) => typeof item === 'number' && Number.isFinite(item))
  );
}
