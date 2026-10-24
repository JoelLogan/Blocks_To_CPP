/**
 * Symbol references with the real compiler core (docs/spec/03-block-language.md §3.6, 06 §6.5):
 * scope-filtered dropdowns, references that keep their symbol when moved out of scope, and renames
 * that relabel every reference.
 */
import {
  B2cSymbolDeclField,
  B2cSymbolRefField,
  getEditorServices,
  newId,
} from '@blocks2cpp/blockly-ext';
import type * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { useAppStore } from '../../app/store';
import {
  disposeWorkspaces,
  EXAMPLE_PROJECTS,
  loadText,
  present,
  startSession,
  testCore,
  type TestSession,
} from './testing';

const core = await testCore();

let running: TestSession | null = null;

beforeEach(() => {
  vi.useFakeTimers({
    toFake: ['setTimeout', 'clearTimeout', 'requestAnimationFrame', 'cancelAnimationFrame'],
  });
});

afterEach(() => {
  running?.dispose();
  running = null;
  disposeWorkspaces();
  // Blockly schedules its event queue once until it is emptied: empty it before the fake timers go.
  vi.advanceTimersByTime(50);
  vi.useRealTimers();
});

/** Lets Blockly report its events, then reads the canvas and previews it. */
async function settle(session: TestSession): Promise<void> {
  vi.advanceTimersByTime(20);
  await session.session.flush();
}

function block(workspace: Blockly.Workspace, id: string): Blockly.Block {
  const found = workspace.getBlockById(id);
  if (found === null) {
    throw new Error(`no block ${id}`);
  }
  return found;
}

function connection(owner: Blockly.Block, input: string): Blockly.Connection {
  const found = owner.getInput(input)?.connection;
  if (found === null || found === undefined) {
    throw new Error(`no input ${input}`);
  }
  return found;
}

/** A new block of a catalog type, ready to connect. */
function newBlock(workspace: Blockly.Workspace, type: string): Blockly.Block {
  const created = workspace.newBlock(type);
  created.initModel();
  return created;
}

async function guessingGame(): Promise<TestSession> {
  if (core === null) {
    throw new Error('no core');
  }
  const text = EXAMPLE_PROJECTS['guessing_game.b2c'] ?? '';
  running = startSession(core, loadText(core, text));
  await settle(running);
  return running;
}

describe.skipIf(core === null)('symbol references on the guessing game', () => {
  it("lists guess and secret in the ask block's dropdown only below their declarations", async () => {
    const { workspace } = await guessingGame();
    const ask = block(workspace, 'b005');
    const field = ask.getField('VAR');
    expect(field).toBeInstanceOf(B2cSymbolRefField);
    const ref = field as B2cSymbolRefField;
    const labels = (): string[] =>
      ref.menuOptions().map((option) => (typeof option[0] === 'string' ? option[0] : ''));
    const scope = (): string[] =>
      getEditorServices()
        .symbols.symbolsAt('b005', null)
        .map((symbol) => symbol.name);

    expect(scope()).toEqual(['guess', 'secret']);
    expect(labels()).toEqual(['guess', 'secret']);

    // Move the ask block to the top of main, above both declarations.
    ask.unplug(true);
    connection(block(workspace, 'b011'), 'BODY').connect(
      present(ask.previousConnection, 'the previous connection'),
    );
    await settle(present(running, 'the session'));

    expect(scope()).toEqual([]);
    // The reference stays selected and shown, but nothing else is offered.
    expect(labels()).toEqual(['guess']);
    expect(ref.getRef()).toEqual({ ref: 's_guess' });
  });

  it('keeps the reference of a var.get dragged out of main, and the preview reports B2C-E0203', async () => {
    const session = await guessingGame();
    const { workspace } = session;
    const getter = newBlock(workspace, 'var.get');
    (getter.getField('VAR') as B2cSymbolRefField).setRef({ ref: 's_guess' });
    connection(block(workspace, 'b006'), 'ITEM0').connect(
      present(getter.outputConnection, 'the output'),
    );
    await settle(session);
    const codesAt = (id: string): string[] =>
      (useAppStore.getState().analysis.preview?.diagnostics ?? [])
        .filter((diagnostic) => diagnostic.primary.block === id)
        .map((diagnostic) => diagnostic.code);
    expect(codesAt(getter.id)).toEqual([]);
    expect(useAppStore.getState().analysis.preview?.blockTypes[getter.id]).toBe('int');

    // A function outside main, with a print in its body, and the getter moved into it.
    const helper = newBlock(workspace, 'func.define');
    (helper.getField('NAME') as B2cSymbolDeclField).setDecl({ sym: newId('sym'), name: 'helper' });
    helper.moveBy(600, 40);
    const print = newBlock(workspace, 'io.print');
    connection(helper, 'BODY').connect(
      present(print.previousConnection, 'the previous connection'),
    );
    getter.unplug();
    connection(print, 'ITEM0').connect(present(getter.outputConnection, 'the output'));
    await settle(session);

    expect((getter.getField('VAR') as B2cSymbolRefField).getRef()).toEqual({ ref: 's_guess' });
    expect(getter.getField('VAR')?.getText()).toBe('guess');
    expect(codesAt(getter.id)).toContain('B2C-E0203');
    const saved = useAppStore.getState().project?.document;
    expect(JSON.stringify(saved)).toContain(
      `"id":"${getter.id}","type":"var.get","v":1,"fields":{"VAR":{"ref":"s_guess"}}`,
    );
  });

  it("relabels every reference when 'guess' is renamed", async () => {
    const session = await guessingGame();
    const { workspace } = session;
    const declaration = block(workspace, 'b003').getField('NAME');
    expect(declaration).toBeInstanceOf(B2cSymbolDeclField);
    declaration?.setValue('attempt');
    await settle(session);

    expect(block(workspace, 'b005').getField('VAR')?.getText()).toBe('attempt');
    const condition = connection(block(workspace, 'b010'), 'COND').targetBlock();
    expect(condition?.getField('TEXT_BEFORE')?.getValue()).toBe('attempt == secret');
    const ifConditions = connection(block(workspace, 'b009'), 'COND1').targetBlock();
    expect(ifConditions?.getField('TEXT_BEFORE')?.getValue()).toBe('attempt > secret');
    // The symbol ID is unchanged: references store IDs, not names.
    expect((declaration as B2cSymbolDeclField).getDecl()).toEqual({
      sym: 's_guess',
      name: 'attempt',
    });
    expect(useAppStore.getState().project?.dirty).toBe(true);
  });

  it('keeps the last-known name of a deleted declaration until a document is loaded', async () => {
    const session = await guessingGame();
    const { workspace } = session;
    block(workspace, 'b003').dispose(true);
    await settle(session);
    const ask = block(workspace, 'b005').getField('VAR');
    expect(ask?.getText()).toBe('guess');
    expect(
      (useAppStore.getState().analysis.preview?.diagnostics ?? []).some(
        (diagnostic) => diagnostic.code === 'B2C-E0201' && diagnostic.primary.block === 'b005',
      ),
    ).toBe(true);

    const doc = useAppStore.getState().project?.document;
    if (doc === undefined) {
      throw new Error('no document');
    }
    session.session.loadDocument(doc, { clearUndo: true });
    expect(block(workspace, 'b005').getField('VAR')?.getText()).toBe('missing (s_guess)');
  });
});
