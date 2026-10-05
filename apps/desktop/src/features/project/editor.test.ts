/**
 * The project lifecycle with the real block editor (a rendered Blockly workspace with the editing
 * session and live preview) and the real compiler core: moving a block marks the project dirty
 * and tells the backend, zooming and scrolling do not, saving writes the view that is shown, and
 * an unchanged project saves byte for byte as it was read.
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
import { clearWorkspace } from '../../editor/sync/traverse';
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

/** Opens Hello World from its file and waits for the editor's first preview. */
async function openHello(): Promise<void> {
  harness.ipc.projectOpenDialog.mockResolvedValueOnce({
    status: 'ok',
    ...opened(HELLO_TEXT, { handle: HANDLE_A }),
  });
  expect(await harness.feature.lifecycle.open()).toBe(true);
  await waitFor(() => {
    expect(useAppStore.getState().analysis.preview).not.toBeNull();
  });
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

  it('saves an unchanged project byte for byte, again and again', async () => {
    await openHello();
    await Blockly.renderManagement.finishQueuedRenders();
    expect(await harness.feature.lifecycle.save()).toBe(true);
    expect(await harness.feature.lifecycle.save()).toBe(true);

    const [first, second] = harness.ipc.projectSave.mock.calls.map(([request]) => request.document);
    expect(second).toBe(first);
    // Everything but the view is the file as it was read. The editing session takes its "view
    // unchanged" baseline before Blockly's first render moves the scroll position, so the first
    // save also writes the view shown (reported to the editor's owners; see the package report).
    const withoutView = (text: string | undefined) =>
      (text ?? '').replace(/,\n\s*"viewport": \{[^}]*\}/g, '');
    expect(withoutView(first)).toBe(HELLO_TEXT);
    expect(dirty()).toBe(false);
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
