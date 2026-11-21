/**
 * A *namespace* block's mouth in the project file (docs/plans/m3-contracts/w3-modules.md §2.10,
 * §2.12, 05 §5.4): the definitions in it are the mouth's statement list, loaded into the mouth and
 * read back as they were. What the editor cannot show in place (a statement in a mouth, a definition
 * stacked under a top-level definition) becomes a placeholder that keeps its data, as any block
 * that does not fit its place; the catalog reports it (`B2C-E0604`).
 */
import type { BdmBlock, BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import { B2C_CHECKER_NAME, isPlaceholder } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it } from 'vitest';

import { documentFixture } from '../../app/testing/fixtures';
import { loadModule } from './bdmToWorkspace';
import { B2cBlockDragStrategy } from './drag';
import { disposeWorkspaces, renderedWorkspace } from './testing';
import { readModule } from './workspaceToBdm';

afterEach(() => {
  disposeWorkspaces();
});

/** A document whose main module holds `blocks`. */
function docWith(blocks: BdmBlock[]): BdmDocument {
  const doc = documentFixture();
  return { ...doc, modules: [{ id: 'mod_main', name: 'main', workspace: { blocks } }] };
}

function define(id: string, sym: string, name: string): BdmBlock {
  return {
    id,
    type: 'func.define',
    v: 1,
    fields: { NAME: { sym, name }, RETURNS: 'void' },
    extra: { params: [] },
  };
}

function namespace(id: string, sym: string, name: string, definitions: BdmBlock[]): BdmBlock {
  return {
    id,
    type: 'program.namespace',
    v: 1,
    fields: { NAME: { sym, name } },
    statements: { DEFINITIONS: definitions },
  };
}

/** Loads `blocks` into an editor-like canvas and reads the module back. */
function roundTrip(blocks: BdmBlock[]): {
  read: readonly BdmBlock[];
  ids: (id: string) => boolean;
} {
  const workspace = renderedWorkspace({ plugins: { connectionChecker: B2C_CHECKER_NAME } });
  const doc = docWith(blocks);
  loadModule(workspace, doc, 'mod_main');
  const read = readModule(workspace, doc, 'mod_main').modules[0]?.workspace.blocks ?? [];
  return {
    read,
    ids: (id) => {
      const block = workspace.getBlockById(id);
      return block !== null && !isPlaceholder(block);
    },
  };
}

describe('definitions in a namespace’s mouth', () => {
  it('load into the mouth and read back as the file has them', () => {
    const blocks: BdmBlock[] = [
      {
        ...namespace('blk_geo', 's_geo', 'geo', [
          define('blk_area', 's_area', 'area'),
          {
            id: 'blk_box',
            type: 'type.struct',
            v: 1,
            fields: { NAME: { sym: 's_box', name: 'Box' } },
            extra: { fields: [] },
          },
          namespace('blk_inner', 's_inner', 'inner', [define('blk_half', 's_half', 'half')]),
        ]),
        x: 40,
        y: 40,
      },
      {
        id: 'blk_use',
        type: 'program.use_namespace',
        v: 1,
        x: 40,
        y: 400,
        fields: { NS: 's_geo' },
      },
    ];
    const { read, ids } = roundTrip(blocks);
    for (const id of ['blk_geo', 'blk_area', 'blk_box', 'blk_inner', 'blk_half', 'blk_use']) {
      expect(ids(id), id).toBe(true);
    }
    expect(read).toEqual(blocks);
  });

  it('keep what does not fit as placeholders with their data', () => {
    const misplaced: BdmBlock[] = [
      {
        ...namespace('blk_geo', 's_geo', 'geo', [
          {
            id: 'blk_print',
            type: 'io.print',
            v: 1,
            fields: { NEWLINE: true, SEP: 'space', STREAM: 'out' },
            extra: { itemCount: 0 },
          },
        ]),
        x: 40,
        y: 40,
      },
      {
        ...define('blk_top', 's_top', 'top'),
        x: 40,
        y: 400,
        stack: [define('blk_under', 's_under', 'under')],
      },
    ];
    const { read, ids } = roundTrip(misplaced);
    expect(ids('blk_geo')).toBe(true);
    expect(ids('blk_print')).toBe(false);
    expect(ids('blk_top')).toBe(false);
    expect(read).toEqual(misplaced);
  });
});

describe('dragging a definition out of a mouth', () => {
  it('leaves the definitions below it in the mouth, without a modifier key', () => {
    const workspace = renderedWorkspace({ plugins: { connectionChecker: B2C_CHECKER_NAME } });
    const doc = docWith([
      {
        ...namespace('blk_geo', 's_geo', 'geo', [
          define('blk_area', 's_area', 'area'),
          define('blk_size', 's_size', 'size'),
          define('blk_half', 's_half', 'half'),
        ]),
        x: 40,
        y: 40,
      },
    ]);
    loadModule(workspace, doc, 'mod_main');
    const size = workspace.getBlockById('blk_size');
    expect(size).not.toBeNull();
    if (size === null) {
      return;
    }
    const drag = new B2cBlockDragStrategy(size);
    drag.startDrag(new PointerEvent('pointerdown'));
    drag.drag(new Blockly.utils.Coordinate(600, 600), new PointerEvent('pointermove'));
    drag.endDrag(new PointerEvent('pointerup'));
    const read = readModule(workspace, doc, 'mod_main').modules[0]?.workspace.blocks ?? [];
    const geo = read.find((block) => block.id === 'blk_geo');
    expect(geo?.statements?.['DEFINITIONS']?.map((block) => block.id)).toEqual([
      'blk_area',
      'blk_half',
    ]);
    const dragged = read.find((block) => block.id === 'blk_size');
    expect(dragged?.stack).toBeUndefined();
  });
});
