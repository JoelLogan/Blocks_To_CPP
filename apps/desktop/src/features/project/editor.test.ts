/**
 * The project lifecycle with the real block editor (a rendered Blockly workspace with the editing
 * session and live preview) and the real compiler core: moving a block marks the project dirty
 * and tells the backend, zooming and scrolling do not, saving writes the view that is shown, and
 * an unchanged project saves byte for byte as it was read, its view included.
 *
 * As in the app, projects open while the editor is hidden (the start page is showing): the
 * workspace has no size until the editor is shown.
 *
 * Without a build of the compiler core these tests are skipped, unless B2C_REQUIRE_WASM is set
 * (as in CI), in which case a missing build fails them.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { b2cLightTheme } from '@blocks2cpp/blockly-ext';
import { waitFor } from '@testing-library/react';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { getCore, setCore } from '../../app/core';
import { useAppStore } from '../../app/store';
import {
  type AttachedEditor,
  attachEditor,
  editorInjectOptions,
} from '../../editor/EditorWorkspace';
import { createCoreHost } from '../../editor/preview/coreHost';
import { registerEditorBlocks } from '../../editor/services';
import { withoutEvents } from '../../editor/sync/bdmToWorkspace';
import { setViewSize } from '../../editor/sync/testing';
import { clearWorkspace } from '../../editor/sync/traverse';
import { viewStateOf } from '../../editor/sync/viewport';
import {
  CORE_BUILT,
  HANDLE_A,
  HELLO_TEXT,
  type Harness,
  installHarness,
  opened,
  realCore,
} from './testing';

let core: CoreWasm;
let harness: Harness;
let workspace: Blockly.WorkspaceSvg;
let editor: AttachedEditor;

/** Longer than the preview's 50 ms debounce. */
function afterDebounce(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, 150));
}

beforeAll(async () => {
  if (CORE_BUILT) {
    core = await realCore();
  }
});

beforeEach(() => {
  if (!CORE_BUILT) {
    return;
  }
  harness = installHarness(core);
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
  const host = createCoreHost({
    initCore: () => Promise.resolve(core),
    resetCore: () => undefined,
    getCore,
    setCore,
  });
  // The editor's own options: a scrollable workspace, so zooming never moves blocks.
  registerEditorBlocks();
  const element = document.createElement('div');
  document.body.append(element);
  workspace = Blockly.inject(element, editorInjectOptions(b2cLightTheme));
  editor = attachEditor(workspace, {
    store: useAppStore,
    host,
    dialogs: harness.dialogs,
    plugins: [],
  });
  harness.setEditor(editor.handle);
  harness.ipc.projectSave.mockResolvedValue({
    savedAt: '2026-10-05T10:42:00Z',
    hash: 'f'.repeat(64),
  });
});

afterEach(() => {
  if (!CORE_BUILT) {
    return;
  }
  editor.detach();
  // As the editor does: children first and without events.
  clearWorkspace(workspace);
  withoutEvents(() => {
    workspace.dispose();
  });
  document.body.replaceChildren();
  harness.dispose();
});

/**
 * Opens a file (Hello World by default) while the editor is hidden, waits for the editor's first
 * preview, then shows the editor, as the app does when it leaves the start page.
 */
async function openHello(text = HELLO_TEXT): Promise<void> {
  harness.ipc.projectOpenDialog.mockResolvedValueOnce({
    status: 'ok',
    ...opened(text, { handle: HANDLE_A }),
  });
  expect(await harness.feature.lifecycle.open()).toBe(true);
  await waitFor(() => {
    expect(useAppStore.getState().analysis.preview).not.toBeNull();
  });
  await Blockly.renderManagement.finishQueuedRenders();
  // What EditorWorkspace's resize observer does once the editor is shown.
  setViewSize(workspace, 1000, 700);
  editor.session.resize();
  await Blockly.renderManagement.finishQueuedRenders();
}

/** Hello World with a saved view, in canonical form. */
function helloWithView(): string {
  const loaded = core.load(new TextEncoder().encode(HELLO_TEXT));
  if (!loaded.ok) {
    throw new Error('Hello World does not load');
  }
  const module = loaded.document.modules[0];
  if (module === undefined) {
    throw new Error('Hello World has no module');
  }
  module.workspace.viewport = { x: -60, y: -30, scale: 1.25 };
  const canonical = core.canonical(JSON.stringify(loaded.document));
  if (!canonical.ok) {
    throw new Error('Hello World with a view is not canonical');
  }
  return canonical.text;
}

function dirty(): boolean {
  return useAppStore.getState().project?.dirty ?? false;
}

describe.skipIf(!CORE_BUILT)('the project in the block editor', () => {
  it('shows an opened project unchanged', async () => {
    await openHello();
    expect(workspace.getTopBlocks(false)).toHaveLength(1);
    await afterDebounce();
    expect(dirty()).toBe(false);
    expect(harness.ipc.projectSetDirty).not.toHaveBeenCalled();
  });

  it.each([
    ['without a saved view', () => HELLO_TEXT],
    ['with a saved view', helloWithView],
  ])('saves an unchanged project %s byte for byte, again and again', async (_what, file) => {
    const text = file();
    await openHello(text);
    expect(await harness.feature.lifecycle.save()).toBe(true);
    expect(await harness.feature.lifecycle.save()).toBe(true);

    // Blockly's first render and showing the editor move the scroll position; that is not the
    // user scrolling, so the file's view (or its lack of one) is kept.
    const [first, second] = harness.ipc.projectSave.mock.calls.map(([request]) => request.document);
    expect(first).toBe(text);
    expect(second).toBe(text);
    expect(dirty()).toBe(false);
  });

  it('shows the view a project was saved with', async () => {
    await openHello(helloWithView());
    const state = viewStateOf(workspace);
    expect(state?.scale).toBe(1.25);
    expect(Math.round(-(state?.scrollX ?? 0) / 1.25)).toBe(-60);
    expect(Math.round(-(state?.scrollY ?? 0) / 1.25)).toBe(-30);
  });

  it('counts a moved block as a change, but not zooming or scrolling', async () => {
    await openHello();

    workspace.setScale(1.5);
    workspace.scroll(-120, -80);
    await afterDebounce();
    expect(dirty()).toBe(false);
    expect(harness.ipc.projectSetDirty).not.toHaveBeenCalled();

    const [block] = workspace.getTopBlocks(false);
    const before = block?.getRelativeToSurfaceXY().x ?? 0;
    block?.moveBy(48, 0);
    await waitFor(() => {
      expect(dirty()).toBe(true);
    });
    await waitFor(() => {
      expect(harness.ipc.projectSetDirty).toHaveBeenCalledWith({ handle: HANDLE_A, dirty: true });
    });

    // Saving writes the moved block and the view that is shown, and the project is clean again.
    expect(await harness.feature.lifecycle.save()).toBe(true);
    const sent = harness.ipc.projectSave.mock.calls[0]?.[0].document ?? '';
    const loaded = core.load(new TextEncoder().encode(sent));
    if (!loaded.ok) {
      throw new Error('the saved document does not load');
    }
    const module = loaded.document.modules[0];
    expect(module?.workspace.viewport?.scale).toBe(1.5);
    expect(module?.workspace.blocks[0]?.x).toBe(before + 48);
    expect(dirty()).toBe(false);
    await waitFor(() => {
      expect(harness.ipc.projectSetDirty).toHaveBeenLastCalledWith({
        handle: HANDLE_A,
        dirty: false,
      });
    });

    // The next live preview agrees: the saved view is part of the project now.
    block?.moveBy(0, 0);
    workspace.setScale(1.5);
    await afterDebounce();
    expect(dirty()).toBe(false);
  });
});
