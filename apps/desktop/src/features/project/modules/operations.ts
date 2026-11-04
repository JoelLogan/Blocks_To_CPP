/**
 * The document operations on a project's modules (docs/plans/m3-contracts/w3-modules.md §1.2,
 * §2.9; ADR-0019 §9): add, rename, delete, restore a deleted module, and move one left or right.
 * Each takes a document and gives a new one, changing nothing else: the other modules keep their
 * blocks, every block and symbol keeps its ID, and a rename changes no block (blocks refer to
 * modules by ID, never by name). They refuse a 257th module and the deletion of the only one
 * ({@link ModuleEditError}), as the module strip does.
 *
 * Module names reach file names only through the compiler core's rules (`FileStem`, checked by
 * ./names.ts before a name is used); nothing here builds a path.
 */
import type { BdmBlock, BdmDocument, BdmModule } from '@blocks2cpp/b2c-core-wasm';

/** The most modules a project can have (05 §5.6, the loader's `B2C-E0120`). */
export const MAX_MODULES = 256;

/** Why a module edit cannot be made. */
export type ModuleEditProblem = 'tooMany' | 'onlyModule' | 'unknownModule' | 'alreadyThere';

/** What a project's modules allow: thrown by the operations when an edit cannot be made. */
export class ModuleEditError extends Error {
  override readonly name = 'ModuleEditError';
  readonly problem: ModuleEditProblem;

  constructor(problem: ModuleEditProblem) {
    super(MODULE_EDIT_PROBLEMS[problem]);
    this.problem = problem;
  }
}

/** Why an edit cannot be made, as the strip says it (its disabled items' descriptions). */
export const MODULE_EDIT_PROBLEMS: Readonly<Record<ModuleEditProblem, string>> = {
  tooMany: `A project can have at most ${String(MAX_MODULES)} modules.`,
  onlyModule: 'A project needs at least one module.',
  unknownModule: 'The project has no such module.',
  alreadyThere: 'The project already has this module, or one with the same name.',
};

/** A deleted module, kept so that *Undo delete* can put it back where it was (W3M-D6). */
export interface KeptModule {
  /** The module as it was when it was deleted: its ID, name and blocks. */
  readonly module: BdmModule;
  /** Its place among the modules (0 for the first). */
  readonly index: number;
}

/** The block type of the program's start (*when program starts*). */
const PROGRAM_START = 'program.main';

/** The index of module `id` in `doc`, or a {@link ModuleEditError}. */
function indexOf(doc: BdmDocument, id: string): number {
  const index = doc.modules.findIndex((module) => module.id === id);
  if (index < 0) {
    throw new ModuleEditError('unknownModule');
  }
  return index;
}

/** `doc` with `modules` as its modules. */
function withModules(doc: BdmDocument, modules: BdmModule[]): BdmDocument {
  return { ...doc, modules };
}

/** Whether another module can be added to `doc` (it has fewer than {@link MAX_MODULES}). */
export function canAddModule(doc: BdmDocument): boolean {
  return doc.modules.length < MAX_MODULES;
}

/** Whether a module of `doc` can be deleted (it has more than one). */
export function canDeleteModule(doc: BdmDocument): boolean {
  return doc.modules.length > 1;
}

/**
 * The name a new module gets: the first of `module2`, `module3`, … that no module has, ignoring
 * letter case (05 §5.6).
 */
export function nextModuleName(doc: BdmDocument): string {
  const taken = new Set(doc.modules.map((module) => module.name.toLowerCase()));
  for (let number = 2; ; number += 1) {
    const name = `module${String(number)}`;
    if (!taken.has(name)) {
      return name;
    }
  }
}

/** `doc` with an empty module `{ id, name }` added after the others. */
export function addModule(doc: BdmDocument, id: string, name: string): BdmDocument {
  if (!canAddModule(doc)) {
    throw new ModuleEditError('tooMany');
  }
  return withModules(doc, [...doc.modules, { id, name, workspace: { blocks: [] } }]);
}

/** `doc` with module `id` named `name`; its blocks and everything else stay as they are. */
export function renameModule(doc: BdmDocument, id: string, name: string): BdmDocument {
  const index = indexOf(doc, id);
  return withModules(
    doc,
    doc.modules.map((module, at) => (at === index ? { ...module, name } : module)),
  );
}

/** `doc` without module `id`, and the module kept to restore it ({@link restoreModule}). */
export function deleteModule(
  doc: BdmDocument,
  id: string,
): { readonly doc: BdmDocument; readonly kept: KeptModule } {
  if (!canDeleteModule(doc)) {
    throw new ModuleEditError('onlyModule');
  }
  const index = indexOf(doc, id);
  const module = doc.modules[index];
  if (module === undefined) {
    throw new ModuleEditError('unknownModule');
  }
  return {
    doc: withModules(
      doc,
      doc.modules.filter((_, at) => at !== index),
    ),
    kept: { module, index },
  };
}

/**
 * `doc` with the deleted module back at its place (or last, when fewer modules are left), with its
 * IDs, so that the other modules' references to its definitions bind again. Refused when `doc`
 * already has a module with its ID or its name (ignoring letter case, 05 §5.6): two would not load.
 */
export function restoreModule(doc: BdmDocument, kept: KeptModule): BdmDocument {
  if (!canAddModule(doc)) {
    throw new ModuleEditError('tooMany');
  }
  const name = kept.module.name.toLowerCase();
  if (
    doc.modules.some((module) => module.id === kept.module.id || module.name.toLowerCase() === name)
  ) {
    throw new ModuleEditError('alreadyThere');
  }
  const modules = [...doc.modules];
  modules.splice(Math.min(Math.max(kept.index, 0), modules.length), 0, kept.module);
  return withModules(doc, modules);
}

/**
 * `doc` with module `id` moved one place left (`-1`) or right (`1`); a move past either end
 * changes nothing. The first module names the program (W3M-D9).
 */
export function moveModule(doc: BdmDocument, id: string, delta: -1 | 1): BdmDocument {
  const index = indexOf(doc, id);
  const target = index + delta;
  if (target < 0 || target >= doc.modules.length) {
    return doc;
  }
  const modules = doc.modules.filter((_, at) => at !== index);
  modules.splice(target, 0, ...doc.modules.slice(index, index + 1));
  return withModules(doc, modules);
}

/** How many top-level blocks a module holds (what its delete dialog counts). */
export function topLevelBlockCount(module: BdmModule): number {
  return module.workspace.blocks.length;
}

/** Whether a module holds *when program starts* (a top-level block, never nested). */
export function holdsProgramStart(module: BdmModule): boolean {
  return module.workspace.blocks.some((block: BdmBlock) => block.type === PROGRAM_START);
}
