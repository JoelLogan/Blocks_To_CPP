/**
 * Acknowledging program output (docs/spec/02-architecture.md §2.5.3, 07 §7.6.5): the console
 * tells the backend with `run_ack {runId, seq}` which output batches it has written, at most once
 * every {@link ACK_INTERVAL_MS}. While more than 4 MiB is unacknowledged, the backend keeps only
 * the tail of the output, so the console never falls hopelessly behind.
 */
import type { Clock } from './clock';

/** The shortest time between two acknowledgements (02 §2.5.3). */
export const ACK_INTERVAL_MS = 100;

/** Sends one acknowledgement. */
export type SendAck = (seq: number) => Promise<unknown>;

/**
 * Paces acknowledgements: it sends the highest batch number written so far, at most once every
 * {@link ACK_INTERVAL_MS} and never while the previous acknowledgement is still on its way.
 * Nothing is sent until {@link AckPacer.ready} (the run's ID has to be known first).
 */
export class AckPacer {
  readonly #send: SendAck;
  readonly #clock: Clock;
  readonly #interval: number;
  /** The highest batch written. */
  #written = 0;
  /** The highest batch acknowledged (or being acknowledged). */
  #acked = 0;
  #lastSentAt = Number.NEGATIVE_INFINITY;
  #timer: unknown = null;
  #inFlight = false;
  #ready = false;
  #disposed = false;

  constructor(send: SendAck, clock: Clock, interval: number = ACK_INTERVAL_MS) {
    this.#send = send;
    this.#clock = clock;
    this.#interval = interval;
  }

  /** The highest batch number acknowledged so far. */
  get acked(): number {
    return this.#acked;
  }

  /** Allows sending (the run's ID is known). */
  ready(): void {
    this.#ready = true;
    this.#schedule();
  }

  /** Every batch up to `seq` has been written to the terminal. */
  written(seq: number): void {
    if (seq <= this.#written) {
      return;
    }
    this.#written = seq;
    this.#schedule();
  }

  /** Stops sending; a timer still waiting is cancelled. */
  dispose(): void {
    this.#disposed = true;
    if (this.#timer !== null) {
      this.#clock.clearTimeout(this.#timer);
      this.#timer = null;
    }
  }

  #schedule(): void {
    if (
      this.#disposed ||
      !this.#ready ||
      this.#inFlight ||
      this.#timer !== null ||
      this.#written <= this.#acked
    ) {
      return;
    }
    const wait = this.#lastSentAt + this.#interval - this.#clock.now();
    if (wait <= 0) {
      this.#flush();
      return;
    }
    this.#timer = this.#clock.setTimeout(() => {
      this.#timer = null;
      this.#flush();
    }, wait);
  }

  #flush(): void {
    if (this.#disposed || this.#written <= this.#acked) {
      return;
    }
    const seq = this.#written;
    this.#acked = seq;
    this.#lastSentAt = this.#clock.now();
    this.#inFlight = true;
    this.#send(seq)
      .catch((error: unknown) => {
        // A late or refused acknowledgement only affects flow control; the next one carries on.
        console.debug('run_ack failed', error instanceof Error ? error.message : error);
      })
      .finally(() => {
        this.#inFlight = false;
        this.#schedule();
      });
  }
}
