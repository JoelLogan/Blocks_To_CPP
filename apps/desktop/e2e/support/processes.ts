/**
 * The processes a test started, on Linux, read from `/proc`: what is left when a WebDriver session
 * did not end (`session.quit()` failed, hung or never ran).
 *
 * `tauri-driver` 2.1.0 only stops the native driver when it is stopped, and `WebKitWebDriver`,
 * killed, never closes the browser it launched: the app under test. The app, WebKit's network and
 * web processes and anything the app started (g++, the program) would live on, reparented to init,
 * while later tests run. The harness therefore takes the driver's descendants before it stops the
 * driver, adds every process whose environment has the test's own entry (`B2C_E2E_ROOT=<profile>`,
 * which survives reparenting), and kills what is left afterwards (`TauriDriver.stop`, ./driver.ts).
 * It does not move the driver into a process group of its own, so Ctrl+C in a terminal still
 * reaches every process of the run.
 *
 * Nothing here applies to Windows: `tauri-driver` puts itself and everything it starts into a job
 * object that is killed when it exits.
 */
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

import { sleep } from './wait';

/** Where the kernel lists the processes. */
export const PROC_ROOT = '/proc';

/** One process, as `/proc/<pid>/stat` describes it. */
export interface ProcessEntry {
  readonly pid: number;
  readonly ppid: number;
  /** The state letter: `R`, `S`, `D`, `T`, … and `Z` (a zombie) or `X` (dead). */
  readonly state: string;
  /**
   * When the process started, in clock ticks since boot. With the PID it names one process, also
   * after the PID has been reused.
   */
  readonly startTime: string;
}

/** What {@link testProcesses} looks for. */
export interface ProcessQuery {
  /** Processes whose descendants belong to the test (they must still run: see the caller). */
  readonly roots: readonly number[];
  /** An environment entry (`NAME=value`) that only the test's processes have, or `null`. */
  readonly marker: string | null;
}

/**
 * Parses the text of `/proc/<pid>/stat`. The command name (field 2) is in parentheses and may
 * itself contain spaces and parentheses, so the other fields are read after the last `)`.
 * `null` when the text is not such a line.
 */
export function parseStat(pid: number, text: string): ProcessEntry | null {
  const end = text.lastIndexOf(')');
  if (end === -1) {
    return null;
  }
  // From field 3 (the state) on; field 4 is the parent, field 22 the start time.
  const fields = text
    .slice(end + 1)
    .trim()
    .split(/\s+/);
  const state = fields[0];
  const ppid = Number(fields[1]);
  const startTime = fields[19];
  if (
    state === undefined ||
    !/^[A-Za-z]$/.test(state) ||
    !Number.isSafeInteger(ppid) ||
    startTime === undefined ||
    !/^\d+$/.test(startTime)
  ) {
    return null;
  }
  return { pid, ppid, state, startTime };
}

/** Process `pid` as `/proc` describes it now, or `null` when there is none (any more). */
export function readProcess(pid: number, proc = PROC_ROOT): ProcessEntry | null {
  try {
    return parseStat(pid, readFileSync(path.join(proc, String(pid), 'stat'), 'utf8'));
  } catch {
    return null;
  }
}

/** Every process `/proc` lists now (one that ends while it is read is left out). */
export function listProcesses(proc = PROC_ROOT): ProcessEntry[] {
  let names: string[];
  try {
    names = readdirSync(proc);
  } catch {
    return [];
  }
  const found: ProcessEntry[] = [];
  for (const name of names) {
    if (/^\d+$/.test(name)) {
      const entry = readProcess(Number(name), proc);
      if (entry !== null) {
        found.push(entry);
      }
    }
  }
  return found;
}

/** Whether process `pid`'s environment holds exactly `entry` (false when it cannot be read). */
export function hasEnvironmentEntry(pid: number, entry: string, proc = PROC_ROOT): boolean {
  try {
    return readFileSync(path.join(proc, String(pid), 'environ'), 'utf8')
      .split('\0')
      .includes(entry);
  } catch {
    return false;
  }
}

/** The processes of `processes` below `roots` (children, their children, …), without the roots. */
export function descendantsOf(
  processes: readonly ProcessEntry[],
  roots: Iterable<number>,
): ProcessEntry[] {
  const children = new Map<number, ProcessEntry[]>();
  for (const entry of processes) {
    const siblings = children.get(entry.ppid);
    if (siblings === undefined) {
      children.set(entry.ppid, [entry]);
    } else {
      siblings.push(entry);
    }
  }
  const seen = new Set<number>(roots);
  const pending = [...seen];
  const found: ProcessEntry[] = [];
  for (let pid = pending.pop(); pid !== undefined; pid = pending.pop()) {
    for (const child of children.get(pid) ?? []) {
      if (!seen.has(child.pid)) {
        seen.add(child.pid);
        found.push(child);
        pending.push(child.pid);
      }
    }
  }
  return found;
}

/** Whether a state letter means the process has ended (a zombie waits only to be reaped). */
function ended(state: string): boolean {
  return state === 'Z' || state === 'X';
}

/**
 * The test's processes that run now: the descendants of `query.roots`, and every process with the
 * `query.marker` environment entry together with its descendants. This process (the test runner)
 * and init are never among them.
 */
export function testProcesses(query: ProcessQuery, proc = PROC_ROOT): ProcessEntry[] {
  const all = listProcesses(proc).filter((entry) => !ended(entry.state));
  const marker = query.marker;
  const marked =
    marker === null
      ? []
      : all.filter(
          (entry) => entry.pid !== process.pid && hasEnvironmentEntry(entry.pid, marker, proc),
        );
  const found = new Map<number, ProcessEntry>();
  for (const entry of [
    ...marked,
    ...descendantsOf(all, [...query.roots, ...marked.map((entry) => entry.pid)]),
  ]) {
    if (entry.pid !== process.pid && entry.pid > 1) {
      found.set(entry.pid, entry);
    }
  }
  return [...found.values()];
}

/** Whether the process `entry` names still runs: the same PID and start time, and not a zombie. */
export function stillRunning(entry: ProcessEntry, proc = PROC_ROOT): boolean {
  const now = readProcess(entry.pid, proc);
  return now !== null && now.startTime === entry.startTime && !ended(now.state);
}

/**
 * Kills (`SIGKILL`) every process of `entries` that still runs, then waits up to `timeout`
 * milliseconds for them to end. Returns the PIDs still running when the time is up.
 */
export async function killProcesses(
  entries: readonly ProcessEntry[],
  timeout: number,
  proc = PROC_ROOT,
): Promise<number[]> {
  const unique = new Map(
    entries.map((entry) => [`${String(entry.pid)}:${entry.startTime}`, entry]),
  );
  let left = [...unique.values()].filter((entry) => stillRunning(entry, proc));
  for (const entry of left) {
    try {
      process.kill(entry.pid, 'SIGKILL');
    } catch {
      // It ended meanwhile.
    }
  }
  const deadline = Date.now() + timeout;
  for (;;) {
    left = left.filter((entry) => stillRunning(entry, proc));
    if (left.length === 0 || Date.now() >= deadline) {
      return left.map((entry) => entry.pid);
    }
    await sleep(50);
  }
}
