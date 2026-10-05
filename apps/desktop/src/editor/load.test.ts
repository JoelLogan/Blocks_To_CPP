/** Opening a document in the editor through the compiler core's loader. */
import {
  type BdmDocument,
  CoreTrap,
  type CoreWasm,
  MAX_DOCUMENT_BYTES,
} from '@blocks2cpp/b2c-core-wasm';
import { afterEach, beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

import { setCore } from '../app/core';
import type { EditorHandle } from '../app/editor-types';
import type { FeatureContext } from '../app/features';
import { resetAppStore, useAppStore } from '../app/store';
import { openDocumentInEditor, type OpenDocumentArgs } from './load';
import { canonicalText, EXAMPLE_PROJECTS, loadText, present, testCore } from './sync/testing';

const core = await testCore();

const TRUSTED = {
  state: 'trusted',
  source: 'project',
  restrictedReason: null,
  markOfTheWeb: false,
} as const;
const HANDLE = 'ph_0123456789abcdef0123456789abcdef';

function context(wasm: CoreWasm | null, editor: EditorHandle | null = null): FeatureContext {
  return {
    store: useAppStore,
    core: () => wasm,
    editor: () => editor,
  } as unknown as FeatureContext;
}

/** A stand-in editor and the mock of its `loadDocument`. */
function fakeEditor(): { editor: EditorHandle; loadDocument: Mock<EditorHandle['loadDocument']> } {
  const loadDocument = vi.fn<EditorHandle['loadDocument']>();
  return {
    loadDocument,
    editor: {
      loadDocument,
      currentDocument: vi.fn<EditorHandle['currentDocument']>(),
      selectBlock: vi.fn<EditorHandle['selectBlock']>(),
      workspace: {} as never,
    },
  };
}

function args(overrides: Partial<OpenDocumentArgs> = {}): OpenDocumentArgs {
  const text = EXAMPLE_PROJECTS['guessing_game.b2c'] ?? '';
  return {
    handle: HANDLE,
    documentText: text,
    trust: TRUSTED,
    fileName: 'guessing_game.b2c',
    migratedFrom: null,
    savedText: text,
    ...overrides,
  };
}

beforeEach(() => {
  resetAppStore();
});

afterEach(() => {
  setCore(null);
});

describe.skipIf(core === null)('opening a document', () => {
  it('loads it, makes it the open project, shows it and switches to the editor', async () => {
    if (core === null) {
      return;
    }
    const { editor, loadDocument } = fakeEditor();
    const result = await openDocumentInEditor(context(core, editor), args());
    expect(result).toEqual({ ok: true });
    const { project, ui } = useAppStore.getState();
    const expected = loadText(core, args().documentText);
    expect(project?.document).toEqual(expected);
    expect(project?.canonicalText).toBe(canonicalText(core, expected));
    expect(project?.contentHash).toMatch(/^[0-9a-f]{64}$/);
    expect(project?.savedCanonicalText).toBe(project?.canonicalText);
    expect(project?.dirty).toBe(false);
    expect(project?.activeModuleId).toBe('mod_main');
    expect(project?.trust).toEqual(TRUSTED);
    expect(project?.savedAt).toBeNull();
    expect(ui.screen).toBe('editor');
    expect(loadDocument).toHaveBeenCalledWith(project?.document, { clearUndo: true });
  });

  it('marks a project that was never saved as changed', async () => {
    await openDocumentInEditor(context(core), args({ savedText: null, fileName: null }));
    const project = useAppStore.getState().project;
    expect(project?.savedCanonicalText).toBeNull();
    expect(project?.dirty).toBe(true);
  });

  it('compares a restored snapshot with the canonical form of the saved file', async () => {
    const text = args().documentText;
    // The same project, formatted differently: no unsaved changes.
    const reformatted = JSON.stringify(JSON.parse(text));
    await openDocumentInEditor(context(core), args({ savedText: reformatted }));
    expect(useAppStore.getState().project?.dirty).toBe(false);

    const changed = text.replace('Guess a number from 1 to 100!', 'Guess!');
    await openDocumentInEditor(context(core), args({ documentText: changed }));
    expect(useAppStore.getState().project?.dirty).toBe(true);

    await openDocumentInEditor(context(core), args({ savedText: 'not a project' }));
    expect(useAppStore.getState().project?.savedCanonicalText).toBeNull();
  });

  it("returns the loader's problems and leaves the editor alone", async () => {
    const { editor, loadDocument } = fakeEditor();
    const result = await openDocumentInEditor(
      context(core, editor),
      args({ documentText: '{"format": "blocks2cpp/project", "formatVersion": 1}' }),
    );
    expect(result.ok).toBe(false);
    expect(result.ok ? [] : result.diagnostics.map((d) => d.code)).not.toEqual([]);
    expect(useAppStore.getState().project).toBeNull();
    expect(loadDocument).not.toHaveBeenCalled();
    expect(useAppStore.getState().ui.screen).toBe('start');
  });

  it('refuses text over the size limit without encoding all of it', async () => {
    const huge = ' '.repeat(MAX_DOCUMENT_BYTES + 10);
    const result = await openDocumentInEditor(context(core), args({ documentText: huge }));
    expect(result.ok ? [] : result.diagnostics.map((d) => d.code)).toContain('B2C-E0101');
  });

  it('keeps the shown module and the save time when the open project is reopened', async () => {
    const doc: BdmDocument = loadText(present(core, 'the core'), args().documentText);
    doc.modules.push({ id: 'mod_more', name: 'more', workspace: { blocks: [] } });
    const text = JSON.stringify(doc);
    await openDocumentInEditor(context(core), args({ documentText: text, savedText: text }));
    useAppStore
      .getState()
      .actions.updateProject({ activeModuleId: 'mod_more', savedAt: '2026-10-05T12:00:00Z' });
    await openDocumentInEditor(context(core), args({ documentText: text, savedText: text }));
    const project = useAppStore.getState().project;
    expect(project?.activeModuleId).toBe('mod_more');
    expect(project?.savedAt).toBe('2026-10-05T12:00:00Z');

    await openDocumentInEditor(
      context(core),
      args({ handle: 'ph_ffffffffffffffffffffffffffffffff', documentText: text, savedText: text }),
    );
    expect(useAppStore.getState().project?.activeModuleId).toBe('mod_main');
    expect(useAppStore.getState().project?.savedAt).toBeNull();
  });

  it('starts a fresh core when the first one traps', async () => {
    const trapping = {
      load: () => {
        throw new CoreTrap('load: unreachable');
      },
    } as unknown as CoreWasm;
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const result = await openDocumentInEditor(context(trapping), args());
    expect(result).toEqual({ ok: true });
    expect(useAppStore.getState().project?.handle).toBe(HANDLE);
  });
});
