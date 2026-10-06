import { FitAddon } from '@xterm/addon-fit';
import { Terminal, type ITheme } from '@xterm/xterm';
import '@xterm/xterm/css/xterm.css';
import { useEffect, useId, useImperativeHandle, useRef, useState, type Ref } from 'react';

import '../panels.css';
import type { RunMode } from '../types';
import {
  formatElapsed,
  headerStatus,
  NOTICE_DETAIL,
  NOTICE_TEXT,
  skippedMarker,
  type ConsoleHeader,
} from './header';
import { LinkDialog } from './LinkDialog';
import { safeLinkUrl } from './links';
import { OutputScheduler } from './outputScheduler';

export type { ConsoleHeader, ConsoleNotice, ConsoleState } from './header';

/** The scrollback range of the Settings page (04 §4.5) and its default. */
export const MIN_SCROLLBACK_LINES = 1000;
export const MAX_SCROLLBACK_LINES = 100_000;
export const DEFAULT_SCROLLBACK_LINES = 10_000;

/** The terminal sizes `run_resize` accepts (02 §2.5): 2–1000 columns, 1–1000 rows. */
export const COLS_RANGE = { min: 2, max: 1000 } as const;
export const ROWS_RANGE = { min: 1, max: 1000 } as const;

/** A terminal size in character cells. */
export interface TerminalSize {
  readonly cols: number;
  readonly rows: number;
}

/** What the console offers the run controller (through the component's `ref`). */
export interface ConsoleHandle {
  /**
   * Writes program output. Resolves when the terminal has processed it: the moment to acknowledge
   * the batch with `run_ack`. Writes reach the terminal at most 60 times a second.
   */
  write(bytes: Uint8Array): Promise<void>;
  /**
   * Writes the "… N lines skipped" marker (with `lines` 0, "… output skipped": only part of one
   * line was dropped), in order with the output already written.
   */
  writeSkipped(lines: number): void;
  /** Clears the terminal (output already queued is written after the clear). */
  clear(): void;
  /**
   * Starts the terminal afresh for another project: output still queued is dropped, and the
   * terminal is fully reset (screen, scrollback, colours, cursor, alternate screen and modes) once
   * it has processed what it was already given.
   */
  reset(): void;
  /** The terminal's current size. */
  size(): TerminalSize;
  /** Calls `callback` with what the person types (or pastes); returns an unsubscribe function. */
  onData(callback: (data: string) => void): () => void;
  /** Calls `callback` when the fitted size changes; returns an unsubscribe function. */
  onResize(callback: (size: TerminalSize) => void): () => void;
  /** Moves the keyboard focus into the terminal. */
  focus(): void;
}

/** The props of {@link ConsolePanel}. */
export interface ConsolePanelProps {
  /** The header: state, exit, elapsed time and notices. */
  header: ConsoleHeader;
  /** The scrollback cap from the settings, clamped to 1,000–100,000 lines. */
  scrollbackLines: number;
  /** ■ Stop. */
  onStop: () => void;
  /** ⟲ Run again. */
  onRunAgain: () => void;
  /** Clear (the terminal is cleared as well). */
  onClear: () => void;
  /**
   * How the program's terminal is connected (`started.mode`). In `pipes` mode a bare line feed
   * also returns the cursor, as no terminal driver translates it. Defaults to `pty`.
   */
  mode?: RunMode;
  /** Receives the {@link ConsoleHandle}. */
  ref?: Ref<ConsoleHandle>;
}

/** The scrollback to use for a setting: clamped to the allowed range, the default if invalid. */
export function clampScrollback(lines: number): number {
  if (!Number.isFinite(lines)) {
    return DEFAULT_SCROLLBACK_LINES;
  }
  return Math.min(Math.max(Math.round(lines), MIN_SCROLLBACK_LINES), MAX_SCROLLBACK_LINES);
}

const FALLBACK_THEME = {
  background: '#161a22',
  foreground: '#e6e9f0',
  cursor: '#ffc266',
  selectionBackground: 'rgba(138, 180, 255, 0.35)',
} as const;

/** The terminal colours from the panel tokens (panels.css), or their defaults. */
function terminalTheme(element: Element): ITheme {
  const style = getComputedStyle(element);
  const token = (name: string, fallback: string) => {
    const value = style.getPropertyValue(name).trim();
    return value === '' ? fallback : value;
  };
  return {
    background: token('--b2c-console-bg', FALLBACK_THEME.background),
    foreground: token('--b2c-console-fg', FALLBACK_THEME.foreground),
    cursor: token('--b2c-console-cursor', FALLBACK_THEME.cursor),
    selectionBackground: token('--b2c-console-selection', FALLBACK_THEME.selectionBackground),
  };
}

/** Fits the terminal to its element, within the sizes `run_resize` accepts. */
function fitTerminal(terminal: Terminal, fit: FitAddon): void {
  const proposed = fit.proposeDimensions();
  if (
    proposed === undefined ||
    !Number.isFinite(proposed.cols) ||
    !Number.isFinite(proposed.rows)
  ) {
    return;
  }
  const cols = Math.min(Math.max(Math.floor(proposed.cols), COLS_RANGE.min), COLS_RANGE.max);
  const rows = Math.min(Math.max(Math.floor(proposed.rows), ROWS_RANGE.min), ROWS_RANGE.max);
  if (cols !== terminal.cols || rows !== terminal.rows) {
    terminal.resize(cols, rows);
  }
}

/** Calls every listener; one that throws is reported without stopping the others or xterm. */
function notify<T>(listeners: ReadonlySet<(value: T) => void>, value: T): void {
  for (const listener of listeners) {
    try {
      listener(value);
    } catch (error) {
      queueMicrotask(() => {
        throw error;
      });
    }
  }
}

/** Adds a listener to a set and returns the function that removes it. */
function subscribe<T>(listeners: Set<T>, listener: T): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** The key that always moves the focus out of the terminal, as the terminal's description says. */
export const LEAVE_CONSOLE_KEYS = 'Ctrl+Tab';

/**
 * What the terminal does with a key event, so it is never a keyboard trap (WCAG 2.1.2):
 *
 * * `terminal`: xterm handles it (while a program runs, Tab and Shift+Tab go to the program, as
 *   in any terminal).
 * * `browser`: xterm ignores it and the browser's default runs: with no program running, Tab and
 *   Shift+Tab move the focus like on any other control.
 * * `leave`: Ctrl+Tab (with or without Shift) moves the focus to the console's header.
 * * `ignore`: xterm ignores it and nothing else happens (the `keypress` and `keyup` of Ctrl+Tab).
 */
export function consoleKeyAction(
  event: Pick<KeyboardEvent, 'type' | 'key' | 'ctrlKey' | 'altKey' | 'metaKey'>,
  programRunning: boolean,
): 'terminal' | 'browser' | 'leave' | 'ignore' {
  if (event.key !== 'Tab') {
    return 'terminal';
  }
  if (event.ctrlKey && !event.altKey && !event.metaKey) {
    return event.type === 'keydown' ? 'leave' : 'ignore';
  }
  return programRunning ? 'terminal' : 'browser';
}

/**
 * The Console tab (docs/spec/04-user-interface.md §4.5): an xterm.js terminal (its DOM renderer,
 * so the text is real DOM text) for the running program, with a header for the state, elapsed
 * time, notices, ■ Stop, ⟲ Run again and Clear.
 *
 * Program output is only ever terminal cells. Clipboard writes by escape sequence (OSC 52) are
 * swallowed, and there is no clipboard add-on. A link (OSC 8) is accepted only for `http` and
 * `https`; activating it shows the full URL with *Copy link* and opens nothing.
 *
 * The terminal is never a keyboard trap (see {@link consoleKeyAction}): with no program running,
 * Tab and Shift+Tab move the focus as usual; while one runs they go to the program, and Ctrl+Tab
 * moves the focus to the header's first enabled button. The terminal's input announces that key
 * through `aria-describedby`.
 */
export function ConsolePanel({
  header,
  scrollbackLines,
  onStop,
  onRunAgain,
  onClear,
  mode = 'pty',
  ref,
}: ConsolePanelProps) {
  const host = useRef<HTMLDivElement>(null);
  const headerBar = useRef<HTMLDivElement>(null);
  const keysNoteId = useId();
  const terminal = useRef<Terminal | null>(null);
  const [scheduler] = useState(() => new OutputScheduler());
  const [dataListeners] = useState(() => new Set<(data: string) => void>());
  const [resizeListeners] = useState(() => new Set<(size: TerminalSize) => void>());
  const [link, setLink] = useState<string | null>(null);

  useEffect(() => {
    const element = host.current;
    if (element === null) {
      return;
    }
    const term = new Terminal({
      allowProposedApi: false,
      cursorBlink: false,
      fontFamily:
        "ui-monospace, 'Cascadia Mono', 'Segoe UI Mono', 'DejaVu Sans Mono', 'Liberation Mono', Menlo, Consolas, monospace",
      fontSize: 13,
      theme: terminalTheme(element),
      linkHandler: {
        allowNonHttpProtocols: false,
        activate: (_event, target) => {
          const url = safeLinkUrl(target);
          if (url !== null) {
            setLink(url);
          }
        },
      },
    });
    // OSC 52 asks the terminal to write to the clipboard: never from a program's output.
    const clipboardWrites = term.parser.registerOscHandler(52, () => true);
    const fit = new FitAddon();
    term.loadAddon(fit);
    const typed = term.onData((data) => {
      notify(dataListeners, data);
    });
    const resized = term.onResize(({ cols, rows }) => {
      notify(resizeListeners, { cols, rows });
    });
    // xterm cancels every Tab it sees, so without this the focus could never leave by keyboard.
    // Returning false makes xterm leave the event alone. Typing is enabled only while a program
    // runs (see `disableStdin` below).
    term.attachCustomKeyEventHandler((event) => {
      switch (consoleKeyAction(event, term.options.disableStdin !== true)) {
        case 'terminal':
          return true;
        case 'leave':
          event.preventDefault();
          headerBar.current?.querySelector<HTMLElement>('button:not([disabled])')?.focus();
          return false;
        case 'browser':
        case 'ignore':
          return false;
      }
    });
    term.open(element);
    term.textarea?.setAttribute('aria-describedby', keysNoteId);
    fitTerminal(term, fit);
    const observer =
      typeof ResizeObserver === 'undefined'
        ? null
        : new ResizeObserver(() => {
            fitTerminal(term, fit);
          });
    observer?.observe(element);

    terminal.current = term;
    scheduler.attach({
      write: (data, done) => {
        term.write(data, done);
      },
      clear: () => {
        term.clear();
      },
      reset: () => {
        term.reset();
      },
    });
    return () => {
      scheduler.attach(null);
      terminal.current = null;
      observer?.disconnect();
      typed.dispose();
      resized.dispose();
      clipboardWrites.dispose();
      // xterm.js 5.5 syncs its viewport in a timer that `open` starts and that fails once the
      // terminal is disposed. Timers run in order, so disposing in a later one is always safe.
      globalThis.setTimeout(() => {
        term.dispose();
      }, 0);
    };
  }, [scheduler, dataListeners, resizeListeners, keysNoteId]);

  useEffect(() => {
    if (terminal.current !== null) {
      terminal.current.options.scrollback = clampScrollback(scrollbackLines);
    }
  }, [scrollbackLines]);

  useEffect(() => {
    if (terminal.current !== null) {
      terminal.current.options.convertEol = mode === 'pipes';
    }
  }, [mode]);

  const running = header.state === 'running';
  useEffect(() => {
    if (terminal.current !== null) {
      // Typing only goes somewhere while a program runs.
      terminal.current.options.disableStdin = !running;
    }
  }, [running]);

  useImperativeHandle(
    ref,
    (): ConsoleHandle => ({
      write: (bytes) => scheduler.write(bytes),
      writeSkipped: (lines) => {
        const marker = skippedMarker(lines);
        if (marker !== null) {
          void scheduler.writeText(marker);
        }
      },
      clear: () => {
        scheduler.clear();
      },
      reset: () => {
        scheduler.reset();
      },
      size: () => ({
        cols: terminal.current?.cols ?? 80,
        rows: terminal.current?.rows ?? 24,
      }),
      onData: (callback) => subscribe(dataListeners, callback),
      onResize: (callback) => subscribe(resizeListeners, callback),
      focus: () => {
        terminal.current?.focus();
      },
    }),
    [scheduler, dataListeners, resizeListeners],
  );

  const status = headerStatus(header);

  return (
    <div className="b2c-panel b2c-console-panel" data-testid="console-panel">
      <div
        className="b2c-panel-bar b2c-console-header"
        data-testid="console-header"
        ref={headerBar}
      >
        <span
          className={`b2c-console-state b2c-console-state-${status.tone}`}
          role="status"
          data-testid="console-state"
        >
          {status.icon !== '' && <span aria-hidden="true">{status.icon} </span>}
          {status.text}
        </span>
        {header.state !== 'idle' && (
          <span className="b2c-console-elapsed" data-testid="console-elapsed">
            Elapsed {formatElapsed(header.elapsedMs)}
          </span>
        )}
        {header.notices.map((notice) => (
          // A disclosure rather than a tooltip, so the explanation opens by keyboard too.
          <details key={notice} className="b2c-console-notice" data-testid="console-notice">
            <summary>{NOTICE_TEXT[notice]}</summary>
            <p className="b2c-console-notice-detail">{NOTICE_DETAIL[notice]}</p>
          </details>
        ))}
        <span className="b2c-panel-status">
          <button type="button" disabled={!running} onClick={onStop}>
            <span aria-hidden="true">■ </span>Stop
          </button>{' '}
          <button type="button" disabled={header.state === 'idle'} onClick={onRunAgain}>
            <span aria-hidden="true">⟲ </span>Run again
          </button>{' '}
          <button
            type="button"
            onClick={() => {
              scheduler.clear();
              onClear();
            }}
          >
            Clear
          </button>
        </span>
      </div>
      <div className="b2c-console-terminal" ref={host} data-testid="console-terminal" />
      <span id={keysNoteId} className="b2c-panel-visually-hidden">
        {LEAVE_CONSOLE_KEYS} leaves the console.
      </span>
      <LinkDialog
        url={link}
        onClose={() => {
          setLink(null);
          terminal.current?.focus();
        }}
      />
    </div>
  );
}
