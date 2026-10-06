/**
 * The seam between the Console tab and the run controller. The connected console panel
 * (src/app/panels.tsx) attaches its {@link ConsoleHandle} here, and the build and run feature
 * connects to it: program output goes to the attached terminal, and what the person types and the
 * terminal's size come back. Without an attached console the bridge stands in with a terminal
 * that discards output at once, so acknowledgements carry on and a run never waits for a panel.
 *
 * The bridge also carries how the running program's terminal is connected (`pty` or `pipes`, from
 * the `started` run event), which the console needs for line endings.
 */
import type { RunMode } from '@blocks2cpp/ipc-types';

import type { ConsoleHandle, TerminalSize } from '../../panels';

/** The size a run starts with when no console is attached (a classic terminal). */
export const DEFAULT_TERMINAL_SIZE: TerminalSize = Object.freeze({ cols: 80, rows: 24 });

/** A console that shows nothing: output is discarded at once, nobody types. */
const DETACHED_CONSOLE: ConsoleHandle = Object.freeze({
  write: () => Promise.resolve(),
  writeSkipped: () => undefined,
  clear: () => undefined,
  reset: () => undefined,
  size: () => DEFAULT_TERMINAL_SIZE,
  onData: () => () => undefined,
  onResize: () => () => undefined,
  focus: () => undefined,
});

/** What the run controller hears from the console. */
export interface ConsoleListener {
  /** The person typed (or pasted) `data`. */
  onInput(data: string): void;
  /** The terminal's fitted size changed. */
  onResize(size: TerminalSize): void;
}

/** See the module documentation. */
export class ConsoleBridge {
  #handle: ConsoleHandle | null = null;
  #unsubscribe: (() => void) | null = null;
  #listener: ConsoleListener | null = null;
  #mode: RunMode = 'pty';
  #used = false;
  readonly #modeListeners = new Set<() => void>();

  /**
   * Attaches the console panel's handle (the newest attachment wins). Returns the function that
   * detaches it again; it does nothing once another handle has been attached.
   */
  attach(handle: ConsoleHandle): () => void {
    this.#unsubscribe?.();
    this.#handle = handle;
    const typed = handle.onData((data) => {
      this.#listener?.onInput(data);
    });
    const resized = handle.onResize((size) => {
      this.#listener?.onResize(size);
    });
    const unsubscribe = () => {
      typed();
      resized();
    };
    this.#unsubscribe = unsubscribe;
    return () => {
      if (this.#handle === handle) {
        unsubscribe();
        this.#handle = null;
        this.#unsubscribe = null;
      }
    };
  }

  /** Whether a console panel is attached. */
  get attached(): boolean {
    return this.#handle !== null;
  }

  /** The attached console, or one that discards everything. */
  console(): ConsoleHandle {
    return this.#handle ?? DETACHED_CONSOLE;
  }

  /** Connects the run controller (the newest connection wins). Returns the disconnecting function. */
  connect(listener: ConsoleListener): () => void {
    this.#listener = listener;
    return () => {
      if (this.#listener === listener) {
        this.#listener = null;
      }
    };
  }

  /**
   * Writes program output to the attached console; resolves when the terminal has processed it
   * (at once when none is attached).
   */
  write(bytes: Uint8Array): Promise<void> {
    if (this.#handle !== null) {
      this.#used = true;
    }
    return this.console().write(bytes);
  }

  /**
   * Writes text of our own (the terminal reset and separator before a run) into the attached
   * console, in order with the output. It does not count as output for {@link used}. Nothing
   * happens without a console.
   */
  writeText(text: string): void {
    void this.console().write(new TextEncoder().encode(text));
  }

  /** Whether program output has been written to the attached console since it was last cleared. */
  get used(): boolean {
    return this.#used;
  }

  /** Clears the attached console (output already queued is written after the clear). */
  clear(): void {
    this.console().clear();
    this.cleared();
  }

  /**
   * Starts the attached console afresh for another project: the output of the old one that is
   * still queued is dropped and the terminal is fully reset, modes included.
   */
  reset(): void {
    this.console().reset();
    this.cleared();
  }

  /** The console was cleared (by its own Clear button). */
  cleared(): void {
    this.#used = false;
  }

  /** How the running program's terminal is connected. */
  readonly mode = (): RunMode => this.#mode;

  /** Sets how the program's terminal is connected (from the `started` run event). */
  setMode(mode: RunMode): void {
    if (mode === this.#mode) {
      return;
    }
    this.#mode = mode;
    for (const listener of [...this.#modeListeners]) {
      listener();
    }
  }

  /** Calls `listener` when the mode changes; returns the unsubscriber (for useSyncExternalStore). */
  readonly subscribe = (listener: () => void): (() => void) => {
    this.#modeListeners.add(listener);
    return () => {
      this.#modeListeners.delete(listener);
    };
  };
}

/** The app's console bridge, shared by the console panel and the build and run feature. */
export const consoleBridge = new ConsoleBridge();
