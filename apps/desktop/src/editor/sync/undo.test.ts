/**
 * Undo in the editor with the real blocks, services and compiler core (03 §3.6, 05 §5.2): opening
 * a project leaves nothing to undo, one Ctrl+Z undoes one edit completely, and undoing a delete
 * brings the block back exactly as the file had it.
 *
 * Real timers: Blockly delivers (and records) its events after an animation frame and a task.
 */
import type { BdmBlock, BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import { B2cSymbolDeclField } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it } from 'vitest';

import { useAppStore } from '../../app/store';
import { documentFixture } from '../../app/testing/fixtures';
import {
  canonicalText,
  disposeWorkspaces,
  EXAMPLE_PROJECTS,
  loadText,
  present,
  renderedWorkspace,
  startSession,
  testCore,
  type TestSession,
} from './testing';

const core = await testCore();

let running: TestSession | null = null;

afterEach(() => {
  running?.dispose();
  running = null;
  disposeWorkspaces();
});

/** Waits until Blockly has delivered (and recorded) the events fired so far. */
function eventsDelivered(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      setTimeout(() => {
        setTimeout(resolve, 0);
      }, 0);
    });
  });
}

/** Lets Blockly deliver its events and the session preview what they changed. */
async function settle(session: TestSession): Promise<void> {
  await eventsDelivered();
  await session.session.flush();
  await eventsDelivered();
}

/** The text of every read-only expression on the canvas, in canvas order. */
function labels(workspace: Blockly.Workspace): string[] {
  return workspace.getAllBlocks(true).flatMap((block) => {
    const field = block.getField('TEXT_BEFORE');
    return field === null ? [] : [String(field.getValue())];
  });
}

/** The declaration field of symbol `sym` on the canvas. */
function declField(workspace: Blockly.Workspace, sym: string): B2cSymbolDeclField {
  for (const block of workspace.getAllBlocks(false)) {
    for (const input of block.inputList) {
      for (const field of input.fieldRow) {
        if (field instanceof B2cSymbolDeclField && field.getSymbolId() === sym) {
          return field;
        }
      }
    }
  }
  throw new Error(`no declaration of ${sym}`);
}

function dirty(): boolean | undefined {
  return useAppStore.getState().project?.dirty;
}

/** The canonical text of the document the canvas was last read into. */
function liveText(): string | undefined {
  return useAppStore.getState().project?.canonicalText;
}

describe.skipIf(core === null)('undo', () => {
  it('has nothing to undo after a project opens; one Ctrl+Z undoes a rename and its labels', async () => {
    const wasm = present(core, 'core');
    const doc = loadText(wasm, present(EXAMPLE_PROJECTS['guessing_game.b2c'], 'guessing game'));
    running = startSession(wasm, doc, { workspace: renderedWorkspace() });
    const { workspace } = running;
    await settle(running);
    const before = ['guess == secret', 'guess < secret', 'guess > secret'];
    expect(labels(workspace)).toEqual(before);
    // Relabelling the read-only expressions when the project opened is not an edit.
    expect(workspace.getUndoStack()).toHaveLength(0);

    declField(workspace, 's_guess').setDecl({ sym: 's_guess', name: 'attempt' });
    await settle(running);
    expect(labels(workspace)).toEqual([
      'attempt == secret',
      'attempt < secret',
      'attempt > secret',
    ]);
    expect(workspace.getUndoStack()).toHaveLength(1);
    expect(dirty()).toBe(true);

    workspace.undo(false);
    await settle(running);
    expect(declField(workspace, 's_guess').getDecl()?.name).toBe('guess');
    expect(labels(workspace)).toEqual(before);
    expect(workspace.getUndoStack()).toHaveLength(0);
    expect(dirty()).toBe(false);
  });

  it.each([
    ['a block that left out its default fields and extra', 'p'],
    ['a block that wrote an empty statement list', 'f'],
  ])('brings back %s exactly when a delete is undone', async (_what, id) => {
    const wasm = present(core, 'core');
    const print: BdmBlock = {
      id: 'p',
      type: 'io.print',
      v: 1,
      inputs: { ITEM0: { expr: [{ str: 'hi' }] } },
    };
    const loop: BdmBlock = { id: 'f', type: 'control.forever', v: 1, statements: { BODY: [] } };
    const sparse: BdmDocument = documentFixture('Sparse');
    sparse.modules = [
      {
        id: 'mod_main',
        name: 'main',
        workspace: {
          blocks: [
            {
              id: 'main',
              type: 'program.main',
              v: 1,
              x: 0,
              y: 0,
              statements: { BODY: [print, loop] },
            },
          ],
        },
      },
    ];
    const doc = loadText(wasm, JSON.stringify(sparse));
    const saved = canonicalText(wasm, doc);
    running = startSession(wasm, doc, { workspace: renderedWorkspace() });
    const { workspace } = running;
    await settle(running);
    expect(dirty()).toBe(false);

    // As the Delete key does: one undoable group, the blocks below move up.
    Blockly.Events.setGroup(true);
    present(workspace.getBlockById(id), id).dispose(true);
    Blockly.Events.setGroup(false);
    await settle(running);
    expect(dirty()).toBe(true);

    workspace.undo(false);
    await settle(running);
    expect(dirty()).toBe(false);
    expect(liveText()).toBe(saved);

    // Redo and undo again: the block keeps its file's shape every time it comes back.
    workspace.undo(true);
    await settle(running);
    workspace.undo(false);
    await settle(running);
    expect(dirty()).toBe(false);
    expect(liveText()).toBe(saved);
  });
});
