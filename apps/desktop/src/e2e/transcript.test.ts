/** The console transcript the end-to-end tests read: plain text of what the console was given. */
import { describe, expect, it, vi } from 'vitest';

import { ConsoleBridge } from '../features/build-run';
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

/** A console that records nothing, enough for the bridge, and its `write` and `clear` mocks. */
function fakeConsole() {
  const write = vi.fn(() => Promise.resolve());
  const clear = vi.fn();
  const handle: ConsoleHandle = {
    write,
    writeSkipped: vi.fn(),
    clear,
    reset: vi.fn(),
    size: () => ({ cols: 80, rows: 24 }),
    onData: () => () => undefined,
    onResize: () => () => undefined,
    focus: vi.fn(),
  };
  return { handle, write, clear };
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
});
