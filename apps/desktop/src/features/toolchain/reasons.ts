/**
 * Why a compiler was rejected or needs attention, in a few plain words per toolchain code
 * (docs/reference/diagnostics/toolchain.md, docs/spec/07-toolchain-build-run.md §7.2–§7.3). The
 * backend's diagnostic message says what exactly happened; the summary says what kind of problem
 * it is and what to do.
 */
import type { Diagnostic, Toolchain } from '@blocks2cpp/ipc-types';

import { visibleInvisibles } from '../../panels/shared/invisibles';

/** A short summary of a toolchain problem and what to do about it. */
export interface ReasonSummary {
  /** What is wrong, for example *GCC is too old*. */
  title: string;
  /** What to do, or `null` when there is nothing to do. */
  fix: string | null;
}

/** The summaries of the codes the toolchain list and the setup page explain (04 §4.6). */
export const REASONS: Readonly<Record<string, ReasonSummary>> = {
  'B2C-T1002': {
    title: 'This program cannot be used as the compiler',
    fix: 'Choose the g++ program itself (g++.exe on Windows), from a folder outside your projects.',
  },
  'B2C-T1003': {
    title: 'The compiler did not run',
    fix: 'Check that it works on its own (g++ --version in a terminal); security software sometimes blocks new programs.',
  },
  'B2C-T1004': {
    title: 'GCC is too old',
    fix: 'Install g++ 11 or newer (13 or newer is recommended).',
  },
  'B2C-T1005': {
    title: 'Clang is not supported yet',
    fix: "Install GCC's g++ and choose it.",
  },
  'B2C-T1006': {
    title: 'The installation is broken',
    fix: 'Reinstall the compiler. On Linux, make sure the g++ package (not only gcc) is installed.',
  },
  'B2C-T1007': {
    title: 'Cygwin compiler: programs need cygwin1.dll',
    fix: "Use MSYS2's UCRT64 g++ (mingw-w64-ucrt-x86_64-gcc), which builds normal Windows programs.",
  },
  'B2C-T1008': {
    title: 'The old MinGW from mingw.org',
    fix: 'Install MSYS2 (UCRT64) or WinLibs.',
  },
  'B2C-T1014': {
    title: "This program does not look like GCC's g++",
    fix: "Choose GCC's g++ program.",
  },
  'B2C-T1020': {
    title: 'The compiler is on a network folder',
    fix: 'Install the compiler on a local disk: builds are slower, and anyone who can change that folder can change what runs on this computer.',
  },
};

/** The most problems listed for one toolchain; the rest are counted. */
export const MAX_LISTED_PROBLEMS = 10;

/** The longest diagnostic message shown, in UTF-16 code units; longer ones are cut. */
export const MAX_MESSAGE_LENGTH = 600;

/** The summary for `code`, or `null` for a code without one. */
export function reasonFor(code: string): ReasonSummary | null {
  return Object.hasOwn(REASONS, code) ? (REASONS[code] ?? null) : null;
}

/**
 * Text from the backend made safe to show as plain text: hidden and reordering characters become
 * visible placeholders (such as ⟨U+202E⟩), and it is cut at `max` code units.
 */
export function displayText(text: string, max: number = MAX_MESSAGE_LENGTH): string {
  const visible = visibleInvisibles(text);
  return visible.length <= max ? visible : `${visible.slice(0, max - 1)}…`;
}

/** A problem of a toolchain, ready to show. */
export interface ProblemView {
  code: string;
  severity: Diagnostic['severity'];
  /** What kind of problem it is (a generic title for codes without a summary). */
  title: string;
  /** What to do, or `null`. */
  fix: string | null;
  message: string;
}

/** The title of a problem whose code has no summary. */
export const GENERIC_PROBLEM_TITLE = 'A problem was found';

/** The problems of `diagnostics` to list (at most {@link MAX_LISTED_PROBLEMS}) and how many more. */
export function problemViews(diagnostics: readonly Diagnostic[]): {
  shown: ProblemView[];
  more: number;
} {
  const shown = diagnostics.slice(0, MAX_LISTED_PROBLEMS).map((diagnostic): ProblemView => {
    const summary = reasonFor(diagnostic.code);
    return {
      code: displayText(diagnostic.code, 32),
      severity: diagnostic.severity,
      title: summary?.title ?? GENERIC_PROBLEM_TITLE,
      fix: summary?.fix ?? null,
      message: displayText(diagnostic.message),
    };
  });
  return { shown, more: Math.max(0, diagnostics.length - shown.length) };
}

/** How the page names a toolchain: `g++ 15.2.0 (MSYS2 UCRT64)`, or `g++` without a version. */
export function toolchainName(toolchain: Toolchain): string {
  const name = toolchain.version === null ? 'g++' : `g++ ${displayText(toolchain.version, 64)}`;
  return toolchain.flavor === null ? name : `${name} (${displayText(toolchain.flavor, 64)})`;
}
