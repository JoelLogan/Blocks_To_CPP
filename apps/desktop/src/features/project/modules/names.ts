/**
 * Checking a module name in the add and rename dialogs (docs/plans/m3-contracts/w3-modules.md
 * §2.10). A module's name becomes its file names, so the rules are the compiler core's: the
 * dialogs ask the core's loader to read the project with the candidate name (`canonical()`), which
 * checks the file-name pattern, the Windows device names and names equal ignoring letter case
 * (`B2C-E0117`, `B2C-E0118`, `B2C-E0119`), and give its message. No copy of those rules is kept
 * here, so none can drift from the loader's. The `b2c` prefix, which the loader accepts and the
 * analyser refuses (`B2C-E0221`), is refused here too, and a name whose only problem is capital
 * letters gets its lower-case form suggested.
 */
import type { BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';

import { addModule, renameModule } from './operations';

/** What {@link checkModuleName} decided: the name to use, or why it cannot be used. */
export type ModuleNameCheck =
  { readonly ok: true; readonly name: string } | { readonly ok: false; readonly message: string };

/** The loader's codes about a module's name (docs/reference/diagnostics/loader-and-catalog.md). */
const NAME_CODES: ReadonlySet<string> = new Set([
  'B2C-E0117',
  'B2C-E0118',
  'B2C-E0119',
  'B2C-E0123',
  'B2C-E0124',
  'B2C-E0125',
  'B2C-E0126',
]);

/** The message of `B2C-E0221`, which the dialogs give before the analyser would. */
export const B2C_PREFIX_MESSAGE =
  'Module names that start with “b2c” are kept for files that Blocks2Cpp writes. Choose another name.';

/** Whether `name` starts with `b2c` in any letter case. */
function hasB2cPrefix(name: string): boolean {
  return name.slice(0, 3).toLowerCase() === 'b2c';
}

/** The ID {@link checkModuleName} gives a module being added while it checks its name. */
const CANDIDATE_ID = 'mod_b2cNameCheck00000';

/** `doc` with the candidate name: module `moduleId` renamed, or a new module added. */
function candidate(doc: BdmDocument, name: string, moduleId: string | null): BdmDocument {
  return moduleId === null ? addModule(doc, CANDIDATE_ID, name) : renameModule(doc, moduleId, name);
}

/** The loader's problems with `doc`, or `null` when it loads. */
function problems(
  core: CoreWasm,
  doc: BdmDocument,
): readonly { code: string; message: string }[] | null {
  const result = core.canonical(JSON.stringify(doc));
  return result.ok ? null : result.diagnostics;
}

/**
 * Checks `text` as the name of module `moduleId` of `doc` (a rename) or, with `null`, of a module
 * added to it. `core` is `null` before the core has started; the name cannot be checked then.
 * Surrounding spaces are taken off. A rename to the module's own name is accepted.
 */
export function checkModuleName(
  text: string,
  doc: BdmDocument,
  moduleId: string | null,
  core: CoreWasm | null,
): ModuleNameCheck {
  const name = text.trim();
  if (name === '') {
    return { ok: false, message: 'Type a name for the module.' };
  }
  if (moduleId !== null && doc.modules.find((module) => module.id === moduleId)?.name === name) {
    return { ok: true, name };
  }
  if (hasB2cPrefix(name)) {
    return { ok: false, message: B2C_PREFIX_MESSAGE };
  }
  if (core === null) {
    return {
      ok: false,
      message: 'The name cannot be checked until Blocks2Cpp has finished starting. Try again.',
    };
  }
  try {
    const found = problems(core, candidate(doc, name, moduleId));
    if (found === null) {
      return { ok: true, name };
    }
    const lower = name.toLowerCase();
    if (lower !== name && problems(core, candidate(doc, lower, moduleId)) === null) {
      return {
        ok: false,
        message: `Module names use lower-case letters, because they become file names. Use “${lower}” instead.`,
      };
    }
    const about = found.find((problem) => NAME_CODES.has(problem.code)) ?? found[0];
    return {
      ok: false,
      message:
        about === undefined ? 'This name cannot be used.' : `${about.message} (${about.code})`,
    };
  } catch (error: unknown) {
    console.error('The compiler core could not check a module name', error);
    return { ok: false, message: 'The name could not be checked. Try again.' };
  }
}
