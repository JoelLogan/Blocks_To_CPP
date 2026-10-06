/**
 * "No compiler process ever starts" (docs/spec/08-security.md §8.13, item 1): a watch over the
 * process list while a test runs, and the build cache folders a build would create.
 *
 * - Linux: every 100 ms, the processes below the app under test (found once by the test's own
 *   environment entry `B2C_E2E_ROOT=<profile>`, ../../../support/processes.ts) whose program is
 *   a compiler, assembler or linker (by `argv[0]` and by `comm`). A compiler the app starts is
 *   its child, also inside a `systemd-run --scope` (which execs in place).
 * - Windows: every 250 ms, `tasklist` image names (the list has no parents; the runner runs
 *   nothing else that compiles while the test runs).
 *
 * A sample can miss a process that lives less than the interval, so a test also checks that the
 * build cache has no build folder: the backend creates it before it starts the compiler.
 */
import { execFile } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

import { descendantsOf, hasEnvironmentEntry, listProcesses } from '../../../support/processes';

/**
 * The programs of a C++ build: the driver (`g++`, `c++`, `gcc`, with a target prefix such as
 * `x86_64-linux-gnu-` and a version suffix such as `-13`), its passes (`cc1plus`, `cc1`,
 * `collect2`, `lto1`, `lto-wrapper`), the assembler and the linker, with or without `.exe`.
 */
const COMPILER_PROGRAM =
  /^(?:[a-z0-9_.]+(?:-[a-z0-9_.]+)*-)?(?:g\+\+|c\+\+|gcc|cc1plus|cc1|collect2|lto1|lto-wrapper|as|ld|ld\.bfd|ld\.gold|ld\.lld)(?:-[0-9]+(?:\.[0-9]+)*)?(?:\.exe)?$/i;

/** Whether `program` (a path or a file name) names a compiler, assembler or linker. */
export function isCompilerProgram(program: string): boolean {
  const name = program.split(/[\\/]/).pop() ?? '';
  return COMPILER_PROGRAM.test(name);
}

/** A compiler process seen while watching. */
export interface Sighting {
  readonly pid: number;
  /** The program, as the process list shows it (`argv[0]` on Linux, the image name on Windows). */
  readonly program: string;
}

/**
 * The processes of `tasklist /FO CSV /NH` output: one `"image","pid",…` line each (lines that do
 * not parse are skipped).
 */
export function parseTasklist(text: string): Sighting[] {
  const found: Sighting[] = [];
  for (const line of text.split(/\r?\n/)) {
    const match = /^"([^"]*)","(\d+)"/.exec(line.trim());
    if (match?.[1] !== undefined && match[2] !== undefined) {
      found.push({ program: match[1], pid: Number(match[2]) });
    }
  }
  return found;
}

/** `argv[0]` and `comm` of a Linux process ('' each when gone or unreadable). */
function linuxProgram(pid: number): { readonly argv0: string; readonly comm: string } {
  const read = (file: string): string => {
    try {
      return readFileSync(path.join('/proc', String(pid), file), 'utf8');
    } catch {
      return '';
    }
  };
  return { argv0: read('cmdline').split('\0')[0] ?? '', comm: read('comm').trim() };
}

/** The compilers below `roots` now (Linux). */
function sampleLinux(roots: readonly number[]): Sighting[] {
  const below = descendantsOf(listProcesses(), roots);
  const found: Sighting[] = [];
  for (const entry of below) {
    const { argv0, comm } = linuxProgram(entry.pid);
    if (isCompilerProgram(argv0) || isCompilerProgram(comm)) {
      found.push({ pid: entry.pid, program: argv0 === '' ? comm : argv0 });
    }
  }
  return found;
}

/** The compilers on the machine now (Windows). */
function sampleWindows(): Promise<Sighting[]> {
  return new Promise((resolve) => {
    execFile(
      'tasklist',
      ['/FO', 'CSV', '/NH'],
      { windowsHide: true, timeout: 10_000, maxBuffer: 16 * 1024 * 1024 },
      (error, stdout) => {
        if (error !== null) {
          process.stderr.write(`tasklist failed: ${error.message}\n`);
          resolve([]);
          return;
        }
        resolve(parseTasklist(stdout).filter((entry) => isCompilerProgram(entry.program)));
      },
    );
  });
}

/** How often the process list is read. */
const INTERVAL_MS = process.platform === 'win32' ? 250 : 100;

/** Watches the process list for compilers; see the module comment. */
export class CompilerWatch {
  readonly #roots: readonly number[];
  readonly #seen = new Map<string, Sighting>();
  #stopped = false;
  #samples = 0;
  readonly #loop: Promise<void>;

  private constructor(roots: readonly number[]) {
    this.#roots = roots;
    this.#loop = this.#run();
  }

  /**
   * Starts watching the processes of the app whose profile is `profile` (the test's
   * `B2C_E2E_ROOT`).
   *
   * @throws Error on a system other than Linux or Windows, or (Linux) when no process of the
   * test has the profile in its environment.
   */
  static start(profile: string): CompilerWatch {
    if (process.platform === 'win32') {
      return new CompilerWatch([]);
    }
    if (process.platform !== 'linux') {
      throw new Error(`The compiler watch supports Linux and Windows, not ${process.platform}`);
    }
    const marker = `B2C_E2E_ROOT=${profile}`;
    const roots = listProcesses()
      .filter((entry) => entry.pid !== process.pid && hasEnvironmentEntry(entry.pid, marker))
      .map((entry) => entry.pid);
    if (roots.length === 0) {
      throw new Error(`No process has ${marker} in its environment: is the app running?`);
    }
    return new CompilerWatch(roots);
  }

  async #run(): Promise<void> {
    while (!this.#stopped) {
      const found = process.platform === 'win32' ? await sampleWindows() : sampleLinux(this.#roots);
      this.#samples += 1;
      for (const sighting of found) {
        this.#seen.set(`${String(sighting.pid)}:${sighting.program}`, sighting);
      }
      await new Promise((resolve) => setTimeout(resolve, INTERVAL_MS));
    }
  }

  /** The compilers seen so far. */
  sightings(): Sighting[] {
    return [...this.#seen.values()];
  }

  /** How many times the process list was read (a test checks that the watch really ran). */
  get samples(): number {
    return this.#samples;
  }

  /** Stops watching and returns every compiler seen. */
  async stop(): Promise<Sighting[]> {
    this.#stopped = true;
    await this.#loop;
    return this.sightings();
  }
}

/** The build cache of a profile (`Dirs::under_root`: `<profile>/cache/builds`). */
export function buildCache(profile: string): string {
  return path.join(profile, 'cache', 'builds');
}

/**
 * The build folders in a profile's cache (`builds/<project>/<config>-<hash8>/`, as relative
 * paths); empty when nothing was ever built.
 */
export function buildFolders(profile: string): string[] {
  const root = buildCache(profile);
  if (!existsSync(root)) {
    return [];
  }
  const found: string[] = [];
  for (const project of readdirSync(root, { withFileTypes: true })) {
    if (!project.isDirectory()) {
      continue;
    }
    const configs = readdirSync(path.join(root, project.name), { withFileTypes: true });
    found.push(project.name);
    for (const config of configs) {
      found.push(path.join(project.name, config.name));
    }
  }
  return found;
}
