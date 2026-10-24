import { describe, expect, it, vi } from 'vitest';

import { installEventTargetReceiverShim } from './dom-shims';

/** An event no browser or library sends, so only the test's listeners see it. */
const EVENT = 'b2c-test-event';

describe('installEventTargetReceiverShim', () => {
  it('lets an unbound document.addEventListener add its listener to the window', () => {
    // setup.ts installed the shim; a second install changes nothing.
    installEventTargetReceiverShim();
    const listener = vi.fn();
    // Unbound on purpose, as Blockly calls it.
    // eslint-disable-next-line @typescript-eslint/unbound-method
    const add = document.addEventListener;
    // eslint-disable-next-line @typescript-eslint/unbound-method
    const remove = document.removeEventListener;

    add(EVENT, listener);
    window.dispatchEvent(new Event(EVENT));
    expect(listener).toHaveBeenCalledTimes(1);

    remove(EVENT, listener);
    window.dispatchEvent(new Event(EVENT));
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it('leaves calls with a receiver unchanged', () => {
    const element = document.createElement('div');
    const listener = vi.fn();

    element.addEventListener(EVENT, listener);
    element.dispatchEvent(new Event(EVENT));
    window.dispatchEvent(new Event(EVENT));
    expect(listener).toHaveBeenCalledTimes(1);

    element.removeEventListener(EVENT, listener);
    element.dispatchEvent(new Event(EVENT));
    expect(listener).toHaveBeenCalledTimes(1);
  });
});
