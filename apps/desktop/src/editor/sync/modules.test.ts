/**
 * The editing session's module edits (docs/plans/m3-contracts/w3-modules.md §1.2, §6 W3M-D6):
 * add, rename, delete, restore and move, made from the canvas as it is, refused while it holds a
 * change that cannot be read back, and the shown blocks built again when the module count crosses
 * one and two (*share with other modules* on *define*).
 */
import type {
  BdmBlock,
  BdmDocument,
  CanonicalResult,
  CoreWasm,
  UpdateResult,
} from '@blocks2cpp/b2c-core-wasm';
import {
  DEFAULT_EDITOR_SERVICES,
  resetEditorServices,
  SHARED_FIELD,
  setEditorServices,
} from '@blocks2cpp/blockly-ext';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { getCore, setCore } from '../../app/core';
import { resetAppStore, useAppStore } from '../../app/store';
import {
  diagnosticFixture,
  documentFixture,
  previewFixture,
  projectFixture,
} from '../../app/testing/fixtures';
import { createCoreHost } from '../preview/coreHost';
import { EditorSession } from './session';
import { disposeWorkspaces, headlessWorkspace, present } from './testing';

/** The block whose document the stand-in core's loader refuses (as one nested too deeply). */
const TOO_DEEP = 'too_deep';

/** A stand-in core: canonical text is the JSON; previews are empty; {@link TOO_DEEP} is refused. */
function stubCore(): CoreWasm {
  const unused = (): never => {
    throw new Error('unused');
  };
  const refused = (json: string): boolean => json.includes(`"${TOO_DEEP}"`);
  const problem = diagnosticFixture({ code: 'B2C-E0104', source: 'loader' });
  return {
    version: unused,
    load: unused,
    canonical: (json: string): CanonicalResult =>
      refused(json)
        ? { ok: false, diagnostics: [problem] }
        : { ok: true, text: json, hash: 'c'.repeat(64), textHash: json, diagnostics: [] },
    update: (json: string): UpdateResult =>
      refused(json)
        ? {
            ...previewFixture(),
            stage: 'load',
            diagnostics: [problem],
            buildable: false,
            contentHash: null,
            textHash: null,
          }
        : { ...previewFixture(), textHash: json },
    symbolsInScope: () => [],
    conversionTable: unused,
    clipboardMake: unused,
    pastePrepare: unused,
  };
}

/** A *define* of `name` at the top level of a canvas. */
function define(id: string, sym: string, name: string, shared = false): BdmBlock {
  return {
    id,
    type: 'func.define',
    v: 1,
    x: 10,
    y: 10,
    fields: { NAME: { sym, name }, RETURNS: 'void', ...(shared ? { SHARED: true } : {}) },
  };
}

/** A project with the given modules (`main` first). */
function project(modules: { id: string; name: string; blocks: BdmBlock[] }[]): BdmDocument {
  const doc = documentFixture('Modules');
  doc.modules = modules.map(({ id, name, blocks }) => ({ id, name, workspace: { blocks } }));
  return doc;
}

/** main (a define and a call of utils' greet) and utils (greet, shared). */
function twoModules(): BdmDocument {
  return project([
    {
      id: 'mod_main',
      name: 'main',
      blocks: [
        define('blk_area', 's_area', 'area'),
        { id: 'blk_main', type: 'program.main', v: 1, x: 10, y: 200 },
      ],
    },
    { id: 'mod_utils', name: 'utils', blocks: [define('blk_greet', 's_greet', 'greet', true)] },
  ]);
}

const sessions: EditorSession[] = [];

/** Opens `doc` (saved, clean) and starts a session on a headless canvas. */
function start(doc: BdmDocument): EditorSession {
  const core = stubCore();
  setCore(core);
  useAppStore.getState().actions.setProject(
    projectFixture({
      document: doc,
      textHash: JSON.stringify(doc),
      savedTextHash: JSON.stringify(doc),
      activeModuleId: doc.modules[0]?.id ?? '',
    }),
  );
  const host = createCoreHost({
    initCore: () => Promise.resolve(core),
    resetCore: () => undefined,
    getCore,
    setCore,
  });
  const session = new EditorSession({ workspace: headlessWorkspace(), store: useAppStore, host });
  sessions.push(session);
  return session;
}

/** The open project's document. */
function documentNow(): BdmDocument {
  return present(useAppStore.getState().project, 'project').document;
}

/** The modules' IDs and names, in order. */
function modulesNow(): string[] {
  return documentNow().modules.map((module) => `${module.id}:${module.name}`);
}

/** Every block ID of module `id`, as the document holds them. */
function blocksOf(doc: BdmDocument, id: string): unknown {
  return doc.modules.find((module) => module.id === id)?.workspace.blocks;
}

beforeEach(() => {
  resetAppStore();
  vi.useFakeTimers({
    toFake: ['setTimeout', 'clearTimeout', 'requestAnimationFrame', 'cancelAnimationFrame'],
  });
  // Blocks show *share with other modules* while the open project has several modules, as the
  // editor's symbol services say (EditorWorkspace.tsx).
  setEditorServices({
    ...DEFAULT_EDITOR_SERVICES,
    symbols: {
      ...DEFAULT_EDITOR_SERVICES.symbols,
      multiModule: () => (useAppStore.getState().project?.document.modules.length ?? 0) > 1,
    },
  });
});

afterEach(() => {
  for (const session of sessions.splice(0)) {
    session.dispose();
  }
  disposeWorkspaces();
  resetEditorServices();
  vi.advanceTimersByTime(50);
  vi.useRealTimers();
  setCore(null);
});

describe('module edits of the editing session (w3-modules.md §1.2)', () => {
  it('adds an empty module and shows it, keeping the other modules and every ID', async () => {
    const session = start(twoModules());
    await vi.advanceTimersByTimeAsync(0);
    const before = documentNow();
    expect(session.addModule('mod_tools', 'tools')).toBe('done');
    expect(modulesNow()).toEqual(['mod_main:main', 'mod_utils:utils', 'mod_tools:tools']);
    expect(session.shownModuleId()).toBe('mod_tools');
    expect(useAppStore.getState().project?.activeModuleId).toBe('mod_tools');
    expect(useAppStore.getState().project?.dirty).toBe(true);
    expect(blocksOf(documentNow(), 'mod_main')).toEqual(blocksOf(before, 'mod_main'));
    expect(blocksOf(documentNow(), 'mod_utils')).toEqual(blocksOf(before, 'mod_utils'));
    expect(session.workspace.getAllBlocks(false)).toEqual([]);
  });

  it('renames a module without changing a block or the canvas’s undo history', async () => {
    const session = start(twoModules());
    await vi.advanceTimersByTimeAsync(0);
    session.workspace.newBlock('control.forever', 'blk_new');
    await vi.advanceTimersByTimeAsync(100);
    const undo = session.workspace.getUndoStack().length;
    const before = documentNow();
    expect(session.renameModule('mod_utils', 'tools')).toBe('done');
    expect(modulesNow()).toEqual(['mod_main:main', 'mod_utils:tools']);
    expect(documentNow().modules.map((module) => module.workspace)).toEqual(
      before.modules.map((module) => module.workspace),
    );
    expect(session.shownModuleId()).toBe('mod_main');
    expect(session.workspace.getUndoStack()).toHaveLength(undo);
  });

  it('deletes a module, keeps it, and restores it at its place with its IDs', async () => {
    const session = start(
      project([
        { id: 'mod_main', name: 'main', blocks: [define('blk_area', 's_area', 'area')] },
        { id: 'mod_utils', name: 'utils', blocks: [define('blk_greet', 's_greet', 'greet')] },
        { id: 'mod_more', name: 'more', blocks: [] },
      ]),
    );
    await vi.advanceTimersByTimeAsync(0);
    useAppStore.getState().actions.updateProject({ activeModuleId: 'mod_utils' });
    expect(session.shownModuleId()).toBe('mod_utils');
    // An edit on the canvas not yet read by the preview is in the kept module.
    session.workspace.newBlock('control.forever', 'blk_fresh');
    const { outcome, kept } = session.deleteModule('mod_utils');
    expect(outcome).toBe('done');
    expect(modulesNow()).toEqual(['mod_main:main', 'mod_more:more']);
    // The module that took its place is shown.
    expect(session.shownModuleId()).toBe('mod_more');
    expect(kept?.index).toBe(1);
    expect(kept?.module.id).toBe('mod_utils');
    expect(JSON.stringify(kept?.module.workspace.blocks)).toContain('"blk_fresh"');
    const keptModule = present(kept, 'the kept module');

    expect(session.restoreModule(keptModule)).toBe('done');
    expect(modulesNow()).toEqual(['mod_main:main', 'mod_utils:utils', 'mod_more:more']);
    expect(session.shownModuleId()).toBe('mod_utils');
    expect(session.workspace.getBlockById('blk_greet')).not.toBeNull();
    expect(JSON.stringify(blocksOf(documentNow(), 'mod_utils'))).toContain('"s_greet"');
  });

  it('shows the module before when the last one shown is deleted, and refuses the only one', async () => {
    const session = start(twoModules());
    await vi.advanceTimersByTimeAsync(0);
    useAppStore.getState().actions.updateProject({ activeModuleId: 'mod_utils' });
    expect(session.deleteModule('mod_utils').outcome).toBe('done');
    expect(session.shownModuleId()).toBe('mod_main');
    const before = documentNow();
    expect(session.deleteModule('mod_main')).toEqual({ outcome: 'refused', kept: null });
    expect(documentNow()).toBe(before);
    expect(session.renameModule('mod_gone', 'x')).toBe('refused');
  });

  it('moves a module left and right; the shown module stays', async () => {
    const session = start(twoModules());
    await vi.advanceTimersByTimeAsync(0);
    expect(session.moveModule('mod_utils', -1)).toBe('done');
    expect(modulesNow()).toEqual(['mod_utils:utils', 'mod_main:main']);
    expect(session.shownModuleId()).toBe('mod_main');
    expect(session.moveModule('mod_utils', -1)).toBe('done');
    expect(modulesNow()).toEqual(['mod_utils:utils', 'mod_main:main']);
    expect(session.moveModule('mod_utils', 1)).toBe('done');
    expect(modulesNow()).toEqual(['mod_main:main', 'mod_utils:utils']);
  });

  it('refuses every module edit while the canvas holds a change that cannot be read back', async () => {
    const session = start(twoModules());
    await vi.advanceTimersByTimeAsync(0);
    session.workspace.newBlock('control.forever', TOO_DEEP);
    await vi.advanceTimersByTimeAsync(100);
    expect(useAppStore.getState().analysis.notice).toBe('syncFailed');
    const before = documentNow();
    expect(session.addModule('mod_tools', 'tools')).toBe('unchecked');
    expect(session.renameModule('mod_utils', 'tools')).toBe('unchecked');
    expect(session.deleteModule('mod_utils')).toEqual({ outcome: 'unchecked', kept: null });
    expect(session.moveModule('mod_utils', -1)).toBe('unchecked');
    expect(documentNow()).toBe(before);
    expect(session.workspace.getBlockById(TOO_DEEP)).not.toBeNull();
  });

  it('shows SHARED on the shown defines when a second module is added, and hides it again', async () => {
    const session = start(
      project([{ id: 'mod_main', name: 'main', blocks: [define('blk_area', 's_area', 'area')] }]),
    );
    await vi.advanceTimersByTimeAsync(0);
    const sharedShown = (): boolean | undefined =>
      session.workspace.getBlockById('blk_area')?.getField(SHARED_FIELD)?.isVisible();
    expect(sharedShown()).toBe(false);
    expect(session.addModule('mod_utils', 'utils')).toBe('done');
    useAppStore.getState().actions.updateProject({ activeModuleId: 'mod_main' });
    expect(session.shownModuleId()).toBe('mod_main');
    expect(sharedShown()).toBe(true);
    session.workspace.getBlockById('blk_area')?.setFieldValue('TRUE', SHARED_FIELD);
    // Deleting the other module lays the shown blocks out again, without the box but with its value.
    expect(session.deleteModule('mod_utils').outcome).toBe('done');
    expect(session.shownModuleId()).toBe('mod_main');
    expect(sharedShown()).toBe(false);
    expect(session.workspace.getBlockById('blk_area')?.getFieldValue(SHARED_FIELD)).toBe('TRUE');
    await vi.advanceTimersByTimeAsync(100);
    expect(JSON.stringify(blocksOf(documentNow(), 'mod_main'))).toContain('"SHARED":true');
  });
});
