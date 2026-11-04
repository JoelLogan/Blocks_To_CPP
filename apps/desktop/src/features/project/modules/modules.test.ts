/**
 * The document operations on modules and the name check of the add and rename dialogs
 * (docs/plans/m3-contracts/w3-modules.md §1.2, §2.9, §2.10), the names checked by the real
 * compiler core's loader.
 */
import type { BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { describe, expect, it } from 'vitest';

import { documentFixture } from '../../../app/testing/fixtures';
import { testCore } from '../../../editor/sync/testing';
import { B2C_PREFIX_MESSAGE, checkModuleName } from './names';
import {
  addModule,
  canAddModule,
  canDeleteModule,
  deleteModule,
  holdsProgramStart,
  MAX_MODULES,
  MODULE_EDIT_PROBLEMS,
  ModuleEditError,
  moveModule,
  nextModuleName,
  renameModule,
  restoreModule,
  topLevelBlockCount,
} from './operations';

/** *when program starts* in main; a shared greet in utils. */
function twoModules(): BdmDocument {
  const doc = documentFixture('Modules');
  doc.modules = [
    {
      id: 'mod_main',
      name: 'main',
      workspace: {
        blocks: [{ id: 'blk_main', type: 'program.main', v: 1, x: 0, y: 0, inputs: {} }],
        viewport: { x: 1, y: 2, scale: 1 },
      },
    },
    {
      id: 'mod_utils',
      name: 'utils',
      workspace: {
        blocks: [
          {
            id: 'blk_greet',
            type: 'func.define',
            v: 1,
            x: 0,
            y: 0,
            fields: { NAME: { sym: 's_greet', name: 'greet' }, RETURNS: 'void', SHARED: true },
          },
        ],
      },
    },
  ];
  return doc;
}

/** A project with `count` empty modules. */
function manyModules(count: number): BdmDocument {
  const doc = documentFixture();
  doc.modules = Array.from({ length: count }, (_, index) => ({
    id: `mod_${String(index).padStart(17, '0')}`,
    name: index === 0 ? 'main' : `module${String(index + 1)}`,
    workspace: { blocks: [] },
  }));
  return doc;
}

describe('the document operations on modules', () => {
  it('adds an empty module last and names new modules module2, module3, …', () => {
    const doc = twoModules();
    expect(nextModuleName(doc)).toBe('module2');
    const added = addModule(doc, 'mod_new', nextModuleName(doc));
    expect(added.modules.map((module) => module.name)).toEqual(['main', 'utils', 'module2']);
    expect(added.modules[2]?.workspace).toEqual({ blocks: [] });
    expect(nextModuleName(added)).toBe('module3');
    // Letter case does not make a name free.
    expect(nextModuleName(renameModule(added, 'mod_new', 'Module3'))).toBe('module2');
    // The other modules are the same objects: their blocks and IDs are untouched.
    expect(added.modules[0]).toBe(doc.modules[0]);
    expect(added.project).toBe(doc.project);
  });

  it('renames a module and changes no block', () => {
    const doc = twoModules();
    const renamed = renameModule(doc, 'mod_utils', 'tools');
    expect(renamed.modules[1]?.name).toBe('tools');
    expect(renamed.modules[1]?.workspace).toBe(doc.modules[1]?.workspace);
    expect(renamed.modules[0]).toBe(doc.modules[0]);
    expect(() => renameModule(doc, 'mod_gone', 'x')).toThrow(ModuleEditError);
  });

  it('deletes a module, keeping it to restore at its place with its IDs', () => {
    const doc = twoModules();
    const { doc: without, kept } = deleteModule(doc, 'mod_main');
    expect(without.modules.map((module) => module.id)).toEqual(['mod_utils']);
    expect(kept.index).toBe(0);
    expect(kept.module).toBe(doc.modules[0]);
    expect(restoreModule(without, kept)).toEqual(doc);
    // Fewer modules left than its place: it goes last.
    expect(restoreModule(without, { ...kept, index: 9 }).modules.map((m) => m.id)).toEqual([
      'mod_utils',
      'mod_main',
    ]);
  });

  it('refuses a 257th module and deleting the only one, with the strip’s reasons', () => {
    const full = manyModules(MAX_MODULES);
    expect(canAddModule(full)).toBe(false);
    expect(() => addModule(full, 'mod_x', 'x')).toThrow(MODULE_EDIT_PROBLEMS.tooMany);
    expect(MODULE_EDIT_PROBLEMS.tooMany).toBe('A project can have at most 256 modules.');
    expect(canAddModule(manyModules(MAX_MODULES - 1))).toBe(true);
    const one = manyModules(1);
    expect(canDeleteModule(one)).toBe(false);
    expect(() => deleteModule(one, 'mod_00000000000000000')).toThrow(
      'A project needs at least one module.',
    );
    const { doc: without, kept } = deleteModule(twoModules(), 'mod_utils');
    expect(() => restoreModule({ ...full }, kept)).toThrow(ModuleEditError);
    expect(restoreModule(without, kept).modules).toHaveLength(2);
  });

  it('moves a module one place, and nowhere past the ends', () => {
    const doc = twoModules();
    const moved = moveModule(doc, 'mod_utils', -1);
    expect(moved.modules.map((module) => module.id)).toEqual(['mod_utils', 'mod_main']);
    expect(moveModule(moved, 'mod_utils', -1)).toBe(moved);
    expect(moveModule(moved, 'mod_main', 1)).toBe(moved);
    expect(moveModule(moved, 'mod_main', -1)).toEqual(doc);
  });

  it('counts a module’s top-level blocks and tells whether it holds when program starts', () => {
    const doc = twoModules();
    const [main, utils] = doc.modules;
    expect(main === undefined ? -1 : topLevelBlockCount(main)).toBe(1);
    expect(main !== undefined && holdsProgramStart(main)).toBe(true);
    expect(utils !== undefined && holdsProgramStart(utils)).toBe(false);
  });
});

const core: CoreWasm | null = await testCore();

describe.skipIf(core === null)('the module name check (w3-modules.md §2.10)', () => {
  /** The message for `text` as the name of a new module of {@link twoModules}. */
  const added = (text: string): string | null => {
    const result = checkModuleName(text, twoModules(), null, core);
    return result.ok ? null : result.message;
  };
  /** The message for `text` as utils' new name. */
  const renamed = (text: string): string | null => {
    const result = checkModuleName(text, twoModules(), 'mod_utils', core);
    return result.ok ? null : result.message;
  };

  it('accepts names the loader accepts, trimmed', () => {
    expect(checkModuleName(' tools ', twoModules(), null, core)).toEqual({
      ok: true,
      name: 'tools',
    });
    expect(added('a')).toBeNull();
    expect(added('module-2')).toBeNull();
    expect(added('com0x')).toBeNull();
    expect(added('x'.repeat(64))).toBeNull();
    // A rename to the module's own name changes nothing.
    expect(renamed('utils')).toBeNull();
  });

  it('refuses Main, suggesting main, and the other names the core refuses, with its messages', () => {
    for (const check of [added, renamed]) {
      expect(check('Tools')).toBe(
        'Module names use lower-case letters, because they become file names. Use “tools” instead.',
      );
      expect(check('2d')).toMatch(/cannot be used as a file name.*\(B2C-E0117\)$/u);
      expect(check('x'.repeat(65))).toMatch(/\(B2C-E01\d\d\)$/u);
      expect(check('con')).toMatch(/reserved by Windows.*\(B2C-E0118\)$/u);
      expect(check('b2c_x')).toBe(B2C_PREFIX_MESSAGE);
      expect(check('B2C')).toBe(B2C_PREFIX_MESSAGE);
      expect(check('')).toBe('Type a name for the module.');
      expect(check('   ')).toBe('Type a name for the module.');
    }
    // Renaming utils to Main: main is taken (equal ignoring case), so no suggestion helps.
    expect(renamed('main')).toMatch(/\(B2C-E0119\)$/u);
    expect(added('MAIN')).toMatch(/\(B2C-E011[79]\)$/u);
    // A new module named like main, but with a capital: lower case would clash too.
    expect(renamed('Main')).not.toContain('Use “main”');
    // A module named Main elsewhere: suggest main.
    const doc = twoModules();
    const check = checkModuleName('Main', renameModule(doc, 'mod_main', 'start'), null, core);
    expect(check).toEqual({
      ok: false,
      message:
        'Module names use lower-case letters, because they become file names. Use “main” instead.',
    });
  });

  it('cannot check without a running core', () => {
    expect(checkModuleName('tools', twoModules(), null, null)).toEqual({
      ok: false,
      message: 'The name cannot be checked until Blocks2Cpp has finished starting. Try again.',
    });
  });
});
