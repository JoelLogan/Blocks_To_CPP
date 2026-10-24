/**
 * Waiting until Blockly has delivered a change before an undo (editor.ts `DELIVERY_SCRIPT`), run on
 * a fake page (./fake-page.ts) where the change is made as Blockly makes it: the task that changes
 * the canvas (a pointerup) queues the change's events and asks for an animation frame, and that
 * frame queues their delivery in a zero timeout. The script starts in a later task, once the test
 * has seen the change on the canvas, and must call back only after the delivery.
 */
import { describe, expect, it } from 'vitest';

import { DELIVERY_SCRIPT } from './editor';
import { FakeLoop, runScript } from './fake-page';

/** One case of {@link deliveryOrder}. */
interface Case {
  /** Whether zero timeouts are clamped to 4 ms, as nested timers can be. */
  readonly clampTimers: boolean;
  /** How long the frame after the change takes to render it, in milliseconds. */
  readonly frameMs: number;
  /** When the script starts, in milliseconds after the change. */
  readonly scriptAfterMs: number;
}

/**
 * What happened on a fake page, in order: the change is made 5 ms after a frame, the script
 * starts `scriptAfterMs` later, and the change's delivery queues a zero timeout of its own (a
 * listener's). `called back` is missing when the script never called back.
 */
function deliveryOrder(script: string, page: Case, frames = true): string[] {
  const loop = new FakeLoop();
  loop.clampTimers = page.clampTimers;
  const globals = loop.globals();
  const requestFrame = globals['requestAnimationFrame'] as (callback: () => void) => void;
  const later = globals['setTimeout'] as (run: () => void, ms?: number) => void;
  const seen: string[] = [];
  // The frame before the change, so that the next one comes 16 ms after it.
  requestFrame(() => undefined);
  loop.run(() => loop.now > 0);
  if (!frames) {
    loop.frameEvery = null;
  }
  const changeAt = loop.now + 5;
  // The change: Blockly 12.5.1 delivers its events in `requestAnimationFrame(() =>
  // setTimeout(fireNow, 0))` (core/events/utils.ts fireInternal).
  loop.post(() => {
    seen.push('changed');
    requestFrame(() => {
      loop.now += page.frameMs;
      later(() => {
        seen.push('delivered');
        later(() => seen.push('listened'), 0);
      }, 0);
    });
  }, 5);
  loop.post(
    () => {
      seen.push('script');
      runScript(script, globals, [], () => seen.push('called back'));
    },
    changeAt + page.scriptAfterMs - loop.now,
  );
  loop.run(() => seen.includes('called back'));
  // Whatever is still queued runs too, so a call back that came too early shows in the order.
  loop.run(() => false);
  return seen;
}

describe('waiting until Blockly has delivered a change', () => {
  it.each<Case>([
    { clampTimers: false, frameMs: 2, scriptAfterMs: 1 },
    { clampTimers: true, frameMs: 2, scriptAfterMs: 1 },
    { clampTimers: false, frameMs: 40, scriptAfterMs: 1 },
    { clampTimers: true, frameMs: 40, scriptAfterMs: 1 },
    // A frame came between the change and the script, and the delivery is still queued.
    { clampTimers: true, frameMs: 2, scriptAfterMs: 12 },
  ])(
    'calls back after the delivery and what it queued (timers clamped: $clampTimers, frame $frameMs ms, script $scriptAfterMs ms after the change)',
    (page) => {
      expect(deliveryOrder(DELIVERY_SCRIPT, page)).toEqual([
        'changed',
        'script',
        'delivered',
        'listened',
        'called back',
      ]);
    },
  );

  it('never calls back without animation frames, so the script timeout fails the test', () => {
    const page: Case = { clampTimers: true, frameMs: 2, scriptAfterMs: 1 };
    expect(deliveryOrder(DELIVERY_SCRIPT, page, false)).toEqual(['changed', 'script']);
  });
});
