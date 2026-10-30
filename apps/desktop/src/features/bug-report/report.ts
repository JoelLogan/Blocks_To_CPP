/**
 * *Copy bug report* (docs/spec/07-toolchain-build-run.md §7.5.3 step 4, 08 §8.11;
 * docs/plans/m3-contracts/w4-editor-tools.md §2.7): the text of a report on a diagnostic that looks
 * like a bug in Blocks2Cpp, for the user to read before it is copied. Nothing is sent anywhere.
 * Pure.
 *
 * Always in it: the app's and the catalog's versions, the platform, the toolchain's flavour,
 * version, target and capabilities (never its path), the project's C++ standard and build
 * configuration, and the diagnostic's code and source, with g++'s message in which every quoted
 * name, every path and every file name is replaced and the lines that quote source code are left
 * out.
 *
 * With the project's text (the user ticks it): also g++'s whole message (paths still replaced),
 * the blocks of the diagnostic's block (its top block, as the project file stores it) and the
 * generated C++ of its module. Nothing else of the project: no other module, no name, no path.
 */
import type { BdmBlock, BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import type { AppInfo, Diagnostic, GeneratedFile, Toolchain } from '@blocks2cpp/ipc-types';

/** What a report is made from. */
export interface BugReportInput {
  readonly appInfo: AppInfo | null;
  readonly toolchain: Toolchain | null;
  /** The project's C++ standard, and the name of the build configuration in use. */
  readonly standard: string | null;
  readonly configuration: string | null;
  readonly diagnostic: Diagnostic;
  /** The project, for the diagnostic's blocks (only with the project's text). */
  readonly document: BdmDocument | null;
  /** The generated files (only with the project's text). */
  readonly files: readonly GeneratedFile[];
}

/** What a report holds. */
export interface BugReportOptions {
  /** The blocks, the generated C++ and g++'s whole message (the user's tick). */
  readonly includeProject: boolean;
}

/** The longest generated file a report holds, in characters. */
export const MAX_REPORT_CPP_CHARS = 64 * 1024;
/** The longest message a report holds, in characters. */
export const MAX_REPORT_MESSAGE_CHARS = 16 * 1024;

/**
 * Replaces paths: absolute ones and `~`-ones, Windows and POSIX, by `<path>`, and the folders of a
 * place g++ names at the start of a line (`/home/a b/x/main.cpp:7:5`, spaces included) by
 * `<path>/`. A path elsewhere in a line runs on past a space when the next word reaches another
 * folder separator (`/home/jo ann/.cache/x.out`, `C:\Users\Alice Smith\AppData\…`), so a
 * folder name with a space is never cut in two; `a/b and /c` stays two paths.
 */
export function withoutPaths(text: string): string {
  return text
    .split('\n')
    .map((line) =>
      line.replace(
        /^((?:In file included from |\s+from )?)(?:[A-Za-z]:)?[\\/~].*[\\/](?=[^\\/\s]+:(?:\d|\s))/u,
        '$1<path>/',
      ),
    )
    .join('\n')
    .replace(/[A-Za-z]:[\\/](?:[^\s:'"‘’`]| (?=[^\s:'"‘’`\\/]*[\\/]))*/gu, '<path>')
    .replace(/\\\\[^\s:'"‘’`]+/gu, '<path>')
    .replace(
      /(^|[\s(`'"‘])(?:~|\/)(?:[^\s:'"‘’`)]| (?=[^\s:'"‘’`)/]*\/))*\/[^\s:'"‘’`)]*/gu,
      '$1<path>',
    );
}

/** Replaces every name g++ quotes (‘score’, 'score', `f(int)') by `‘…’`. */
export function withoutQuotedNames(text: string): string {
  return text.replace(/[‘'`][^’'`\n]*[’']/gu, '‘…’');
}

/** Whether a line of g++'s output quotes source code (`   12 | int x;` and the caret line). */
function isSourceLine(line: string): boolean {
  return /^\s*\d*\s*\|/u.test(line);
}

/**
 * Replaces source and object file names (`Game.cpp:7:5`, `main.o`) by `<file>`: the generated
 * files are named after the project's modules.
 */
export function withoutFileNames(text: string): string {
  return text.replace(/[^\s:'"‘’`()<>]+\.(?:cpp|hpp|h|o|obj|exe)\b/gu, '<file>');
}

/** g++'s message as a report holds it without the project's text. */
export function sanitisedMessage(raw: string): string {
  return raw
    .split('\n')
    .filter((line) => !isSourceLine(line))
    .map((line) => withoutQuotedNames(withoutFileNames(withoutPaths(line))))
    .join('\n');
}

/** Cuts text to `max` characters, saying so. */
function capped(text: string, max: number): string {
  return text.length <= max ? text : `${text.slice(0, max)}\n… (cut)`;
}

/** The top block of the document that holds block `id`, or `null`. */
export function topBlockOf(document: BdmDocument, id: string): BdmBlock | null {
  for (const module of document.modules) {
    for (const top of module.workspace.blocks) {
      const pending: BdmBlock[] = [top];
      let visited = 0;
      for (let block = pending.pop(); block !== undefined; block = pending.pop()) {
        if (block.id === id) {
          return top;
        }
        if (++visited > 100_000) {
          break;
        }
        for (const input of Object.values(block.inputs ?? {})) {
          if ('block' in input) {
            pending.push(input.block);
          }
        }
        for (const list of Object.values(block.statements ?? {})) {
          pending.push(...list);
        }
        pending.push(...(block.stack ?? []));
      }
    }
  }
  return null;
}

/** The text of a report (see the module comment). */
export function bugReport(input: BugReportInput, options: BugReportOptions): string {
  const { appInfo, toolchain, diagnostic } = input;
  const lines: string[] = [
    'Blocks2Cpp bug report',
    '',
    `App: ${appInfo?.appVersion ?? 'unknown'} (catalog ${appInfo?.catalogVersion ?? 'unknown'}, ${appInfo?.platform ?? 'unknown system'})`,
    `Compiler: ${
      toolchain === null
        ? 'unknown'
        : [
            toolchain.flavor ?? 'g++',
            toolchain.version ?? 'unknown version',
            toolchain.target ?? '',
          ]
            .filter((part) => part !== '')
            .join(' ')
    }`,
  ];
  if (toolchain !== null) {
    const { capabilities } = toolchain;
    lines.push(
      `Compiler can: ${capabilities.standards.join(', ')}; std::format ${capabilities.stdFormat ? 'yes' : 'no'}`,
    );
  }
  lines.push(
    `C++ standard: ${input.standard ?? 'unknown'}; configuration: ${input.configuration ?? 'unknown'}`,
    `Diagnostic: ${diagnostic.code} (${diagnostic.source}, ${diagnostic.severity})`,
    '',
  );
  const raw = diagnostic.raw ?? '';
  if (options.includeProject) {
    lines.push('g++ said:', capped(withoutPaths(raw), MAX_REPORT_MESSAGE_CHARS), '');
    const block = diagnostic.primary.block;
    const top =
      block === undefined || input.document === null ? null : topBlockOf(input.document, block);
    if (top !== null) {
      lines.push('Blocks:', capped(JSON.stringify(top, null, 2), MAX_REPORT_CPP_CHARS), '');
    }
    const module = diagnostic.primary.module;
    const file =
      input.files.find((candidate) => module !== undefined && candidate.module === module) ??
      (input.files.length === 1 ? input.files[0] : undefined);
    if (file !== undefined) {
      lines.push(
        `Generated C++ (${file.path}):`,
        capped(withoutPaths(file.contents), MAX_REPORT_CPP_CHARS),
        '',
      );
    }
  } else {
    lines.push(
      'g++ said (names and paths left out):',
      capped(sanitisedMessage(raw), MAX_REPORT_MESSAGE_CHARS),
      '',
      'The blocks and the generated C++ are not included.',
    );
  }
  return lines.join('\n').trimEnd() + '\n';
}
