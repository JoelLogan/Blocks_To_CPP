/**
 * Whether the keyboard is in use (./inputModality.ts): `data-input="keyboard"` on the root element
 * from a Tab key press until the next pointer press, which app.css reads to show the focus ring of
 * a page's heading only to keyboard users.
 */
import { afterEach, describe, expect, it } from 'vitest';

import { INPUT_ATTRIBUTE, KEYBOARD_INPUT, trackInputModality } from './inputModality';

const root = document.documentElement;

function press(key: string, init: KeyboardEventInit = {}): void {
  document.body.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, ...init }));
}

function pointerDown(target: EventTarget = document.body): void {
  target.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
}

let stop: (() => void) | null = null;

afterEach(() => {
  stop?.();
  stop = null;
  root.removeAttribute(INPUT_ATTRIBUTE);
});

describe('trackInputModality', () => {
  it('starts without the keyboard mark', () => {
    stop = trackInputModality(document);
    expect(root.hasAttribute(INPUT_ATTRIBUTE)).toBe(false);
  });

  it('marks the keyboard in use on Tab and Shift+Tab, and clears it on a pointer press', () => {
    stop = trackInputModality(document);
    press('Tab');
    expect(root.getAttribute(INPUT_ATTRIBUTE)).toBe(KEYBOARD_INPUT);

    pointerDown();
    expect(root.hasAttribute(INPUT_ATTRIBUTE)).toBe(false);

    press('Tab', { shiftKey: true });
    expect(root.getAttribute(INPUT_ATTRIBUTE)).toBe(KEYBOARD_INPUT);
  });

  it('leaves the mark as it is for other keys', () => {
    stop = trackInputModality(document);
    press('a');
    press('Enter');
    expect(root.hasAttribute(INPUT_ATTRIBUTE)).toBe(false);

    press('Tab');
    press('Escape');
    expect(root.getAttribute(INPUT_ATTRIBUTE)).toBe(KEYBOARD_INPUT);
  });

  it('sees presses whose handlers stop them', () => {
    stop = trackInputModality(document);
    const stopper = document.createElement('button');
    document.body.append(stopper);
    const swallow = (event: Event) => {
      event.stopPropagation();
    };
    stopper.addEventListener('keydown', swallow);
    stopper.addEventListener('pointerdown', swallow);
    try {
      stopper.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
      expect(root.getAttribute(INPUT_ATTRIBUTE)).toBe(KEYBOARD_INPUT);
      pointerDown(stopper);
      expect(root.hasAttribute(INPUT_ATTRIBUTE)).toBe(false);
    } finally {
      stopper.remove();
    }
  });

  it('stops listening and removes the mark when stopped', () => {
    const stopTracking = trackInputModality(document);
    press('Tab');
    stopTracking();
    expect(root.hasAttribute(INPUT_ATTRIBUTE)).toBe(false);

    press('Tab');
    expect(root.hasAttribute(INPUT_ATTRIBUTE)).toBe(false);
  });
});
