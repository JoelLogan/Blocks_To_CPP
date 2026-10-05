/**
 * What the Build output tab shows (docs/spec/04-user-interface.md §4.1, 07 §7.5.3): a line when a
 * build starts, its progress, the toolchain's notes (such as B2C-T1011 *sanitizers dropped*,
 * T1012 *hardening dropped* and T1013 *static linking dropped*), the compiler's and the linker's
 * own text, and how the build ended.
 *
 * It also labels generator bugs (07 §7.5.3 step 4): C++ generated from blocks without errors
 * should always compile, so a `C:` error from anything but a Raw C++ block is marked *This looks
 * like a bug in Blocks2Cpp*. (Copy bug report comes in M3.)
 *
 * Everything here is a pure function of its arguments.
 */
import type { BdmBlock, BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import type { BuildConfig, BuildOutcome, BuildStage, Diagnostic } from '@blocks2cpp/ipc-types';

import type { BuildOutputLine } from '../../app/store';

/** The label of a diagnostic that points at a bug in the generator (07 §7.5.3). */
export const GENERATOR_BUG_LABEL = 'This looks like a bug in Blocks2Cpp';

/** The note the Build output shows once per build that hit a generator bug. */
export const GENERATOR_BUG_NOTE: BuildOutputLine = {
  kind: 'note',
  text: `${GENERATOR_BUG_LABEL}: g++ could not compile the C++ made from blocks without errors. Please report it with the project file.`,
};

/** The most lines of one compiler message shown; the rest is summarised in one line. */
export const MAX_RAW_LINES_PER_DIAGNOSTIC = 200;

/** The longest project name shown in the start line, in UTF-16 code units. */
export const MAX_NAME_CHARS = 120;

/** The most blocks {@link rawBlockIds} visits: more than any project the loader accepts. */
const MAX_VISITED_BLOCKS = 1_000_000;

const CONFIG_LABELS: Readonly<Record<BuildConfig, string>> = { debug: 'Debug', release: 'Release' };

const STAGE_LABELS: Readonly<Record<BuildStage, string>> = {
  generate: 'Generating C++',
  compile: 'Compiling',
  link: 'Linking',
};

/** `0.4 s`, `12.3 s`, `2 min 5 s`. */
export function formatDuration(ms: number): string {
  const safe = Number.isFinite(ms) && ms > 0 ? ms : 0;
  if (safe < 60_000) {
    return `${(Math.round(safe / 100) / 10).toFixed(1)} s`;
  }
  const seconds = Math.round(safe / 1000);
  return `${String(Math.floor(seconds / 60))} min ${String(seconds % 60)} s`;
}

/** The first line of a build: `Building Guessing Game (Debug)…`. */
export function buildStartLine(projectName: string, config: BuildConfig): BuildOutputLine {
  const name =
    projectName.length > MAX_NAME_CHARS ? `${projectName.slice(0, MAX_NAME_CHARS)}…` : projectName;
  return { kind: 'progress', text: `Building ${name} (${CONFIG_LABELS[config]})…` };
}

/** A progress line: `Compiling (1/1)`. */
export function progressLine(stage: BuildStage, done: number, total: number): BuildOutputLine {
  return { kind: 'progress', text: `${STAGE_LABELS[stage]} (${String(done)}/${String(total)})` };
}

/** `1 error`, `2 errors`. */
function errors(count: number): string {
  return count === 1 ? '1 error' : `${String(count)} errors`;
}

/** The last line of a build: how it ended and how long it took. */
export function finishedLine(
  outcome: BuildOutcome,
  elapsedMs: number,
  errorCount: number,
): BuildOutputLine {
  const took = formatDuration(elapsedMs);
  switch (outcome) {
    case 'built':
      return { kind: 'progress', text: `Built in ${took}.` };
    case 'upToDate':
      return { kind: 'progress', text: 'Up to date: nothing changed since the last build.' };
    case 'projectErrors':
      return { kind: 'progress', text: 'Not built: the blocks have errors (see Problems).' };
    case 'toolchainProblem':
      return {
        kind: 'progress',
        text: 'Not built: there is a problem with the C++ compiler (see Problems).',
      };
    case 'cancelled':
      return { kind: 'progress', text: 'Build stopped.' };
    case 'failed':
      return {
        kind: 'progress',
        text:
          errorCount > 0
            ? `Build failed after ${took}: ${errors(errorCount)} (see Problems).`
            : `Build failed after ${took}.`,
      };
  }
}

/** The line of a build that could not be started (the message comes from ./messages.ts). */
export function notStartedLine(message: string): BuildOutputLine {
  return { kind: 'progress', text: `Not built: ${message}` };
}

/** The lines of a compiler or linker message: its own text, or its code and message. */
function rawLines(diagnostic: Diagnostic): BuildOutputLine[] {
  const text = diagnostic.raw ?? `${diagnostic.code}: ${diagnostic.message}`;
  const lines = text.split(/\r\n|\r|\n/);
  while (lines.length > 0 && lines.at(-1) === '') {
    lines.pop();
  }
  const shown = lines
    .slice(0, MAX_RAW_LINES_PER_DIAGNOSTIC)
    .map((line): BuildOutputLine => ({ kind: 'raw', text: line }));
  const hidden = lines.length - shown.length;
  if (hidden > 0) {
    shown.push({
      kind: 'raw',
      text: `… ${hidden.toLocaleString('en-US')} more ${hidden === 1 ? 'line' : 'lines'}`,
    });
  }
  return shown;
}

/**
 * The lines of a build's diagnostics: the compiler's and the linker's own text, and a note with
 * the code and message for everything else (the toolchain's notes, and the blocks' own problems
 * when the backend found any).
 */
export function diagnosticLines(items: readonly Diagnostic[]): BuildOutputLine[] {
  const lines: BuildOutputLine[] = [];
  for (const diagnostic of items) {
    if (diagnostic.source === 'compiler' || diagnostic.source === 'linker') {
      lines.push(...rawLines(diagnostic));
    } else {
      lines.push({ kind: 'note', text: `${diagnostic.code}: ${diagnostic.message}` });
    }
  }
  return lines;
}

/** The IDs of the document's Raw C++ blocks (`raw.*`), found without recursion. */
export function rawBlockIds(document: BdmDocument): ReadonlySet<string> {
  const found = new Set<string>();
  const pending: BdmBlock[] = [];
  const push = (blocks: readonly BdmBlock[]) => {
    for (const block of blocks) {
      pending.push(block);
    }
  };
  for (const module of document.modules) {
    push(module.workspace.blocks);
  }
  let visited = 0;
  for (let block = pending.pop(); block !== undefined; block = pending.pop()) {
    if (++visited > MAX_VISITED_BLOCKS) {
      break;
    }
    if (block.type.startsWith('raw.')) {
      found.add(block.id);
    }
    for (const input of Object.values(block.inputs ?? {})) {
      if ('block' in input) {
        pending.push(input.block);
      }
    }
    for (const list of Object.values(block.statements ?? {})) {
      push(list);
    }
    push(block.stack ?? []);
  }
  return found;
}

/** Whether a diagnostic is a `C:` error of g++ or the linker. */
function isCompilerError(diagnostic: Diagnostic): boolean {
  return (
    diagnostic.severity === 'error' &&
    (diagnostic.source === 'compiler' || diagnostic.source === 'linker') &&
    diagnostic.code.startsWith('C:')
  );
}

/** What {@link labelGeneratorBugs} found. */
export interface LabelledDiagnostics {
  /** The diagnostics, with generator bugs labelled. */
  readonly items: readonly Diagnostic[];
  /** Whether any of them is a generator bug. */
  readonly generatorBug: boolean;
}

/**
 * Labels generator bugs: every `C:` error that does not point at a Raw C++ block gets
 * {@link GENERATOR_BUG_LABEL} in front of its message, unless the backend's message says so
 * already. Other diagnostics are returned unchanged.
 */
export function labelGeneratorBugs(
  items: readonly Diagnostic[],
  document: BdmDocument | null,
): LabelledDiagnostics {
  if (!items.some(isCompilerError)) {
    return { items, generatorBug: false };
  }
  const raw = document === null ? new Set<string>() : rawBlockIds(document);
  let generatorBug = false;
  const labelled = items.map((diagnostic) => {
    const block = diagnostic.primary.block;
    if (!isCompilerError(diagnostic) || (block !== undefined && raw.has(block))) {
      return diagnostic;
    }
    generatorBug = true;
    return diagnostic.message.includes(GENERATOR_BUG_LABEL)
      ? diagnostic
      : { ...diagnostic, message: `${GENERATOR_BUG_LABEL}. ${diagnostic.message}` };
  });
  return { items: labelled, generatorBug };
}
