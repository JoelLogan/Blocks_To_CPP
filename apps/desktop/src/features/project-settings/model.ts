/**
 * The project settings page's form (docs/spec/04-user-interface.md §4.1, 05 §5.3): a draft made from
 * the project, checked as the person types, and turned back into project settings when it is
 * saved. Pure functions; the page (./ProjectSettingsPage.tsx) holds the draft.
 *
 * Every setting is a closed choice or a checked value (ADR-0005): the C++ standard, the generator
 * options, each configuration's optimisation, debug information, sanitizers, warnings and
 * hardening, defines (an identifier and a typed value; the backend checks them again, B2C-T1019),
 * and the run arguments (an argv list, never a shell string) and working folder.
 */
import type {
  BdmBuildConfiguration,
  BdmDefine,
  BdmProject,
  CppStandard,
} from '@blocks2cpp/b2c-core-wasm';

import type { ProjectSettings } from '../../app/editor-types';

/** The most defines and run arguments the page offers. */
export const MAX_DEFINES = 64;
export const MAX_ARGS = 64;

/** The longest define name (`b2c_ir::text::Ident`) and run argument the page accepts. */
export const MAX_DEFINE_NAME = 64;
export const MAX_ARG_LENGTH = 4096;

/** The longest text value of a define. */
export const MAX_DEFINE_TEXT = 1024;

/** The C++ standards, oldest first. */
export const STANDARDS: readonly CppStandard[] = ['c++17', 'c++20', 'c++23', 'c++26'];

/** The type of a define's value. */
export type DefineType = 'int' | 'bool' | 'string';

/** One define row. `value` is the text typed (for `bool`, `true` or `false`). */
export interface DefineRow {
  readonly key: number;
  readonly name: string;
  readonly type: DefineType;
  readonly value: string;
}

/** One run argument row. */
export interface ArgRow {
  readonly key: number;
  readonly text: string;
}

/** What the page edits. */
export interface Draft {
  readonly standard: CppStandard;
  readonly gnuExtensions: boolean;
  readonly options: BdmProject['options'];
  readonly debug: BdmBuildConfiguration;
  readonly release: BdmBuildConfiguration;
  readonly defines: readonly DefineRow[];
  readonly args: readonly ArgRow[];
  readonly workingDirectory: BdmProject['run']['workingDirectory'];
}

/** The rows' keys, for React; unique within a page. */
let nextKey = 1;

/** A fresh key for a new row. */
export function rowKey(): number {
  return nextKey++;
}

function defineRow(define: BdmDefine): DefineRow {
  const { value } = define;
  if ('int' in value) {
    return { key: rowKey(), name: define.name, type: 'int', value: String(value.int) };
  }
  if ('bool' in value) {
    return { key: rowKey(), name: define.name, type: 'bool', value: String(value.bool) };
  }
  return { key: rowKey(), name: define.name, type: 'string', value: value.string };
}

/** The draft of `project`'s settings. */
export function draftFrom(project: BdmProject): Draft {
  return {
    standard: project.language.standard,
    gnuExtensions: project.language.gnuExtensions === true,
    options: project.options,
    debug: project.build.configurations.debug,
    release: project.build.configurations.release,
    defines: (project.build.defines ?? []).map(defineRow),
    args: (project.run.args ?? []).map((text) => ({ key: rowKey(), text })),
    workingDirectory: project.run.workingDirectory,
  };
}

/** C++ keywords and the standard macros a define may not replace. */
const RESERVED = new Set([
  'alignas',
  'alignof',
  'and',
  'and_eq',
  'asm',
  'auto',
  'bitand',
  'bitor',
  'bool',
  'break',
  'case',
  'catch',
  'char',
  'char8_t',
  'char16_t',
  'char32_t',
  'class',
  'compl',
  'concept',
  'const',
  'consteval',
  'constexpr',
  'constinit',
  'const_cast',
  'continue',
  'co_await',
  'co_return',
  'co_yield',
  'decltype',
  'default',
  'delete',
  'do',
  'double',
  'dynamic_cast',
  'else',
  'enum',
  'explicit',
  'export',
  'extern',
  'false',
  'float',
  'for',
  'friend',
  'goto',
  'if',
  'inline',
  'int',
  'long',
  'mutable',
  'namespace',
  'new',
  'noexcept',
  'not',
  'not_eq',
  'nullptr',
  'operator',
  'or',
  'or_eq',
  'private',
  'protected',
  'public',
  'register',
  'reinterpret_cast',
  'requires',
  'return',
  'short',
  'signed',
  'sizeof',
  'static',
  'static_assert',
  'static_cast',
  'struct',
  'switch',
  'template',
  'this',
  'thread_local',
  'throw',
  'true',
  'try',
  'typedef',
  'typeid',
  'typename',
  'union',
  'unsigned',
  'using',
  'virtual',
  'void',
  'volatile',
  'wchar_t',
  'while',
  'xor',
  'xor_eq',
  'NDEBUG',
  'EOF',
  'NULL',
  'assert',
  'errno',
  'stdin',
  'stdout',
  'stderr',
  'offsetof',
  'main',
]);

/** Why `name` cannot be a define's name, or `null`. */
export function defineNameProblem(name: string): string | null {
  if (name === '') {
    return 'Enter a name.';
  }
  if (name.length > MAX_DEFINE_NAME) {
    return `A name has at most ${String(MAX_DEFINE_NAME)} characters.`;
  }
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(name)) {
    return 'A name has letters, digits and _ only, and does not start with a digit.';
  }
  if (name.startsWith('__') || /^_[A-Z]/.test(name)) {
    return 'Names that start with __, or with _ and a capital letter, are reserved for C++.';
  }
  if (RESERVED.has(name)) {
    return `${name} is a C++ word or standard name.`;
  }
  return null;
}

/** Why `value` cannot be the value of a define of `type`, or `null`. */
export function defineValueProblem(type: DefineType, value: string): string | null {
  switch (type) {
    case 'int': {
      const number = Number(value);
      return /^-?\d+$/.test(value.trim()) && Number.isSafeInteger(number)
        ? null
        : `Enter a whole number from −${String(Number.MAX_SAFE_INTEGER)} to ${String(Number.MAX_SAFE_INTEGER)}.`;
    }
    case 'bool':
      return value === 'true' || value === 'false' ? null : 'Choose true or false.';
    case 'string':
      if (value.length > MAX_DEFINE_TEXT) {
        return `A text has at most ${String(MAX_DEFINE_TEXT)} characters.`;
      }
      // Control characters, and the ones that reorder text, never go into a program by a define.
      // eslint-disable-next-line no-control-regex
      return /[\u0000-\u001f\u007f‪-‮⁦-⁩]/.test(value)
        ? 'A text may not contain control characters.'
        : null;
  }
}

/** Why `text` cannot be a run argument, or `null`. */
export function argProblem(text: string): string | null {
  if (text.length > MAX_ARG_LENGTH) {
    return `An argument has at most ${String(MAX_ARG_LENGTH)} characters.`;
  }
  return text.includes('\u0000') ? 'An argument may not contain a NUL character.' : null;
}

/** Where the draft has a problem: the field's ID (`define-name-<key>` and so on) and the text. */
export interface DraftProblem {
  readonly field: string;
  readonly message: string;
}

/** Every problem of `draft`, in page order. */
export function draftProblems(draft: Draft): DraftProblem[] {
  const problems: DraftProblem[] = [];
  const seen = new Set<string>();
  for (const row of draft.defines) {
    const nameProblem =
      defineNameProblem(row.name) ?? (seen.has(row.name) ? `${row.name} is defined twice.` : null);
    seen.add(row.name);
    if (nameProblem !== null) {
      problems.push({ field: `define-name-${String(row.key)}`, message: nameProblem });
    }
    const valueProblem = defineValueProblem(row.type, row.value);
    if (valueProblem !== null) {
      problems.push({ field: `define-value-${String(row.key)}`, message: valueProblem });
    }
  }
  for (const row of draft.args) {
    const problem = argProblem(row.text);
    if (problem !== null) {
      problems.push({ field: `arg-${String(row.key)}`, message: problem });
    }
  }
  return problems;
}

function defineOf(row: DefineRow): BdmDefine {
  switch (row.type) {
    case 'int':
      return { name: row.name, value: { int: Number(row.value.trim()) } };
    case 'bool':
      return { name: row.name, value: { bool: row.value === 'true' } };
    case 'string':
      return { name: row.name, value: { string: row.value } };
  }
}

/**
 * The project settings of `draft` (which has no problems), keeping what the page does not edit
 * (libraries, packs) from `project`. Optional keys are written as the file writes them: only when
 * they say something (05 §5.3).
 */
export function settingsFrom(draft: Draft, project: BdmProject): ProjectSettings {
  const language: BdmProject['language'] = { standard: draft.standard };
  if (draft.gnuExtensions) {
    language.gnuExtensions = true;
  }
  const build: BdmProject['build'] = {
    ...project.build,
    configurations: { debug: draft.debug, release: draft.release },
  };
  if (draft.defines.length > 0) {
    build.defines = draft.defines.map(defineOf);
  } else {
    delete build.defines;
  }
  const run: BdmProject['run'] = { ...project.run, workingDirectory: draft.workingDirectory };
  if (draft.args.length > 0) {
    run.args = draft.args.map((row) => row.text);
  } else {
    delete run.args;
  }
  return { language, options: draft.options, build, run };
}

/** `value` as JSON with the keys of every object sorted, for comparing settings. */
export function canonicalJson(value: unknown): string {
  return JSON.stringify(value, (_key, inner: unknown) =>
    inner !== null && typeof inner === 'object' && !Array.isArray(inner)
      ? Object.fromEntries(
          Object.entries(inner as Record<string, unknown>).sort(([a], [b]) => a.localeCompare(b)),
        )
      : inner,
  );
}

/** Whether `settings` differ from `project`'s own. */
export function settingsChanged(settings: ProjectSettings, project: BdmProject): boolean {
  const current: ProjectSettings = {
    language: project.language,
    options: project.options,
    build: project.build,
    run: project.run,
  };
  return canonicalJson(current) !== canonicalJson(settings);
}
