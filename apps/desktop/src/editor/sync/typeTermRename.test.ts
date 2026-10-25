/**
 * The user types a type term names follow an ID remapping (types.md §2.4), in project trees
 * (`renameInTrees`, used for placeholders and pasted trees) and in live blocks (`renameInBlock`,
 * used for duplicates), as the core's `terms::rename` renames them on paste.
 */
import type { BdmBlock } from '@blocks2cpp/b2c-core-wasm';
import { afterEach, describe, expect, it } from 'vitest';

import { renamedTypeJson, renameInTrees } from './bdmTree';
import { renameInBlock } from './duplicates';
import { disposeWorkspaces, headlessWorkspace } from './testing';

afterEach(() => {
  disposeWorkspaces();
});

const SYMS = new Map([['sym_old', 'sym_new']]);

describe('type terms in a project tree', () => {
  it('rename the user types in a type field and in a parameter row', () => {
    const node = {
      id: 'b1',
      type: 'func.define',
      v: 1,
      fields: {
        NAME: { sym: 'sym_f', name: 'f' },
        RETURNS: 'void',
      },
      extra: {
        params: [
          { sym: 's_a', name: 'a', type: { 'std::set': { ref: 'sym_old' } }, mode: 'copy' },
          { sym: 's_b', name: 'b', type: { ref: 'sym_old' }, mode: 'copy' },
          { sym: 's_c', name: 'c', type: 'int', mode: 'copy' },
        ],
      },
    } as unknown as BdmBlock;
    const declare = {
      id: 'b2',
      type: 'var.declare',
      v: 1,
      fields: {
        TYPE: { 'std::vector': { ref: 'sym_old' } },
        NAME: { sym: 's_v', name: 'v' },
      },
    } as unknown as BdmBlock;
    renameInTrees([node, declare], new Map(), SYMS);
    expect(declare.fields?.['TYPE']).toEqual({ 'std::vector': { ref: 'sym_new' } });
    expect((node.extra as { params: { type: unknown }[] }).params.map((row) => row.type)).toEqual([
      { 'std::set': { ref: 'sym_new' } },
      { ref: 'sym_new' },
      'int',
    ]);
  });

  it('leave terms without those types, and what is no term, as they are', () => {
    expect(renamedTypeJson({ 'std::vector': 'int' }, SYMS)).toBeNull();
    expect(renamedTypeJson({ 'std::map': ['int', { ref: 'sym_other' }] }, SYMS)).toBeNull();
    expect(renamedTypeJson({ sym: 'sym_old', name: 'x' }, SYMS)).toBeNull();
    expect(renamedTypeJson('int', SYMS)).toBeNull();
    expect(renamedTypeJson({ 'std::map': ['int', { ref: 'sym_old' }] }, SYMS)).toEqual({
      'std::map': ['int', { ref: 'sym_new' }],
    });
  });
});

describe('type terms in a live block', () => {
  it('rename the user types a type field holds by its key, and a parameter row’s', () => {
    const workspace = headlessWorkspace();
    const declare = workspace.newBlock('var.declare');
    declare.setFieldValue('{"std::vector":{"ref":"sym_old"}}', 'TYPE');
    renameInBlock(declare, SYMS, () => new Map());
    expect(declare.getFieldValue('TYPE')).toBe('{"std::vector":{"ref":"sym_new"}}');

    const define = workspace.newBlock('func.define') as ReturnType<typeof workspace.newBlock> & {
      b2cSetExtra(extra: unknown): void;
      b2cGetExtra(): { params: { type: unknown }[] };
    };
    define.b2cSetExtra({
      params: [{ sym: 's_a', name: 'a', type: { 'std::set': { ref: 'sym_old' } }, mode: 'copy' }],
    });
    renameInBlock(define, SYMS, () => new Map());
    expect(define.b2cGetExtra().params[0]?.type).toEqual({ 'std::set': { ref: 'sym_new' } });
  });
});
