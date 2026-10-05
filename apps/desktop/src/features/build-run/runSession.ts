/**
 * One run of the program (docs/spec/02-architecture.md §2.4.3, §2.5.3; 07 §7.6.5): the two
 * channels of `run_start` and everything that goes back to the backend.
 *
 * * **Output** (`onOutput`): raw byte batches, numbered from 1 in the order they arrive. Each one
 *   is written to the console, and once the terminal has processed every batch up to `n`, `n` is
 *   acknowledged with `run_ack` (at most every 100 ms, ./acks.ts).
 * * **Events** (`onEvent`): `started` first, then `skipped` and one `exit`. The two channels are
 *   not ordered against each other, so `skipped` and `exit` carry `afterSeq`, the number of
 *   batches sent before them, and are applied only after that many batches have been written:
 *   the "lines skipped" marker lands where the output was dropped, and the exit text shows only
 *   once all the output is on screen. Should batches go missing (they never do), the events are
 *   applied after {@link MISSING_OUTPUT_WAIT_MS} anyway, so the console never stays *Running*.
 * * **Input**: what the person types goes to `run_input` (./input.ts); the terminal's size to
 *   `run_resize`.
 */
import type {
  Containment,
  IpcClient,
  RunEvent,
  RunId,
  RunMode,
  RunResizeRequest,
} from '@blocks2cpp/ipc-types';

import type { TerminalSize } from '../../panels';
import { AckPacer } from './acks';
import { checkRunEvent } from './channel';
import type { Clock } from './clock';
import type { ConsoleBridge } from './consoleBridge';
import { InputSender } from './input';
import { failureCode } from './messages';

/** How long applied events wait for output batches that were sent but never arrived. */
export const MISSING_OUTPUT_WAIT_MS = 2000;

/** The most events waiting for their output (the contract sends a few per run). */
const MAX_WAITING_EVENTS = 1024;

/** What a run reports to its owner, in the order it happens. */
export interface RunSessionHooks {
  /** The program started (`started`, applied at once). */
  onStarted(started: {
    containment: Containment;
    mode: RunMode;
    ideHelpers: boolean;
    at: number;
  }): void;
  /** The program ended (`exit`, applied once its output is written). */
  onExit(exit: Extract<RunEvent, { kind: 'exit' }>): void;
}

/** What a run needs. */
export interface RunSessionOptions {
  readonly ipc: IpcClient;
  readonly bridge: ConsoleBridge;
  readonly clock: Clock;
  readonly hooks: RunSessionHooks;
  /**
   * Text written to the console before the run's first output (a separator between runs), or
   * `null`.
   */
  readonly prelude?: string | null;
}

/** See the module documentation. */
export class RunSession {
  readonly #ipc: IpcClient;
  readonly #bridge: ConsoleBridge;
  readonly #clock: Clock;
  readonly #hooks: RunSessionHooks;
  #prelude: string | null;
  #runId: RunId | null = null;
  /** Batches received. */
  #received = 0;
  /** Every batch up to this one has been written. */
  #written = 0;
  /** Batches written out of order, above {@link #written}. */
  readonly #writtenAbove = new Set<number>();
  /** Events waiting for their output. */
  #waiting: RunEvent[] = [];
  #missingTimer: unknown = null;
  #exitSeen = false;
  #ended = false;
  #detached = false;
  #stopRequested = false;
  readonly #acks: AckPacer;
  readonly #input: InputSender;
  #resizeInFlight = false;
  #pendingSize: TerminalSize | null = null;
  readonly #endedPromise: Promise<void>;
  #resolveEnded: () => void = () => undefined;

  constructor(options: RunSessionOptions) {
    this.#ipc = options.ipc;
    this.#bridge = options.bridge;
    this.#clock = options.clock;
    this.#hooks = options.hooks;
    this.#prelude = options.prelude ?? null;
    this.#acks = new AckPacer((seq) => this.#sendAck(seq), options.clock);
    this.#input = new InputSender((data) => this.#sendInput(data), options.clock);
    this.#endedPromise = new Promise((resolve) => {
      this.#resolveEnded = resolve;
    });
  }

  /** The run's ID, once `run_start` has answered. */
  get runId(): RunId | null {
    return this.#runId;
  }

  /** Whether the run is over for this session: its exit was applied, or it was detached. */
  get hasEnded(): boolean {
    return this.#ended;
  }

  /** Resolves when {@link hasEnded} becomes true. */
  get ended(): Promise<void> {
    return this.#endedPromise;
  }

  /** How many output batches the terminal has written (all of them, in order). */
  get writtenBatches(): number {
    return this.#written;
  }

  /** The `onOutput` channel of `run_start`. */
  readonly onOutput = (bytes: ArrayBuffer): void => {
    if (this.#detached) {
      return;
    }
    const seq = ++this.#received;
    this.#writePrelude();
    const settle = () => {
      this.#markWritten(seq);
    };
    this.#bridge.write(new Uint8Array(bytes)).then(settle, settle);
  };

  /** The `onEvent` channel of `run_start`. */
  readonly onEvent = (message: unknown): void => {
    if (this.#detached || this.#exitSeen) {
      return;
    }
    const event = checkRunEvent(message);
    if (event === null) {
      console.warn('Ignored a run event of an unknown shape');
      return;
    }
    if (this.#waiting.length >= MAX_WAITING_EVENTS && event.kind !== 'exit') {
      return;
    }
    if (event.kind === 'exit') {
      this.#exitSeen = true;
    }
    this.#waiting.push(event);
    this.#drain();
  };

  /** `run_start` answered with the run's ID: acknowledgements, input and resizes may go out. */
  started(runId: RunId): void {
    if (this.#runId !== null) {
      return;
    }
    this.#runId = runId;
    this.#acks.ready();
    this.#input.ready();
    if (this.#stopRequested && !this.#ended) {
      this.#sendStop(runId);
    }
    if (this.#pendingSize !== null) {
      this.#sendResize();
    }
  }

  /** Stops the program (`run_stop`), now or as soon as its ID is known. */
  stop(): void {
    if (this.#ended) {
      return;
    }
    this.#stopRequested = true;
    if (this.#runId !== null) {
      this.#sendStop(this.#runId);
    }
  }

  /** The person typed `data` into the console. */
  typed(data: string): void {
    if (!this.#ended && !this.#detached) {
      this.#input.push(data);
    }
  }

  /** The console's terminal has a new size. */
  resized(size: TerminalSize): void {
    if (this.#ended || this.#detached) {
      return;
    }
    this.#pendingSize = size;
    this.#sendResize();
  }

  /**
   * Stops following the run: no more output, events, acknowledgements or input (the project
   * closed, or a newer run took over). The backend ends the program itself.
   */
  detach(): void {
    if (this.#detached) {
      return;
    }
    this.#detached = true;
    this.#waiting = [];
    this.#finish();
  }

  #writePrelude(): void {
    if (this.#prelude !== null) {
      const prelude = this.#prelude;
      this.#prelude = null;
      this.#bridge.writeText(prelude);
    }
  }

  #markWritten(seq: number): void {
    if (this.#detached) {
      return;
    }
    this.#writtenAbove.add(seq);
    while (this.#writtenAbove.delete(this.#written + 1)) {
      this.#written += 1;
    }
    this.#acks.written(this.#written);
    this.#drain();
  }

  /** Applies the waiting events whose output has been written, in order. */
  #drain(): void {
    for (let next = this.#waiting[0]; next !== undefined; next = this.#waiting[0]) {
      if (next.kind !== 'started' && next.afterSeq > this.#written) {
        break;
      }
      this.#waiting.shift();
      this.#apply(next);
      if (this.#detached) {
        return;
      }
    }
    this.#watchForMissingOutput();
  }

  /**
   * Arms the fallback when an event waits for batches that have not arrived although everything
   * that did arrive is written; disarms it otherwise.
   */
  #watchForMissingOutput(): void {
    const head = this.#waiting[0];
    const missing =
      head !== undefined &&
      head.kind !== 'started' &&
      head.afterSeq > this.#received &&
      this.#written === this.#received;
    if (!missing) {
      if (this.#missingTimer !== null) {
        this.#clock.clearTimeout(this.#missingTimer);
        this.#missingTimer = null;
      }
      return;
    }
    if (this.#missingTimer === null) {
      this.#missingTimer = this.#clock.setTimeout(() => {
        this.#missingTimer = null;
        console.warn('Program output went missing; showing how the run ended anyway');
        const waiting = this.#waiting;
        this.#waiting = [];
        for (const event of waiting) {
          if (this.#detached) {
            return;
          }
          this.#apply(event);
        }
      }, MISSING_OUTPUT_WAIT_MS);
    }
  }

  #apply(event: RunEvent): void {
    switch (event.kind) {
      case 'started':
        this.#writePrelude();
        this.#bridge.setMode(event.mode);
        this.#hooks.onStarted({
          containment: event.containment,
          mode: event.mode,
          ideHelpers: event.ideHelpers,
          at: this.#clock.now(),
        });
        return;
      case 'skipped':
        this.#bridge.console().writeSkipped(event.lines);
        return;
      case 'exit':
        this.#hooks.onExit(event);
        this.#finish();
        return;
    }
  }

  #finish(): void {
    if (this.#ended) {
      return;
    }
    this.#ended = true;
    this.#acks.dispose();
    this.#input.dispose();
    if (this.#missingTimer !== null) {
      this.#clock.clearTimeout(this.#missingTimer);
      this.#missingTimer = null;
    }
    this.#resolveEnded();
  }

  #sendAck(seq: number): Promise<unknown> {
    const runId = this.#runId;
    return runId === null ? Promise.resolve() : this.#ipc.runAck({ runId, seq });
  }

  #sendInput(data: string): Promise<unknown> {
    const runId = this.#runId;
    return runId === null ? Promise.resolve() : this.#ipc.runInput({ runId, data });
  }

  #sendStop(runId: RunId): void {
    this.#ipc.runStop({ runId }).catch((error: unknown) => {
      const code = failureCode(error);
      // A program that has just ended cannot be stopped; its exit event is on its way.
      if (code !== 'notRunning') {
        console.warn('run_stop failed', code);
      }
    });
  }

  /** Sends the newest size, one call at a time. */
  #sendResize(): void {
    const runId = this.#runId;
    const size = this.#pendingSize;
    if (runId === null || size === null || this.#resizeInFlight || this.#ended) {
      return;
    }
    this.#pendingSize = null;
    this.#resizeInFlight = true;
    const request: RunResizeRequest = { runId, cols: size.cols, rows: size.rows };
    this.#ipc
      .runResize(request)
      .catch((error: unknown) => {
        console.debug('run_resize failed', failureCode(error));
      })
      .finally(() => {
        this.#resizeInFlight = false;
        this.#sendResize();
      });
  }
}
