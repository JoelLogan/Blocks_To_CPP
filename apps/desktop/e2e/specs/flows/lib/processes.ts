/**
 * The operating system's processes, seen from the test runner, for the flows that end the app or
 * its programs (docs/spec/07-toolchain-build-run.md §7.6.2: closing the app always kills the
 * program tree; 05 §5.10: recovery after a crash).
 *
 * - **Linux:** `/proc` (../../../support/processes.ts), with each process's executable read from
 *   `/proc/<pid>/exe`.
 * - **Windows:** `Win32_Process` through Windows PowerShell's `Get-CimInstance`, which gives each
 *   process's executable path and command line (`tasklist` gives neither). `taskkill /F /T` ends a
 *   process tree as a crash would.
 *
 * Paths are compared as the system spells them: a folder may be named through a link or, on
 * Windows, an 8.3 short name (the runners' `TEMP` is `C:\Users\RUNNER~1\…`), so both the given
 * spelling and the resolved one count, and Windows compares without regard to case.
 */
import { execFile } from 'node:child_process';
import { readlinkSync, realpathSync } from 'node:fs';
import path from 'node:path';

import { listProcesses, readProcess, testProcesses } from '../../../support/processes';
import { sleep } from '../../../support/wait';

/** One running process. */
export interface OsProcess {
  readonly pid: number;
  readonly ppid: number;
  /** The executable's absolute path, or `null` when the system does not say (or will not). */
  readonly exe: string | null;
  /** The command line, or `null` when it is not known (always on Linux, where it is not read). */
  readonly commandLine: string | null;
}

/** A process query (PowerShell or `taskkill`) failed. */
export class ProcessQueryError extends Error {
  override readonly name = 'ProcessQueryError';
}

/** How long one PowerShell or `taskkill` call may take. */
const COMMAND_TIMEOUT_MS = 60_000;

/** The most output a PowerShell process listing may have (a few thousand processes fit). */
const MAX_LISTING_BYTES = 64 * 1024 * 1024;

/**
 * How long a check that processes have gone may take: a process listing on Windows starts
 * PowerShell, which takes a few seconds on a busy runner.
 */
export const PROCESS_CHECK_TIMEOUT_MS = 30_000;

/** Makes Windows PowerShell write UTF-8, whatever the console's code page. */
const UTF8_OUTPUT = '[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; ';

/**
 * Lists every process as one JSON array (`@()` keeps a single process an array). Windows
 * PowerShell 5.1, which every Windows has, so nothing needs installing.
 */
const CIM_LISTING =
  "$ErrorActionPreference = 'Stop'; " +
  UTF8_OUTPUT +
  'ConvertTo-Json -Compress -InputObject @(Get-CimInstance -ClassName Win32_Process | ' +
  'Select-Object ProcessId, ParentProcessId, ExecutablePath, CommandLine)';

/** Runs a program and returns its standard output. */
function run(command: string, args: readonly string[]): Promise<string> {
  return new Promise((resolve, reject) => {
    execFile(
      command,
      args,
      {
        timeout: COMMAND_TIMEOUT_MS,
        maxBuffer: MAX_LISTING_BYTES,
        windowsHide: true,
        encoding: 'utf8',
      },
      (error, stdout, stderr) => {
        if (error !== null) {
          reject(
            new ProcessQueryError(`${command} failed: ${error.message} ${stderr.trim()}`.trim(), {
              cause: error,
            }),
          );
        } else {
          resolve(stdout);
        }
      },
    );
  });
}

/** Runs a Windows PowerShell command. */
export function powershell(command: string): Promise<string> {
  return run('powershell.exe', [
    '-NoLogo',
    '-NoProfile',
    '-NonInteractive',
    '-ExecutionPolicy',
    'Bypass',
    '-Command',
    command,
  ]);
}

/** A whole number from a JSON value, or `null`. */
function wholeNumber(value: unknown): number | null {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

/** A non-empty string from a JSON value, or `null`. */
function text(value: unknown): string | null {
  return typeof value === 'string' && value !== '' ? value : null;
}

/**
 * Parses the JSON of {@link CIM_LISTING}: an array of `{ProcessId, ParentProcessId,
 * ExecutablePath, CommandLine}`, or one such object. Entries without a process ID are skipped.
 *
 * @throws ProcessQueryError when the text is not such JSON.
 */
export function parseCimProcesses(json: string): OsProcess[] {
  let value: unknown;
  try {
    value = JSON.parse(json.trim() === '' ? '[]' : json);
  } catch (error: unknown) {
    throw new ProcessQueryError('The process listing is not JSON', { cause: error });
  }
  const entries: unknown[] = Array.isArray(value) ? value : [value];
  const found: OsProcess[] = [];
  for (const entry of entries) {
    if (typeof entry !== 'object' || entry === null) {
      throw new ProcessQueryError('The process listing has an entry that is not an object');
    }
    const record = entry as Record<string, unknown>;
    const pid = wholeNumber(record['ProcessId']);
    if (pid === null) {
      continue;
    }
    found.push({
      pid,
      ppid: wholeNumber(record['ParentProcessId']) ?? 0,
      exe: text(record['ExecutablePath']),
      commandLine: text(record['CommandLine']),
    });
  }
  return found;
}

/** The executable of a Linux process, or `null` (a zombie, or another user's process). */
function linuxExecutable(pid: number): string | null {
  try {
    return readlinkSync(`/proc/${String(pid)}/exe`);
  } catch {
    return null;
  }
}

/** Every process that runs now (on Linux, zombies are left out: they have ended). */
export async function listOsProcesses(): Promise<OsProcess[]> {
  if (process.platform === 'win32') {
    return parseCimProcesses(await powershell(CIM_LISTING));
  }
  return listProcesses()
    .filter((entry) => entry.state !== 'Z' && entry.state !== 'X')
    .map((entry) => ({
      pid: entry.pid,
      ppid: entry.ppid,
      exe: linuxExecutable(entry.pid),
      commandLine: null,
    }));
}

/** The processes below `roots` (children, their children, …), without the roots. */
export function descendants(
  processes: readonly OsProcess[],
  roots: readonly number[],
): OsProcess[] {
  const seen = new Set(roots);
  const found: OsProcess[] = [];
  let added = true;
  // Parents may be listed after their children, so repeat until nothing is added.
  while (added) {
    added = false;
    for (const entry of processes) {
      if (!seen.has(entry.pid) && seen.has(entry.ppid) && entry.pid !== entry.ppid) {
        seen.add(entry.pid);
        found.push(entry);
        added = true;
      }
    }
  }
  return found;
}

/** The spellings a path can have in a process listing (as given, and resolved), normalised. */
export function pathSpellings(
  file: string,
  platform: NodeJS.Platform = process.platform,
  resolve: (file: string) => string = realpathSync.native,
): string[] {
  const spellings = new Set<string>();
  const add = (spelling: string) => {
    const normalised = path.normalize(spelling);
    spellings.add(platform === 'win32' ? normalised.toLowerCase() : normalised);
  };
  add(file);
  try {
    add(resolve(file));
  } catch {
    // It does not exist (any more): the given spelling is all there is.
  }
  return [...spellings];
}

/** Whether `file` is `folder` or inside it, for any of the folder's spellings. */
export function isInside(
  file: string,
  folderSpellings: readonly string[],
  platform: NodeJS.Platform = process.platform,
): boolean {
  const normalised = path.normalize(file);
  const candidate = platform === 'win32' ? normalised.toLowerCase() : normalised;
  const separator = platform === 'win32' ? '\\' : '/';
  return folderSpellings.some(
    (folder) =>
      candidate === folder ||
      candidate.startsWith(folder.endsWith(separator) ? folder : `${folder}${separator}`),
  );
}

/** The processes whose executable is inside `folder` (for example a profile's build cache). */
export async function processesRunningFrom(folder: string): Promise<OsProcess[]> {
  const spellings = pathSpellings(folder);
  return (await listOsProcesses()).filter(
    (entry) => entry.exe !== null && isInside(entry.exe, spellings),
  );
}

/** What identifies the app under test among the processes. */
export interface AppQuery {
  /** The app's executable. */
  readonly executable: string;
  /** Linux: the environment entry only this test's processes have (`B2C_E2E_ROOT=<profile>`). */
  readonly marker: string;
  /** Windows: the port `tauri-driver` listens on (its command line has `--port=<port>`). */
  readonly driverPort: number;
}

/** The command-line argument that names a `tauri-driver`'s port. */
function portArgument(port: number): RegExp {
  return new RegExp(`(?:^|\\s|")--port=${String(port)}(?:$|\\s|")`);
}

/**
 * The app's process ID: on Linux the process with the test's marker whose executable is the
 * app's; on Windows the app's executable below the `tauri-driver` that listens on the port.
 * `null` when there is none (yet, or any more).
 *
 * @throws Error when more than one process matches, which would make a kill ambiguous.
 */
export async function findAppProcess(query: AppQuery): Promise<number | null> {
  const executables = pathSpellings(query.executable);
  const isApp = (exe: string | null): boolean =>
    exe !== null &&
    pathSpellings(exe, process.platform, (file) => file).some((spelling) =>
      executables.includes(spelling),
    );
  let matches: number[];
  if (process.platform === 'win32') {
    const all = await listOsProcesses();
    const port = portArgument(query.driverPort);
    const drivers = all.filter(
      (entry) => entry.commandLine !== null && port.test(entry.commandLine),
    );
    matches = descendants(
      all,
      drivers.map((entry) => entry.pid),
    )
      .filter((entry) => isApp(entry.exe))
      .map((entry) => entry.pid);
  } else {
    matches = testProcesses({ roots: [], marker: query.marker })
      .map((entry) => entry.pid)
      .filter((pid) => isApp(linuxExecutable(pid)));
  }
  if (matches.length > 1) {
    throw new Error(`More than one app process matches: ${matches.join(', ')}`);
  }
  return matches[0] ?? null;
}

/** Whether process `pid` runs (on Linux a zombie has ended: it only waits to be reaped). */
export function isRunning(pid: number): boolean {
  if (process.platform === 'linux') {
    const entry = readProcess(pid);
    return entry !== null && entry.state !== 'Z' && entry.state !== 'X';
  }
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

/** Waits up to `timeout` for process `pid` to end; returns whether it did. */
export async function waitForProcessEnd(pid: number, timeout: number): Promise<boolean> {
  const deadline = Date.now() + timeout;
  while (isRunning(pid)) {
    if (Date.now() >= deadline) {
      return false;
    }
    await sleep(100);
  }
  return true;
}

/**
 * Ends the app as a crash would: `SIGKILL` on Linux (its WebKit processes are left to the driver's
 * clean-up), `taskkill /F /T` on Windows (the process and its WebView2 processes; the Job Object's
 * `KILL_ON_JOB_CLOSE` ends a program it runs).
 */
export async function crashProcess(pid: number): Promise<void> {
  if (process.platform === 'win32') {
    try {
      await run('taskkill.exe', ['/F', '/T', '/PID', String(pid)]);
    } catch (error: unknown) {
      // taskkill also fails when a process of the tree ended by itself meanwhile; only the app
      // still running is a failure.
      if (isRunning(pid)) {
        throw error;
      }
    }
  } else {
    process.kill(pid, 'SIGKILL');
  }
}
