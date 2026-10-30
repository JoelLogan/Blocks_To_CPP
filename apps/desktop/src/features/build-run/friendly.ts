/**
 * Friendly g++ messages (docs/spec/07-toolchain-build-run.md §7.5.3 step 3,
 * docs/plans/m3-contracts/w4-editor-tools.md §2.6): the common g++ and linker messages that reach
 * a student, in plain words. A rule matches a diagnostic's option (`C:-Wconversion`, with
 * `-Werror=` read as `-W`) or an anchored pattern of g++'s own message, read from the original
 * text (`raw`), which stays as it is: *Show C++ compiler message* shows g++'s words. A message no
 * rule matches is left alone. Pure.
 *
 * Names g++ quotes (`‘score’` in a UTF-8 locale, `'score'` otherwise) are put in as text.
 */
import type { Diagnostic } from '@blocks2cpp/ipc-types';

/** What a rule makes of a message. */
export interface FriendlyMessage {
  /** The plain-language text. */
  readonly text: string;
  /**
   * The problem is not the generated C++'s (a header the project names that is not installed, a
   * program still running), so it is not labelled as a bug in Blocks2Cpp.
   */
  readonly notGeneratorBug: boolean;
}

/** One rule. */
interface Rule {
  /** The options it is for (`-Wshadow`), any of them. */
  readonly options?: readonly string[];
  /** A pattern of g++'s message (the first line, without its place and option). */
  readonly pattern?: RegExp;
  /** The text, from the pattern's match (or `null`) and the quoted names of the message. */
  readonly text: (match: RegExpExecArray | null, quoted: readonly string[]) => string;
  readonly notGeneratorBug?: true;
  /**
   * Only for g++'s own messages about a place in a file (`main.cpp:1:10: …`): not the linker's,
   * nor the driver's messages without a place, which a pattern could read wrongly.
   */
  readonly compilerAtPlace?: true;
}

/** The first quoted name, or a fallback. */
function first(quoted: readonly string[], fallback: string): string {
  return quoted[0] ?? fallback;
}

/** The rules, in the order they are tried: patterns first, then options. */
const RULES: readonly Rule[] = [
  {
    // An `#include` g++ cannot find (`main.cpp:1:10: x.h: No such file or directory`); a header
    // name has no spaces. The linker's `cannot find -lx: No such file or directory` and the
    // driver's `main.cpp: No such file or directory` are not this.
    pattern: /^(?:fatal error: )?([^\s:]+): No such file or directory$/u,
    text: (match) =>
      `The header “${match?.[1] ?? 'header'}” was not found. Check the name in the “use header” block, or install the library that has it.`,
    notGeneratorBug: true,
    compilerAtPlace: true,
  },
  {
    pattern: /cannot open output file .*: (?:Permission denied|Invalid argument)/u,
    text: () =>
      'The program file could not be written. Is the program still running? Stop it, then build again.',
    notGeneratorBug: true,
  },
  {
    pattern: /undefined reference to/u,
    text: (_, quoted) => {
      const name = first(quoted, 'a function').replace(/\(.*$/u, '');
      return `“${name}” is declared but has no body. Check its “define” block.`;
    },
  },
  {
    pattern: /no match for .operator<<./u,
    text: () =>
      'This value cannot be printed as it is. Tick “printable” on its struct, or print its fields.',
  },
  {
    options: ['-Wconversion', '-Wfloat-conversion'],
    text: (_, quoted) =>
      quoted.length >= 2
        ? `A ${quoted[0] ?? ''} value is changed to ${quoted[1] ?? ''} here, and may lose information. Convert it on purpose with a “convert” block.`
        : 'A value may lose information when it is converted here. Convert it on purpose with a “convert” block.',
  },
  {
    options: ['-Wsign-conversion', '-Wsign-compare'],
    text: () =>
      'A whole number that may be negative is mixed here with a size or a count, which is never negative.',
  },
  {
    options: ['-Wshadow'],
    text: (_, quoted) =>
      `The name “${first(quoted, 'this')}” here hides another one with the same name. Give one of them another name.`,
  },
  {
    options: ['-Wunused-variable', '-Wunused-but-set-variable'],
    text: (_, quoted) => `“${first(quoted, 'A variable')}” is created but never used.`,
  },
  {
    options: ['-Wmaybe-uninitialized', '-Wuninitialized'],
    text: (_, quoted) =>
      `“${first(quoted, 'A variable')}” may be used before it has a value. Give it a value when it is created.`,
  },
  {
    options: ['-Wdiv-by-zero'],
    text: () => 'This divides by zero, which has no result.',
  },
  {
    options: ['-Woverflow'],
    text: () => 'A number here is too large for its type, so its value changes.',
  },
  {
    options: ['-Wdouble-promotion'],
    text: () => 'A decimal number is widened to a double here.',
  },
  {
    options: ['-Wimplicit-fallthrough'],
    text: () =>
      'One case runs on into the next one. Add a “go on into the next case” block if that is meant.',
  },
  {
    options: ['-Wnull-dereference'],
    text: () => 'This may use a null pointer, which crashes the program.',
  },
];

/** `-Werror=conversion` and `-Wconversion` alike: the warning's option. */
export function warningOption(code: string): string | null {
  if (!code.startsWith('C:-W')) {
    return null;
  }
  const option = code.slice('C:'.length);
  return option.startsWith('-Werror=') ? `-W${option.slice('-Werror='.length)}` : option;
}

/**
 * g++'s own message: the first line of the original text that is not an include chain or a
 * function heading, without its place, its severity and its option.
 */
export function compilerMessageText(raw: string): string {
  return messageLine(raw)
    .replace(/^.*?:\d+(?::\d+)?: /u, '')
    .replace(/^(?:\/[^:]*|[A-Za-z]:\\[^:]*|[^:\s]+\.o): /u, '')
    .replace(/^(?:warning|error): /u, '')
    .replace(/ \[-W[^\]]+\]$/u, '')
    .trim();
}

/** Whether g++'s message line of `raw` names a place in a file (`main.cpp:1:10: …`). */
function atPlace(raw: string): boolean {
  return /^.*?:\d+(?::\d+)?: /u.test(messageLine(raw));
}

/** The first line of `raw` that is not an include chain or a function heading. */
function messageLine(raw: string): string {
  return (
    raw
      .split('\n')
      .find(
        (candidate) =>
          candidate.trim() !== '' &&
          !candidate.startsWith('In file included from') &&
          !/^\s+from /u.test(candidate) &&
          !/: In (?:member )?function /u.test(candidate) &&
          !candidate.startsWith('In function'),
      ) ?? ''
  );
}

/** The names g++ quotes in `text` (‘…’ or '…'), at most four. */
export function quotedNames(text: string): string[] {
  const names: string[] = [];
  const pattern = /[‘'`]([^’'`\n]{1,80})[’']/gu;
  for (let match = pattern.exec(text); match !== null; match = pattern.exec(text)) {
    if (match[1] !== undefined) {
      names.push(match[1]);
    }
    if (names.length >= 4) {
      break;
    }
  }
  return names;
}

/** The friendly text of a g++ or linker diagnostic, or `null` when no rule has one. */
export function friendlyCompilerMessage(diagnostic: Diagnostic): FriendlyMessage | null {
  if (diagnostic.source !== 'compiler' && diagnostic.source !== 'linker') {
    return null;
  }
  const message = compilerMessageText(diagnostic.raw ?? '');
  const quoted = quotedNames(message);
  const option = warningOption(diagnostic.code);
  const compilerAtPlace = diagnostic.source === 'compiler' && atPlace(diagnostic.raw ?? '');
  for (const rule of RULES) {
    if (rule.compilerAtPlace === true && !compilerAtPlace) {
      continue;
    }
    const match = rule.pattern?.exec(message) ?? null;
    const byOption = option !== null && rule.options?.includes(option) === true;
    if (match !== null || byOption) {
      return { text: rule.text(match, quoted), notGeneratorBug: rule.notGeneratorBug === true };
    }
  }
  return null;
}

/** What a g++ warning reads, in front of its friendly text. */
export const COMPILER_WARNS = 'g++ warns: ';

/**
 * A diagnostic with its friendly text: a warning reads “g++ warns: ” and the text; an error the
 * text (labelled a generator bug afterwards when it is one, ./buildOutput.ts). Others unchanged.
 */
export function withFriendlyMessage(diagnostic: Diagnostic): {
  readonly diagnostic: Diagnostic;
  readonly notGeneratorBug: boolean;
} {
  const friendly = friendlyCompilerMessage(diagnostic);
  if (friendly === null) {
    return { diagnostic, notGeneratorBug: false };
  }
  const message =
    diagnostic.severity === 'error' ? friendly.text : `${COMPILER_WARNS}${friendly.text}`;
  return { diagnostic: { ...diagnostic, message }, notGeneratorBug: friendly.notGeneratorBug };
}
