/**
 * What the person types into the console goes to the program (docs/spec/04-user-interface.md
 * §4.5, 02 §2.5.6): the terminal's text is encoded as UTF-8, cut into chunks of at most
 * {@link MAX_RUN_INPUT_BYTES} bytes, base64-encoded and sent with `run_input`, in order and one
 * call at a time. The sender keeps under the backend's limits of 200 calls and 1 MiB a second, and
 * when the backend still says `rateLimited` (for example because the program is not reading its
 * input) it waits and tries again. What waits here is bounded by {@link MAX_PENDING_INPUT_BYTES}.
 */
import { IpcCallError } from '@blocks2cpp/ipc-types';

import { type Clock, sleep } from './clock';

/** The most bytes one `run_input` call carries (02 §2.5.6). */
export const MAX_RUN_INPUT_BYTES = 65_536;

/** The most base64 characters one `run_input` call carries: 4 for every 3 bytes, rounded up. */
export const MAX_RUN_INPUT_BASE64 = 87_384;

/** The most bytes the backend accepts per second and run (02 §2.5.6). */
export const INPUT_BYTES_PER_SECOND = 1024 * 1024;

/** The most calls the backend accepts per second and run (02 §2.5.6). */
export const INPUT_CALLS_PER_SECOND = 200;

/** The most bytes waiting to be sent; typing more while the program does not read is dropped. */
export const MAX_PENDING_INPUT_BYTES = 1024 * 1024;

/** The first wait after a `rateLimited` answer; it doubles up to {@link MAX_RETRY_DELAY_MS}. */
export const FIRST_RETRY_DELAY_MS = 100;

/** The longest wait between two tries of the same input. */
export const MAX_RETRY_DELAY_MS = 1000;

const WINDOW_MS = 1000;
const encoder = new TextEncoder();

/** Standard base64 with padding, as `run_input` takes it. */
export function toBase64(bytes: Uint8Array): string {
  let binary = '';
  // In slices, so the argument list of fromCharCode stays small.
  for (let start = 0; start < bytes.length; start += 0x2000) {
    binary += String.fromCharCode(...bytes.subarray(start, start + 0x2000));
  }
  return btoa(binary);
}

/** `bytes` cut into chunks of at most {@link MAX_RUN_INPUT_BYTES}. */
export function chunkBytes(bytes: Uint8Array, size: number = MAX_RUN_INPUT_BYTES): Uint8Array[] {
  const chunks: Uint8Array[] = [];
  for (let start = 0; start < bytes.length; start += size) {
    chunks.push(bytes.subarray(start, start + size));
  }
  return chunks;
}

/**
 * The `run_input` data for typed text: UTF-8, cut into chunks of at most
 * {@link MAX_RUN_INPUT_BYTES} bytes, each base64-encoded.
 */
export function encodeInput(text: string): string[] {
  return chunkBytes(encoder.encode(text)).map(toBase64);
}

/** Sends one chunk (base64) to the program. */
export type SendInput = (data: string) => Promise<unknown>;

/** Sends typed text to the program; see the module documentation. */
export class InputSender {
  readonly #send: SendInput;
  readonly #clock: Clock;
  /** Bytes waiting, oldest first. */
  #queue: Uint8Array[] = [];
  #queued = 0;
  #pumping = false;
  #ready = false;
  #disposed = false;
  #warned = false;
  #windowStart = Number.NEGATIVE_INFINITY;
  #windowBytes = 0;
  #windowCalls = 0;

  constructor(send: SendInput, clock: Clock) {
    this.#send = send;
    this.#clock = clock;
  }

  /** How many bytes are waiting to be sent. */
  get pendingBytes(): number {
    return this.#queued;
  }

  /** Allows sending (the run's ID is known). */
  ready(): void {
    this.#ready = true;
    void this.#pump();
  }

  /** Queues what the person typed. Dropped (with one warning) when too much is waiting. */
  push(text: string): void {
    if (this.#disposed || text === '') {
      return;
    }
    const bytes = encoder.encode(text);
    if (this.#queued + bytes.length > MAX_PENDING_INPUT_BYTES) {
      if (!this.#warned) {
        this.#warned = true;
        console.warn('The program is not reading its input; typed input was dropped');
      }
      return;
    }
    this.#queue.push(bytes);
    this.#queued += bytes.length;
    void this.#pump();
  }

  /** Drops everything waiting and stops sending (the program ended). */
  dispose(): void {
    this.#disposed = true;
    this.#queue = [];
    this.#queued = 0;
  }

  /** Takes the next chunk: queued bytes, oldest first, up to {@link MAX_RUN_INPUT_BYTES}. */
  #take(): Uint8Array {
    const parts: Uint8Array[] = [];
    let size = 0;
    while (this.#queue.length > 0 && size < MAX_RUN_INPUT_BYTES) {
      const head = this.#queue[0];
      if (head === undefined) {
        break;
      }
      const room = MAX_RUN_INPUT_BYTES - size;
      if (head.length <= room) {
        parts.push(head);
        size += head.length;
        this.#queue.shift();
      } else {
        parts.push(head.subarray(0, room));
        size += room;
        this.#queue[0] = head.subarray(room);
      }
    }
    this.#queued -= size;
    if (parts.length === 1 && parts[0] !== undefined) {
      return parts[0];
    }
    const chunk = new Uint8Array(size);
    let offset = 0;
    for (const part of parts) {
      chunk.set(part, offset);
      offset += part.length;
    }
    return chunk;
  }

  /** Puts a chunk that could not be sent back at the front (unless the sender was disposed). */
  #putBack(chunk: Uint8Array): void {
    if (this.#disposed) {
      return;
    }
    this.#queue.unshift(chunk);
    this.#queued += chunk.length;
  }

  /** How long to wait before `bytes` more may be sent within the backend's limits. */
  #budgetWait(bytes: number): number {
    const now = this.#clock.now();
    if (now - this.#windowStart >= WINDOW_MS) {
      this.#windowStart = now;
      this.#windowBytes = 0;
      this.#windowCalls = 0;
    }
    if (
      this.#windowCalls + 1 > INPUT_CALLS_PER_SECOND ||
      this.#windowBytes + bytes > INPUT_BYTES_PER_SECOND
    ) {
      return this.#windowStart + WINDOW_MS - now;
    }
    this.#windowCalls += 1;
    this.#windowBytes += bytes;
    return 0;
  }

  async #pump(): Promise<void> {
    if (this.#pumping || !this.#ready) {
      return;
    }
    this.#pumping = true;
    let retryDelay = FIRST_RETRY_DELAY_MS;
    try {
      while (!this.#disposed && this.#queue.length > 0) {
        const chunk = this.#take();
        const wait = this.#budgetWait(chunk.length);
        if (wait > 0) {
          this.#putBack(chunk);
          await sleep(this.#clock, wait);
          continue;
        }
        try {
          await this.#send(toBase64(chunk));
          retryDelay = FIRST_RETRY_DELAY_MS;
        } catch (error: unknown) {
          const code = error instanceof IpcCallError ? error.error.code : 'transport';
          if (code === 'rateLimited') {
            this.#putBack(chunk);
            await sleep(this.#clock, retryDelay);
            retryDelay = Math.min(retryDelay * 2, MAX_RETRY_DELAY_MS);
          } else if (code === 'notRunning' || code === 'unknownRun') {
            this.dispose();
          } else {
            console.warn('Typed input could not be sent to the program', code);
          }
        }
      }
    } finally {
      this.#pumping = false;
    }
  }
}
