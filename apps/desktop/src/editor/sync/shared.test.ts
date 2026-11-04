/**
 * *share with other modules* on *define* (docs/plans/m3-contracts/w3-modules.md §1.2, W3M-D11):
 * written into the project only while it is ticked, so a project that shares nothing stays readable
 * by M2; a value a file holds is read back as it is.
 */
import type { BdmBlock, BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it } from 'vitest';

import { documentFixture } from '../../app/testing/fixtures';
import { loadModule } from './bdmToWorkspace';
import { disposeWorkspaces, headlessWorkspace } from './testing';
import { readModule } from './workspaceToBdm';

afterEach(() => {
  disposeWorkspaces();
});

/** A document whose main module holds `blocks`. */
function docWith(blocks: BdmBlock[]): BdmDocument {
  const doc = documentFixture();
  return { ...doc, modules: [{ id: 'mod_main', name: 'main', workspace: { blocks } }] };
}

/** A *define* named `greet` as a project file holds it, with `fields` added. */
function define(fields: Record<string, unknown> = {}): BdmBlock {
  return {
    id: 'blk_define',
    type: 'func.define',
    v: 1,
    x: 0,
    y: 0,
    fields: { NAME: { sym: 's_greet', name: 'greet' }, RETURNS: 'void', ...fields },
  };
}

/** The fields the canvas's *define* reads back as. */
function readDefine(workspace: Blockly.Workspace, doc: BdmDocument): Record<string, unknown> {
  const read = readModule(workspace, doc, 'mod_main');
  return read.modules[0]?.workspace.blocks[0]?.fields ?? {};
}

describe('SHARED on define, written only when ticked (W3M-D11)', () => {
  it('leaves an unticked new define’s SHARED out, and writes a ticked one', () => {
    const workspace = headlessWorkspace();
    const doc = docWith([]);
    const block = workspace.newBlock('func.define', 'blk_new');
    block.moveBy(10, 10);
    const fresh = readModule(workspace, doc, 'mod_main').modules[0]?.workspace.blocks[0];
    expect(fresh?.fields ?? {}).not.toHaveProperty('SHARED');
    block.setFieldValue('TRUE', 'SHARED');
    const ticked = readModule(workspace, doc, 'mod_main').modules[0]?.workspace.blocks[0];
    expect(ticked?.fields?.['SHARED']).toBe(true);
  });

  it('reads a loaded define back as its file has it', () => {
    for (const fields of [{}, { SHARED: true }, { SHARED: false }]) {
      const workspace = headlessWorkspace();
      const doc = docWith([define(fields)]);
      loadModule(workspace, doc, 'mod_main');
      expect(readDefine(workspace, doc)).toEqual(define(fields).fields);
    }
  });

  it('leaves SHARED out again when a define whose file had none is ticked and unticked', () => {
    const workspace = headlessWorkspace();
    const doc = docWith([define()]);
    loadModule(workspace, doc, 'mod_main');
    const block = workspace.getBlockById('blk_define');
    block?.setFieldValue('TRUE', 'SHARED');
    expect(readDefine(workspace, doc)['SHARED']).toBe(true);
    block?.setFieldValue('FALSE', 'SHARED');
    expect(readDefine(workspace, doc)).not.toHaveProperty('SHARED');
  });
});
