/**
 * What the console was given, as plain text, for the end-to-end tests: the program's output (with
 * the terminal's echo of what was typed) and the console's own separators, in order. xterm.js's
 * DOM renderer only has the rows on screen, so the tests read the whole run from here and check
 * the screen separately.
 *
 * The transcript listens on the console bridge (features/build-run/consoleBridge.ts) by wrapping
 * the bridge's own methods on the instance; nothing else changes, and {@link tapConsoleBridge}'s
 * clean-up restores them. Terminal control sequences are removed when the text is read.
 */
import type { ConsoleBridge } from '../features/build-run';

/** The most characters the transcript keeps; older output is dropped from the front. */
export const MAX_TRANSCRIPT_CHARS = 1_048_576;

/* eslint-disable no-control-regex -- these patterns exist to remove terminal control characters. */
/** CSI sequences (`ESC [ … final byte`), including private ones such as `ESC [ ? 25 l`. */
const CSI = /\u001b\[[0-?]*[ -/]*[@-~]/g;
/** OSC sequences (`ESC ] … BEL` or `ESC ] … ESC \`), such as titles and hyperlinks. */
const OSC = /\u001b\][^\u0007\u001b]*(?:\u0007|\u001b\\)/g;
/** Other escape sequences: a designator with one parameter byte, or a single character. */
const ESC = /\u001b(?:[()*+][\s\S]|[\s\S])/g;
/** C0 and C1 controls other than tab and line feed (carriage returns are handled first). */
const CONTROLS = /[\u0000-\u0008\u000b-\u001f\u007f-\u009f]/g;
/* eslint-enable no-control-regex */

/**
 * `text` without terminal control sequences: CR LF and lone CRs become LF-only line ends (a lone
 * carriage return, which moves back to the start of the line, is dropped), escape sequences and
 * other control characters are removed, and tabs and line feeds stay.
 */
export function plainText(text: string): string {
  return text
    .replace(OSC, '')
    .replace(CSI, '')
    .replace(ESC, '')
    .replace(/\r\n/g, '\n')
    .replace(/\r/g, '')
    .replace(CONTROLS, '');
}

/** The console's text, bounded to {@link MAX_TRANSCRIPT_CHARS}; see the module comment. */
export class ConsoleTranscript {
  /** Decodes the output's UTF-8, keeping a character split between two batches. */
  #decoder = new TextDecoder('utf-8');
  #raw = '';
  readonly #limit: number;

  constructor(limit = MAX_TRANSCRIPT_CHARS) {
    this.#limit = Math.max(1, Math.floor(limit));
  }

  /** Adds a batch of program output. */
  appendBytes(bytes: Uint8Array): void {
    this.#add(this.#decoder.decode(bytes, { stream: true }));
  }

  /** Adds text the console writes itself (a separator). */
  appendText(text: string): void {
    this.#add(text);
  }

  /** Forgets everything (the console was cleared or reset). */
  clear(): void {
    this.#raw = '';
    this.#decoder = new TextDecoder('utf-8');
  }

  /** The transcript as plain text (see {@link plainText}). */
  text(): string {
    return plainText(this.#raw);
  }

  #add(text: string): void {
    this.#raw += text;
    if (this.#raw.length > this.#limit) {
      this.#raw = this.#raw.slice(this.#raw.length - this.#limit);
    }
  }
}

/** The bridge methods {@link tapConsoleBridge} wraps. */
type Tapped = 'write' | 'writeText' | 'cleared';

/**
 * Records everything the console bridge gives the console into `transcript`, and clears it when
 * the console is cleared or reset. Returns the function that restores the bridge.
 */
export function tapConsoleBridge(bridge: ConsoleBridge, transcript: ConsoleTranscript): () => void {
  const write = bridge.write.bind(bridge);
  const writeText = bridge.writeText.bind(bridge);
  const cleared = bridge.cleared.bind(bridge);
  const wrappers: Pick<ConsoleBridge, Tapped> = {
    write: (bytes) => {
      transcript.appendBytes(bytes);
      return write(bytes);
    },
    writeText: (text) => {
      transcript.appendText(text);
      writeText(text);
    },
    // `clear()` and `reset()` both end in `cleared()`, as does the panel's own Clear button.
    cleared: () => {
      transcript.clear();
      cleared();
    },
  };
  for (const [name, wrapper] of Object.entries(wrappers)) {
    Object.defineProperty(bridge, name, {
      value: wrapper,
      configurable: true,
      writable: true,
    });
  }
  return () => {
    for (const name of Object.keys(wrappers)) {
      Reflect.deleteProperty(bridge, name);
    }
  };
}
