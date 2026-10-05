/** The seam between the console panel and the run controller. */
import { describe, expect, it, vi } from 'vitest';

import { ConsoleBridge, DEFAULT_TERMINAL_SIZE } from './consoleBridge';
import { FakeConsole } from './testing';

describe('ConsoleBridge', () => {
  it('stands in with a console that discards output while none is attached', async () => {
    const bridge = new ConsoleBridge();
    expect(bridge.attached).toBe(false);
    await expect(bridge.write(new Uint8Array([65]))).resolves.toBeUndefined();
    expect(bridge.used).toBe(false);
    expect(bridge.console().size()).toEqual(DEFAULT_TERMINAL_SIZE);
    bridge.console().writeSkipped(3);
    bridge.console().focus();
    bridge.clear();
    expect(bridge.console().onData(() => undefined)).toBeTypeOf('function');
    expect(bridge.console().onResize(() => undefined)).toBeTypeOf('function');
  });

  it('forwards typing and resizes from the attached console to the connected listener', () => {
    const bridge = new ConsoleBridge();
    const terminal = new FakeConsole();
    const detach = bridge.attach(terminal);
    const onInput = vi.fn();
    const onResize = vi.fn();
    const disconnect = bridge.connect({ onInput, onResize });

    terminal.type('a');
    terminal.resizeTo({ cols: 10, rows: 5 });
    expect(onInput).toHaveBeenCalledWith('a');
    expect(onResize).toHaveBeenCalledWith({ cols: 10, rows: 5 });

    disconnect();
    terminal.type('b');
    expect(onInput).toHaveBeenCalledTimes(1);

    bridge.connect({ onInput, onResize });
    detach();
    terminal.type('c');
    expect(onInput).toHaveBeenCalledTimes(1);
    expect(bridge.attached).toBe(false);
  });

  it('keeps the newest attachment when an older one detaches late', () => {
    const bridge = new ConsoleBridge();
    const first = new FakeConsole();
    const second = new FakeConsole();
    const detachFirst = bridge.attach(first);
    bridge.attach(second);
    detachFirst();
    expect(bridge.console()).toBe(second);
    const onInput = vi.fn();
    bridge.connect({ onInput, onResize: vi.fn() });
    first.type('old');
    second.type('new');
    expect(onInput.mock.calls).toEqual([['new']]);
  });

  it('tracks whether the console was written to since it was cleared', async () => {
    const bridge = new ConsoleBridge();
    const terminal = new FakeConsole();
    bridge.attach(terminal);
    bridge.writeText('hi');
    await Promise.resolve();
    expect(terminal.text).toBe('hi');
    expect(bridge.used).toBe(true);
    bridge.cleared();
    expect(bridge.used).toBe(false);
    bridge.writeText('again');
    bridge.clear();
    expect(terminal.clears).toBe(1);
    expect(bridge.used).toBe(false);
  });

  it('tells subscribers when the run mode changes', () => {
    const bridge = new ConsoleBridge();
    const listener = vi.fn();
    const unsubscribe = bridge.subscribe(listener);
    expect(bridge.mode()).toBe('pty');
    bridge.setMode('pty');
    expect(listener).not.toHaveBeenCalled();
    bridge.setMode('pipes');
    expect(bridge.mode()).toBe('pipes');
    expect(listener).toHaveBeenCalledTimes(1);
    unsubscribe();
    bridge.setMode('pty');
    expect(listener).toHaveBeenCalledTimes(1);
  });
});
