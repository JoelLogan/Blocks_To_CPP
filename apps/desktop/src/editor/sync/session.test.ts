/**
 * The editing session with a stand-in core: what changes start the pipeline, the dirty state,
 * switching modules, closing the project, selecting blocks and capturing the viewport for saving.
 */
import type { BdmBlock, BdmDocument, CanonicalResult, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { getCore, setCore } from '../../app/core';
import { resetAppStore, useAppStore } from '../../app/store';
import { documentFixture, previewFixture, projectFixture } from '../../app/testing/fixtures';
import { createCoreHost } from '../preview/coreHost';
import { SyncError } from './errors';
import { EditorSession, type SessionHooks } from './session';
import { disposeWorkspaces, headlessWorkspace, renderedWorkspace } from './testing';

/** A stand-in core: the canonical text is the JSON itself; previews are empty. */
function stubCore(): CoreWasm {
  const unused = (): never => {
    throw new Error('unused');
  };
  return {
    version: unused,
    load: unused,
    canonical: (json: string): CanonicalResult => ({
      ok: true,
      text: json,
      hash: 'c'.repeat(64),
      diagnostics: [],
    }),
    preview: () => previewFixture(),
    symbolsInScope: () => [],
    conversionTable: unused,
    clipboardMake: unused,
    pastePrepare: unused,
  };
}

const forever = (id: string): BdmBlock => ({ id, type: 'control.forever', v: 1 });

/** Two modules: main with a loose block, util with another. */
function twoModules(): BdmDocument {
  const doc = documentFixture('Modules');
  doc.modules = [
    {
      id: 'mod_main',
      name: 'main',
      workspace: {
        blocks: [{ ...forever('a'), x: 10, y: 10 }],
        viewport: { x: 3, y: 4, scale: 1.25 },
      },
    },
    { id: 'mod_util', name: 'util', workspace: { blocks: [{ ...forever('b'), x: 20, y: 20 }] } },
  ];
  return doc;
}

const sessions: EditorSession[] = [];

function start(
  doc: BdmDocument,
  workspace: Blockly.Workspace = headlessWorkspace(),
  hooks: SessionHooks = {},
): EditorSession {
  const core = stubCore();
  setCore(core);
  useAppStore.getState().actions.setProject(
    projectFixture({
      document: doc,
      canonicalText: JSON.stringify(doc),
      savedCanonicalText: JSON.stringify(doc),
      activeModuleId: 'mod_main',
    }),
  );
  const host = createCoreHost({
    initCore: () => Promise.resolve(core),
    resetCore: () => undefined,
    getCore,
    setCore,
  });
  const session = new EditorSession({ workspace, store: useAppStore, host, hooks });
  sessions.push(session);
  return session;
}

beforeEach(() => {
  resetAppStore();
  vi.useFakeTimers({
    toFake: ['setTimeout', 'clearTimeout', 'requestAnimationFrame', 'cancelAnimationFrame'],
  });
});

afterEach(() => {
  for (const session of sessions.splice(0)) {
    session.dispose();
  }
  disposeWorkspaces();
  vi.advanceTimersByTime(50);
  vi.useRealTimers();
  setCore(null);
});

describe('the editing session', () => {
  it('shows the active module and previews it at once', async () => {
    const onLoaded = vi.fn();
    const onPreviewed = vi.fn();
    const session = start(twoModules(), headlessWorkspace(), { onLoaded, onPreviewed });
    expect(session.shownModuleId()).toBe('mod_main');
    expect(session.workspace.getBlockById('a')).not.toBeNull();
    expect(onLoaded).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(0);
    expect(onPreviewed).toHaveBeenCalledTimes(1);
    expect(useAppStore.getState().project?.dirty).toBe(false);
  });

  it('previews 50 ms after a change and marks the project dirty', async () => {
    const onCommitted = vi.fn();
    const session = start(twoModules(), headlessWorkspace(), { onCommitted });
    await vi.advanceTimersByTimeAsync(0);
    onCommitted.mockClear();

    session.workspace.getBlockById('a')?.setCommentText('later');
    await vi.advanceTimersByTimeAsync(20);
    expect(session.syncPending).toBe(true);
    expect(onCommitted).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(60);
    expect(onCommitted).toHaveBeenCalledTimes(1);
    const project = useAppStore.getState().project;
    expect(project?.dirty).toBe(true);
    expect(project?.document.modules[0]?.workspace.blocks[0]).toMatchObject({
      id: 'a',
      comment: { text: 'later' },
    });

    // Undoing the change makes the document equal to the saved one again.
    session.workspace.getBlockById('a')?.setCommentText(null);
    await vi.advanceTimersByTimeAsync(80);
    expect(useAppStore.getState().project?.dirty).toBe(false);
  });

  it('ignores UI events, except opening or closing a comment', async () => {
    const session = start(twoModules());
    await vi.advanceTimersByTimeAsync(0);
    const block = session.workspace.getBlockById('a');
    Blockly.Events.fire(
      new (Blockly.Events.get(Blockly.Events.SELECTED))(null, 'a', session.workspace.id),
    );
    await vi.advanceTimersByTimeAsync(20);
    expect(session.syncPending).toBe(false);

    const BubbleOpen = Blockly.Events.get(Blockly.Events.BUBBLE_OPEN);
    Blockly.Events.fire(new BubbleOpen(block, true, 'comment'));
    await vi.advanceTimersByTimeAsync(20);
    expect(session.syncPending).toBe(true);
  });

  it('switches modules, keeping the left module in the document and clearing undo', async () => {
    const session = start(twoModules());
    await vi.advanceTimersByTimeAsync(0);
    session.workspace.getBlockById('a')?.setCommentText('kept');
    await vi.advanceTimersByTimeAsync(20);
    expect(session.workspace.getUndoStack().length).toBeGreaterThan(0);

    // The switcher only changes the store; the session follows.
    useAppStore.getState().actions.updateProject({ activeModuleId: 'mod_util' });
    expect(session.shownModuleId()).toBe('mod_util');
    expect(session.workspace.getBlockById('a')).toBeNull();
    expect(session.workspace.getBlockById('b')).not.toBeNull();
    expect(session.workspace.getUndoStack()).toHaveLength(0);
    expect(useAppStore.getState().project?.document.modules[0]?.workspace.blocks[0]).toMatchObject({
      id: 'a',
      comment: { text: 'kept' },
    });
    await vi.advanceTimersByTimeAsync(0);
    expect(useAppStore.getState().project?.dirty).toBe(true);

    // An unknown module is refused: the store goes back to the shown one.
    useAppStore.getState().actions.updateProject({ activeModuleId: 'mod_none' });
    expect(session.shownModuleId()).toBe('mod_util');
    expect(useAppStore.getState().project?.activeModuleId).toBe('mod_util');
  });

  it('selects a block in another module by switching to it', async () => {
    const session = start(twoModules(), renderedWorkspace());
    await vi.advanceTimersByTimeAsync(0);
    session.selectBlock('b', { center: true });
    expect(session.shownModuleId()).toBe('mod_util');
    expect(Blockly.common.getSelected()).toBe(session.workspace.getBlockById('b'));
  });

  it('selects the outermost collapsed block around the one asked for', async () => {
    const doc = documentFixture();
    doc.modules = [
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
              statements: {
                BODY: [
                  {
                    id: 'outer',
                    type: 'control.forever',
                    v: 1,
                    collapsed: true,
                    statements: { BODY: [forever('inner')] },
                  },
                ],
              },
            },
          ],
        },
      },
    ];
    const session = start(doc, renderedWorkspace());
    await vi.advanceTimersByTimeAsync(0);
    session.selectBlock('inner');
    expect(Blockly.common.getSelected()).toBe(session.workspace.getBlockById('outer'));
    session.selectBlock('no_such_block');
    expect(Blockly.common.getSelected()).toBe(session.workspace.getBlockById('outer'));
  });

  it('empties the canvas when the project closes, and refuses to save without one', async () => {
    const session = start(twoModules());
    await vi.advanceTimersByTimeAsync(0);
    useAppStore.getState().actions.setProject(null);
    expect(session.shownModuleId()).toBeNull();
    expect(session.workspace.getAllBlocks(false)).toHaveLength(0);
    expect(() => session.currentDocument()).toThrow(SyncError);
  });

  it('makes a loaded document the project document', async () => {
    const session = start(twoModules());
    await vi.advanceTimersByTimeAsync(0);
    const other = twoModules();
    other.project.name = 'Reloaded';
    session.loadDocument(other, { clearUndo: false });
    expect(useAppStore.getState().project?.document).toBe(other);
    await vi.advanceTimersByTimeAsync(0);
    expect(useAppStore.getState().project?.document.project.name).toBe('Reloaded');
  });

  it("captures the viewport only for saving, keeping the file's while the view is unchanged", async () => {
    const session = start(twoModules(), renderedWorkspace());
    await vi.advanceTimersByTimeAsync(0);
    expect(session.currentDocument().modules[0]?.workspace.viewport).toEqual({
      x: 3,
      y: 4,
      scale: 1.25,
    });

    const workspace = session.workspace as Blockly.WorkspaceSvg;
    workspace.setScale(2);
    const saved = session.currentDocument().modules[0]?.workspace.viewport;
    expect(saved?.scale).toBe(2);
    // The live document (what decides dirty) never holds the view.
    await session.flush();
    expect(useAppStore.getState().project?.document.modules[0]?.workspace.viewport).toEqual({
      x: 3,
      y: 4,
      scale: 1.25,
    });

    // A module left behind keeps the view it was left with.
    useAppStore.getState().actions.updateProject({ activeModuleId: 'mod_util' });
    expect(session.currentDocument().modules[0]?.workspace.viewport?.scale).toBe(2);
    expect(session.currentDocument().modules[1]?.workspace.viewport).toBeUndefined();
  });

  it('keeps reading until a drag has ended', async () => {
    const session = start(twoModules(), renderedWorkspace());
    await vi.advanceTimersByTimeAsync(0);
    const workspace = session.workspace as Blockly.WorkspaceSvg;
    const dragging = vi.spyOn(workspace, 'isDragging').mockReturnValue(true);
    workspace.getBlockById('a')?.moveBy(30, 0);
    await vi.advanceTimersByTimeAsync(80);
    expect(useAppStore.getState().project?.dirty).toBe(false);
    expect(session.syncPending).toBe(true);
    dragging.mockReturnValue(false);
    await vi.advanceTimersByTimeAsync(60);
    expect(useAppStore.getState().project?.dirty).toBe(true);
  });
});
