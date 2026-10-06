/** The console transcript the end-to-end tests read: plain text of what the console was given. */
import { describe, expect, it, onTestFinished, vi } from 'vitest';

import { createFakeIpc } from '../app/testing/fixtures';
import { ConsoleBridge } from '../features/build-run';
import { systemClock } from '../features/build-run/clock';
import { RunSession } from '../features/build-run/runSession';
import { FakeConsole, settle } from '../features/build-run/testing';
import type { ConsoleHandle } from '../panels';
import { ConsoleTranscript, MAX_TRANSCRIPT_CHARS, plainText, tapConsoleBridge } from './transcript';

const encode = (text: string) => new TextEncoder().encode(text);

describe('plainText', () => {
  it('removes terminal control sequences and keeps the text', () => {
    expect(plainText('\u001b[1;32mGreen\u001b[0m text')).toBe('Green text');
    expect(plainText('\u001b[?25l\u001b[?2004hHidden cursor')).toBe('Hidden cursor');
    expect(plainText('\u001b]0;title\u0007after')).toBe('after');
    expect(plainText('\u001b]8;;https://example.com\u001b\\link\u001b]8;;\u001b\\')).toBe('link');
    expect(plainText('\u001b(Bcharset\u001bc')).toBe('charset');
  });

  it('turns CR LF into LF, drops lone CRs and other controls, keeps tabs', () => {
    expect(plainText('a\r\nb\r\n')).toBe('a\nb\n');
    expect(plainText('50%\r100%\n')).toBe('50%100%\n');
    expect(plainText('bell\u0007 back\u0008space\ttab\u009b')).toBe('bell backspace\ttab');
  });
});

describe('ConsoleTranscript', () => {
  it('decodes UTF-8 split between batches', () => {
    const transcript = new ConsoleTranscript();
    const bytes = encode('Grüße ✓\n');
    transcript.appendBytes(bytes.slice(0, 3));
    transcript.appendBytes(bytes.slice(3, 9));
    transcript.appendBytes(bytes.slice(9));
    expect(transcript.text()).toBe('Grüße ✓\n');
  });

  it('keeps the newest characters up to its limit, and forgets everything when cleared', () => {
    const transcript = new ConsoleTranscript(10);
    transcript.appendText('0123456789');
    transcript.appendBytes(encode('abc'));
    expect(transcript.text()).toBe('3456789abc');
    transcript.clear();
    expect(transcript.text()).toBe('');
    expect(new ConsoleTranscript(0).text()).toBe('');
    expect(MAX_TRANSCRIPT_CHARS).toBe(1_048_576);
  });
});

/** A console that records nothing, enough for the bridge, and its mocks. */
function fakeConsole() {
  const write = vi.fn(() => Promise.resolve());
  const writeSkipped = vi.fn();
  const clear = vi.fn();
  const reset = vi.fn();
  const focus = vi.fn();
  const handle: ConsoleHandle = {
    write,
    writeSkipped,
    clear,
    reset,
    size: () => ({ cols: 80, rows: 24 }),
    onData: () => () => undefined,
    onResize: () => () => undefined,
    focus,
  };
  return { handle, write, writeSkipped, clear, reset, focus };
}

describe('tapConsoleBridge', () => {
  it('records output and separators, and clears with the console', async () => {
    const bridge = new ConsoleBridge();
    const { handle, write, clear } = fakeConsole();
    bridge.attach(handle);
    const transcript = new ConsoleTranscript();
    const untap = tapConsoleBridge(bridge, transcript);

    await bridge.write(encode('Your guess: 50\r\n'));
    bridge.writeText('\u001b[2m── New run ──\u001b[0m\r\n');
    expect(transcript.text()).toBe('Your guess: 50\n── New run ──\n');
    // The bridge still does its own work.
    expect(write).toHaveBeenCalledTimes(2);
    expect(bridge.used).toBe(true);

    bridge.clear();
    expect(transcript.text()).toBe('');
    expect(clear).toHaveBeenCalledOnce();
    await bridge.write(encode('again'));
    bridge.reset();
    expect(transcript.text()).toBe('');
    await bridge.write(encode('more'));
    bridge.cleared();
    expect(transcript.text()).toBe('');

    untap();
    await bridge.write(encode('not recorded'));
    expect(transcript.text()).toBe('');
    expect(Object.hasOwn(bridge, 'write')).toBe(false);
  });

  it('records the skipped-lines markers in order with the output', async () => {
    const bridge = new ConsoleBridge();
    const { handle, writeSkipped } = fakeConsole();
    bridge.attach(handle);
    const transcript = new ConsoleTranscript();
    const untap = tapConsoleBridge(bridge, transcript);

    await bridge.write(encode('line 1\r\n'));
    // The run session writes the marker straight into the console (RunSession, `skipped`).
    bridge.console().writeSkipped(1234);
    await bridge.write(encode('line 1236\r\n'));
    bridge.console().writeSkipped(0);
    expect(transcript.text()).toBe(
      'line 1\n\n … 1,234 lines skipped \nline 1236\n\n … output skipped \n',
    );
    // The console itself still writes it, and the bridge hands out one handle per console.
    expect(writeSkipped).toHaveBeenNthCalledWith(1, 1234);
    expect(writeSkipped).toHaveBeenNthCalledWith(2, 0);
    expect(bridge.console()).toBe(bridge.console());
    // A count that is not one (the console shows no marker) adds nothing.
    bridge.console().writeSkipped(-1);
    expect(transcript.text().endsWith('line 1236\n\n … output skipped \n')).toBe(true);

    // Without a console the marker is recorded like other output, and nothing breaks.
    bridge.attach(fakeConsole().handle)();
    bridge.console().writeSkipped(2);
    expect(transcript.text().endsWith(' … 2 lines skipped \n')).toBe(true);

    untap();
    expect(Object.hasOwn(bridge, 'console')).toBe(false);
    bridge.console().writeSkipped(3);
    expect(transcript.text()).not.toContain('3 lines');
  });

  it('records the marker a run writes for skipped output, between the batches around it', async () => {
    vi.useFakeTimers();
    onTestFinished(() => {
      vi.useRealTimers();
    });
    const terminal = new FakeConsole();
    const bridge = new ConsoleBridge();
    bridge.attach(terminal);
    const transcript = new ConsoleTranscript();
    const untap = tapConsoleBridge(bridge, transcript);
    const ipc = createFakeIpc();
    ipc.runAck.mockResolvedValue({});
    const run = new RunSession({
      ipc,
      bridge,
      clock: systemClock,
      hooks: { onStarted: vi.fn(), onExit: vi.fn() },
    });
    run.onOutput(encode('first\r\n').slice().buffer);
    run.onEvent({ kind: 'skipped', lines: 5, afterSeq: 1 });
    run.onOutput(encode('last\r\n').slice().buffer);
    await settle();
    expect(terminal.skipped).toEqual([5]);
    expect(transcript.text()).toBe('first\n\n … 5 lines skipped \nlast\n');
    untap();
  });

  it('passes every other console call through to the attached console', () => {
    const bridge = new ConsoleBridge();
    const { handle, write, clear, reset, focus } = fakeConsole();
    bridge.attach(handle);
    const untap = tapConsoleBridge(bridge, new ConsoleTranscript());
    const tapped = bridge.console();
    const listener = vi.fn();
    void tapped.write(encode('x'));
    tapped.clear();
    tapped.reset();
    tapped.focus();
    tapped.onData(listener)();
    tapped.onResize(listener)();
    expect(tapped.size()).toEqual({ cols: 80, rows: 24 });
    expect(write).toHaveBeenCalledOnce();
    expect(clear).toHaveBeenCalledOnce();
    expect(reset).toHaveBeenCalledOnce();
    expect(focus).toHaveBeenCalledOnce();
    untap();
  });
});
