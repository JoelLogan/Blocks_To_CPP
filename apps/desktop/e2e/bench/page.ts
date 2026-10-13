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

/**
 * A User Timing span the app measured while an interaction was timed (src/lib/timing.ts: one
 * `performance.measure` entry named `b2c:…` per span; page clock, milliseconds).
 */
export interface PageSpan {
  readonly name: string;
  readonly start: number;
  readonly ms: number;
}

/** The most spans {@link EDIT_SCRIPT} and {@link TASKS_SETTLE_SCRIPT} return. */
export const MAX_SPANS = 1_000;

/**
 * Page-script code that observes the app's `b2c:` User Timing measures from now on into `spans`
 * (a `PerformanceObserver`, where the page has one: the app keeps only the latest entry of each
 * name in the browser's buffer). `takeSpans()` adds what the observer has not delivered yet and
 * stops it. The observer only reads; without one, `spans` stays empty.
 */
const SPANS_OBSERVER = `
  const spans = [];
  const keepSpans = (entries) => {
    for (const entry of entries) {
      if (typeof entry.name === 'string' && entry.name.startsWith('b2c:')
          && typeof entry.startTime === 'number' && typeof entry.duration === 'number'
          && spans.length < ${String(MAX_SPANS)}) {
        spans.push({ name: entry.name.slice(0, 80), start: entry.startTime, ms: entry.duration });
      }
    }
  };
  let spanObserver = null;
  try {
    if (typeof PerformanceObserver === 'function') {
      spanObserver = new PerformanceObserver((list) => keepSpans(list.getEntries()));
      spanObserver.observe({ entryTypes: ['measure'] });
    }
  } catch (error) {
    spanObserver = null;
  }
  const takeSpans = () => {
    if (spanObserver !== null) {
      try {
        if (typeof spanObserver.takeRecords === 'function') {
          keepSpans(spanObserver.takeRecords());
        }
        spanObserver.disconnect();
      } catch (error) {
        // The spans kept so far are all there is.
      }
      spanObserver = null;
    }
    return spans.slice();
  };
`;

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
      /**
       * When the page had finished what the edit started: the end of the last long task before
       * it was quiet for the settle time (at least {@link seen}). {@link seen} without a settle
       * time.
       */
      readonly settled: number;
      /**
       * The longest task from the insertion (which is part of it) until {@link settled}; `null`
       * when none was as long as a long task.
       */
      readonly longest: PageTask | null;
      /** The app's spans from just before the insertion until the script called back. */
      readonly spans: readonly PageSpan[];
    }
  | { readonly kind: 'error'; readonly message: string }
  | { readonly kind: 'timeout' };

/**
 * Runs in the webview (`executeAsyncScript(EDIT_SCRIPT, hookName, parentId, input, blocks, marker,
 * timeoutMs, longTaskMs, settleMs)`): inserts `blocks` through the test hook, then checks the
 * code the editor shows (`hook.code()`, from the live preview's result) until it contains
 * `marker`, and calls back with an {@link EditResult}. The check runs between the page's tasks, so
 * it never delays the preview. With a `settleMs` above 0 it then waits until no long task has run
 * for that long, so that what the edit started after its preview (the C++ panel's update) is part
 * of the result; without one it calls back as soon as the marker shows.
 *
 * Meanwhile a heartbeat (a message the page posts to itself again and again) finds the long
 * tasks: between two beats the page ran something else, and a gap of at least `longTaskMs` is a
 * long task. The preview pipeline runs in one task (its WebAssembly calls are synchronous) and
 * puts its result in the store at its end, so the first long task after which the marker is in
 * the code is the pipeline's run: that is how its length is measured from outside the app. (The
 * beat after a long task was posted before anything that task queued, such as React's render, so
 * it runs first and the gap holds that one task.) The insertion runs in this script's own task,
 * before the first beat, so the first gap holds it. The app's User Timing spans of the edit are
 * observed meanwhile (`spans`).
 */
export const EDIT_SCRIPT = `
  const [hookName, parentId, input, blocks, marker, timeoutMs, longTaskMs, settleMs] = arguments;
  const done = arguments[arguments.length - 1];
  const hook = window[hookName];
  if (typeof hook !== 'object' || hook === null) {
    done({ kind: 'error', message: 'The end-to-end hook is not installed' });
    return;
  }
  const settle = typeof settleMs === 'number' && settleMs > 0 ? settleMs : 0;
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
  let longest = null;
  let lastLongEnd = null;
  let seen = null;
  const noteLong = (start, ms, end) => {
    if (longest === null || ms > longest.ms) {
      longest = { start, ms };
    }
    lastLongEnd = lastLongEnd === null ? end : Math.max(lastLongEnd, end);
  };
  const channel = new MessageChannel();
  let last = performance.now();
  channel.port1.onmessage = () => {
    if (finished) {
      return;
    }
    const now = performance.now();
    if (now - last >= longTaskMs) {
      noteLong(last, now - last, now);
    }
    if (now - last >= longTaskMs && inserted !== null && task === null && shows()) {
      task = { start: last, ms: now - last };
    }
    last = now;
    channel.port2.postMessage(null);
  };
  channel.port2.postMessage(null);
  ${SPANS_OBSERVER}
  const finish = (result) => {
    if (!finished) {
      finished = true;
      channel.port1.close();
      const kept = takeSpans();
      done(result.kind === 'seen' ? { ...result, spans: kept } : result);
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
  const seenResult = (settled) => ({ kind: 'seen', begin, inserted, seen, task, settled, longest });
  const poll = () => {
    if (finished) {
      return;
    }
    const now = performance.now();
    // No beat since a long task ended: the gap still open is that task.
    const open = now - last >= longTaskMs;
    if (seen === null) {
      if (shows()) {
        if (task === null && open) {
          task = { start: last, ms: now - last };
        }
        if (open) {
          noteLong(last, now - last, now);
        }
        seen = now;
        if (settle === 0) {
          finish(seenResult(now));
          return;
        }
      } else if (failed !== null) {
        finish({ kind: 'error', message: failed });
        return;
      } else if (now - begin > timeoutMs) {
        finish({ kind: 'timeout' });
        return;
      }
    } else {
      if (open) {
        noteLong(last, now - last, now);
      }
      const busyUntil = Math.max(seen, lastLongEnd === null ? seen : lastLongEnd);
      if (now - busyUntil >= settle) {
        finish(seenResult(busyUntil));
        return;
      }
      if (now - seen > timeoutMs) {
        finish({ kind: 'timeout' });
        return;
      }
    }
    setTimeout(poll, 1);
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

/** What {@link RUN_OUTPUT_SCRIPT} returns. */
export interface RunOutput {
  /** How many lines of the console's transcript have the text asked about. */
  readonly lines: number;
  /** Whether the transcript has a "… N lines skipped" marker (output dropped, 07 §7.6.5). */
  readonly skipped: boolean;
  /** The transcript's last 200 characters, without the white space at its end. */
  readonly tail: string;
}

/**
 * Runs in the webview (`executeScript(RUN_OUTPUT_SCRIPT, hookName, text)`): a {@link RunOutput}
 * of the console's transcript (the hook's `consoleText()`), counted in the page so that only the
 * counts cross WebDriver. Lines are counted by what they contain, not compared whole: Windows'
 * pseudoconsole may repaint a line with cursor moves, which the transcript leaves out.
 */
export const RUN_OUTPUT_SCRIPT = `
  const [hookName, needle] = arguments;
  const text = String(window[hookName].consoleText());
  let lines = 0;
  for (const row of text.split('\\n')) {
    if (row.includes(needle)) {
      lines += 1;
    }
  }
  return {
    lines: lines,
    skipped: /… ([\\d,]+) lines? skipped|… output skipped/.test(text),
    tail: text.trimEnd().slice(-200),
  };
`;

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

/** The window property the task recorder of a click keeps its state in while it runs. */
export const TASKS_KEY = '__B2C_BENCH_TASKS__';

/** The most long tasks {@link TASKS_START_SCRIPT} keeps. */
export const MAX_TASKS = 1_000;

/**
 * Runs in the webview (`executeScript(TASKS_START_SCRIPT, TASKS_KEY, longTaskMs)`): starts timing
 * the page's tasks for a click that follows, until {@link TASKS_SETTLE_SCRIPT}. A heartbeat (a
 * message the page posts to itself again and again, as in {@link EDIT_SCRIPT}) notes every gap of
 * at least `longTaskMs` between its beats: a task, or the page's rendering, ran then. A listener
 * on the window's capture phase notes when the pointer is first pressed, and the app's User
 * Timing spans are observed; both only observe. Returns `false` when a recorder is already
 * running.
 */
export const TASKS_START_SCRIPT = `
  const [key, longTaskMs] = arguments;
  if (Object.prototype.hasOwnProperty.call(window, key)) {
    return false;
  }
  const state = { longTaskMs, tasks: [], pressed: null, stopped: false, last: performance.now() };
  const channel = new MessageChannel();
  channel.port1.onmessage = () => {
    if (state.stopped) {
      return;
    }
    const now = performance.now();
    if (now - state.last >= longTaskMs && state.tasks.length < ${String(MAX_TASKS)}) {
      state.tasks.push({ start: state.last, ms: now - state.last });
    }
    state.last = now;
    channel.port2.postMessage(null);
  };
  channel.port2.postMessage(null);
  const onDown = () => {
    if (state.pressed === null) {
      state.pressed = performance.now();
    }
  };
  window.addEventListener('pointerdown', onDown, true);
  ${SPANS_OBSERVER}
  state.stop = () => {
    state.stopped = true;
    channel.port1.close();
    window.removeEventListener('pointerdown', onDown, true);
    return takeSpans();
  };
  Object.defineProperty(window, key, { value: state, configurable: true });
  return true;
`;

/** What {@link TASKS_SETTLE_SCRIPT} calls back with (page clock, milliseconds). */
export type ClickResult =
  | {
      readonly kind: 'settled';
      /** When the pointer was pressed. */
      readonly pressed: number;
      /**
       * The end of the last long task before the page was quiet for the settle time, or, when
       * that came first, when the element waited for showed the text waited for.
       */
      readonly settled: number;
      /** When the element waited for showed the text waited for; `null` when none was. */
      readonly until: number | null;
      /** The long tasks from the start of the recording, in order. */
      readonly tasks: readonly PageTask[];
      /** The app's spans from the start of the recording until it stopped. */
      readonly spans: readonly PageSpan[];
    }
  | { readonly kind: 'no-press' }
  | { readonly kind: 'busy' }
  | { readonly kind: 'error'; readonly message: string };

/**
 * Runs in the webview (`executeAsyncScript(TASKS_SETTLE_SCRIPT, TASKS_KEY, settleMs, timeoutMs)`):
 * waits until the pointer has been pressed and then no long task has run for `settleMs`, stops
 * the recorder of {@link TASKS_START_SCRIPT} and calls back with a {@link ClickResult}: `no-press`
 * when no press came within `timeoutMs`, `busy` when the page did not settle within `timeoutMs`
 * of the press.
 *
 * With two more arguments, a CSS selector and a regular expression (its source), the page counts
 * as busy until the first element the selector finds has shown text that the expression does not
 * match and then text that it matches, after the press (a click on ► Run: the console's state
 * leaves the last run's *Finished* and comes back to it); `until` is when it did. The element is
 * read at each check, every millisecond or so between the page's tasks.
 */
export const TASKS_SETTLE_SCRIPT = `
  const args = Array.prototype.slice.call(arguments, 0, -1);
  const [key, settleMs, timeoutMs] = args;
  const done = arguments[arguments.length - 1];
  const state = window[key];
  if (typeof state !== 'object' || state === null || typeof state.stop !== 'function') {
    done({ kind: 'error', message: 'No task recorder is running' });
    return;
  }
  let until = null;
  if (args.length >= 5) {
    try {
      until = { selector: String(args[3]), pattern: new RegExp(String(args[4])), left: false, at: null };
    } catch (error) {
      state.stop();
      delete window[key];
      done({ kind: 'error', message: 'Not a regular expression: ' + String(args[4]) });
      return;
    }
  }
  const begin = performance.now();
  let openUntil = null;
  const finish = (result) => {
    const spans = state.stop();
    delete window[key];
    done(result.kind === 'settled' ? { ...result, spans } : result);
  };
  const poll = () => {
    const now = performance.now();
    if (state.pressed === null) {
      if (now - begin > timeoutMs) {
        finish({ kind: 'no-press' });
      } else {
        setTimeout(poll, 1);
      }
      return;
    }
    if (until !== null && until.at === null) {
      const element = document.querySelector(until.selector);
      const text = element === null ? '' : String(element.textContent || '');
      if (!until.pattern.test(text)) {
        until.left = true;
      } else if (until.left) {
        until.at = now;
      }
    }
    // No beat since a long task ended: the gap still open is that task, busy until now.
    if (now - state.last >= state.longTaskMs) {
      openUntil = now;
    }
    let busyUntil = Math.max(state.pressed, openUntil === null ? state.pressed : openUntil);
    for (const task of state.tasks) {
      busyUntil = Math.max(busyUntil, task.start + task.ms);
    }
    const waiting = until !== null && until.at === null;
    if (until !== null && until.at !== null) {
      busyUntil = Math.max(busyUntil, until.at);
    }
    if (!waiting && now - busyUntil >= settleMs) {
      finish({
        kind: 'settled',
        pressed: state.pressed,
        settled: busyUntil,
        until: until === null ? null : until.at,
        tasks: state.tasks.slice(),
      });
    } else if (now - state.pressed > timeoutMs) {
      finish({ kind: 'busy' });
    } else {
      setTimeout(poll, 1);
    }
  };
  setTimeout(poll, 0);
`;

/**
 * The longest task after the press of a click (one that ended after it, so the task that handled
 * the press counts): `null` when none was long.
 */
export function longestAfterPress(
  result: Extract<ClickResult, { kind: 'settled' }>,
): PageTask | null {
  let longest: PageTask | null = null;
  for (const task of result.tasks) {
    if (task.start + task.ms > result.pressed && (longest === null || task.ms > longest.ms)) {
      longest = task;
    }
  }
  return longest;
}

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

/** The window global under which {@link POINTER_RECORD_SCRIPT} keeps the last press and release. */
export const POINTER_KEY = '__B2C_BENCH_POINTER__';

/**
 * Runs in the webview (`executeScript(POINTER_RECORD_SCRIPT, POINTER_KEY)`): from now on, keeps
 * where the last pointerdown and pointerup happened and on which element, for a report
 * on a drag that went wrong ({@link DROP_REPORT_SCRIPT}). Installed once; two listeners that only
 * store a few values, so they cost the drag nothing measurable.
 */
export const POINTER_RECORD_SCRIPT = `
  const key = arguments[0];
  if (window[key] !== undefined) {
    return;
  }
  const record = { down: null, up: null };
  window[key] = record;
  const describe = (element) => {
    if (!(element instanceof Element)) {
      return String(element);
    }
    const name = element.getAttribute('class');
    return element.tagName.toLowerCase() + (name ? '.' + name.trim().split(/\\s+/).join('.') : '');
  };
  for (const type of ['pointerdown', 'pointerup']) {
    window.addEventListener(
      type,
      (event) => {
        record[type === 'pointerdown' ? 'down' : 'up'] = {
          x: Math.round(event.clientX),
          y: Math.round(event.clientY),
          target: describe(event.target),
        };
      },
      true,
    );
  }
`;

/**
 * Runs in the webview (`executeScript(DROP_REPORT_SCRIPT, POINTER_KEY, x, y)`): what the page
 * looked like at a drop point, for the message of a drag that went wrong: the window's size and
 * pixel ratio, the elements under the point, where the trash can, the flyout and the zoom
 * controls are, and the last press and release ({@link POINTER_RECORD_SCRIPT}).
 */
export const DROP_REPORT_SCRIPT = `
  const [key, x, y] = arguments;
  const describe = (element) => {
    if (!(element instanceof Element)) {
      return String(element);
    }
    const name = element.getAttribute('class');
    return element.tagName.toLowerCase() + (name ? '.' + name.trim().split(/\\s+/).join('.') : '');
  };
  const rect = (selector) => {
    const element = document.querySelector(selector);
    if (element === null) {
      return null;
    }
    const box = element.getBoundingClientRect();
    return [box.left, box.top, box.right, box.bottom].map(Math.round);
  };
  const under = [];
  for (let element = document.elementFromPoint(x, y); element !== null && under.length < 4; element = element.parentElement) {
    under.push(describe(element));
  }
  return {
    viewport: [window.innerWidth, window.innerHeight, window.devicePixelRatio],
    under,
    trash: rect('.blocklyTrash'),
    flyout: rect('svg.blocklyToolboxFlyout'),
    zoom: rect('[data-testid="canvas-controls"]'),
    pointer: window[key] ?? null,
  };
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
