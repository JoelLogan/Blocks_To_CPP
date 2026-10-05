/**
 * *Reload* with the real compiler core and a real editing session: the file's new content replaces
 * the workspace, the undo history is cleared, there are no unsaved changes left, and an outside
 * change to trust-relevant content shows Restricted Mode (08 §8.3). Skipped when the core is not
 * built, unless CI requires it.
 */
import { render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { EditorHandle } from '../../app/editor-types';
import { StatusBar } from '../../app/layout/StatusBar';
import { useAppStore } from '../../app/store';
import { HintProvider } from '../../app/ui/Hint';
import {
  canonicalText,
  disposeWorkspaces,
  EXAMPLE_PROJECTS,
  loadText,
  present,
  startSession,
  testCore,
  type TestSession,
} from '../../editor/sync/testing';
import { answerChoice, CHANGED_OUTSIDE, createHarness, type Harness } from '../recovery/testing';
import { installExternalChangeFeature } from './feature';

const core = await testCore();

const HELLO = EXAMPLE_PROJECTS['hello_world.b2c'] ?? '';
const GUESSING_GAME = EXAMPLE_PROJECTS['guessing_game.b2c'] ?? '';

let running: TestSession | null = null;
let harness: Harness | null = null;
let uninstall: (() => void) | null = null;

beforeEach(() => {
  vi.useFakeTimers({
    toFake: ['setTimeout', 'clearTimeout', 'requestAnimationFrame', 'cancelAnimationFrame'],
  });
});

afterEach(() => {
  uninstall?.();
  uninstall = null;
  harness?.dispose();
  harness = null;
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

describe.skipIf(core === null)('Reload with the real core', () => {
  it('replaces the workspace, clears undo and shows Restricted Mode', async () => {
    const wasm = present(core, 'the compiler core');
    harness = createHarness(wasm);
    // The editor session opens Hello World as a saved, trusted project.
    running = startSession(wasm, loadText(wasm, HELLO));
    const session = running;
    await settle(session);
    let undoAfterLoad: number | null = null;
    const editor: EditorHandle = {
      loadDocument: (doc, options) => {
        session.session.loadDocument(doc, options);
        undoAfterLoad = session.workspace.getUndoStack().length;
      },
      currentDocument: () => session.session.currentDocument(),
      selectBlock: (id, options) => {
        session.session.selectBlock(id, options);
      },
      workspace: session.workspace as EditorHandle['workspace'],
    };
    harness.setEditor(editor);
    const handle = present(useAppStore.getState().project, 'the project').handle;

    // An edit: it can be undone, and the project has unsaved changes.
    present(session.workspace.getBlockById('b002'), 'block b002').setCommentText('mine');
    await settle(session);
    expect(session.workspace.getUndoStack().length).toBeGreaterThan(0);
    expect(useAppStore.getState().project?.dirty).toBe(true);

    // Another program replaced the file with the guessing game, and its trust record no longer
    // matches: the backend reports the project as restricted.
    harness.ipc.projectReload.mockResolvedValue({
      document: GUESSING_GAME,
      trust: CHANGED_OUTSIDE,
      migratedFrom: null,
    });
    uninstall = installExternalChangeFeature(harness.ctx).uninstall;
    harness.events.emit({ kind: 'projectChangedOnDisk', handle, deleted: false });
    const question = await answerChoice(harness.dialogs, 'reload');
    expect(question.options.message).toContain(
      'Reloading discards the changes you have not saved.',
    );
    await vi.waitFor(() => {
      expect(useAppStore.getState().project?.trust).toEqual(CHANGED_OUTSIDE);
    });
    await settle(session);

    const expected = loadText(wasm, GUESSING_GAME);
    const project = present(useAppStore.getState().project, 'the project');
    expect(project.handle).toBe(handle);
    expect(project.document).toEqual(expected);
    expect(project.canonicalText).toBe(canonicalText(wasm, expected));
    expect(project.dirty).toBe(false);
    expect(session.workspace.getBlockById('b011')).not.toBeNull();
    expect(session.workspace.getBlockById('b002')?.getCommentText() ?? null).toBeNull();
    // The history was cleared when the file's blocks were loaded, so the edit cannot be undone
    // into the reloaded project. (The first preview of a newly loaded document may still record
    // label updates of expression shadows: the editor's behaviour on every open, not the reload's.)
    expect(undoAfterLoad).toBe(0);
    for (const event of session.workspace.getUndoStack()) {
      expect((event as { element?: string }).element).not.toBe('comment');
    }

    // The status bar (and the trust feature's banner) follow the project's trust state.
    render(
      <HintProvider>
        <StatusBar />
      </HintProvider>,
    );
    expect(screen.getByTestId('status-restricted').textContent).toContain('Restricted Mode');
  });
});
