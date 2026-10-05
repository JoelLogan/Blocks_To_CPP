/**
 * Diagnostics and highlighting with the real compiler core (built by
 * `pnpm --filter @blocks2cpp/b2c-core-wasm build`): the analyser's E0201 for a dangling reference
 * lands as a badge on the right block and as a Problems row with its block path, and a click on
 * the guessing game's `guess = b2c::ask<int>("Your guess: ");` line selects the ask block.
 *
 * Without a build these tests are skipped, unless B2C_REQUIRE_WASM is set (as in CI), in which case
 * a missing build fails them.
 */
import type { BdmDocument, CoreWasm, PreviewResult } from '@blocks2cpp/b2c-core-wasm';
import { DIAGNOSTIC_ICON_TYPE, getBlockDiagnostics } from '@blocks2cpp/blockly-ext';
import { EditorView } from '@codemirror/view';
import { act, render, screen, within } from '@testing-library/react';
import { afterEach, beforeAll, beforeEach, describe, expect, it } from 'vitest';

import { type SelectBlockOptions, setEditorHandle } from '../../app/editor-types';
import { ConnectedCodePanel, ConnectedProblemsPanel } from '../../app/panels';
import { resetAppStore, useAppStore } from '../../app/store';
import { projectFixture } from '../../app/testing/fixtures';
import { providePathCatalog } from './catalog';
import { diagnosticsPlugin } from './plugin';
import {
  blockById,
  buildBlocks,
  disposeWorkspaces,
  editorContext,
  editorHandle,
  GUESSING_GAME_TEXT,
  renderedWorkspace,
  settle,
} from './testing';

/** The optimised module the package build writes; the glob is empty without a build. */
const built =
  Object.keys(import.meta.glob('../../../../../packages/b2c-core-wasm/pkg/b2c_core_wasm_bg.wasm'))
    .length > 0;
const required = String(import.meta.env['B2C_REQUIRE_WASM'] ?? '') !== '';

let core: CoreWasm;

beforeAll(async () => {
  if (!built && !required) {
    return;
  }
  const { initCore } = await import('@blocks2cpp/b2c-core-wasm');
  core = await initCore();
});

beforeEach(() => {
  resetAppStore();
  providePathCatalog(null);
});

afterEach(() => {
  setEditorHandle(null);
  providePathCatalog(null);
  disposeWorkspaces();
});

/** The guessing game as the core loads it. */
function loadGuessingGame(): BdmDocument {
  const result = core.load(new TextEncoder().encode(GUESSING_GAME_TEXT));
  if (!result.ok) {
    throw new Error(`the guessing game does not load: ${JSON.stringify(result.diagnostics)}`);
  }
  return result.document;
}

/** The guessing game with the ask block's VAR naming a variable that does not exist. */
function danglingReference(): BdmDocument {
  const doc = loadGuessingGame();
  const loop = doc.modules[0]?.workspace.blocks[0]?.statements?.['BODY']?.[3];
  const ask = loop?.statements?.['BODY']?.[0];
  if (ask?.id !== 'b005' || ask.fields === undefined) {
    throw new Error('the guessing game changed');
  }
  ask.fields['VAR'] = { ref: 's_gone' };
  return doc;
}

function previewOf(doc: BdmDocument): PreviewResult {
  return core.preview(JSON.stringify(doc), { indentWidth: 4 });
}

/** Opens `doc` with `preview` in the store and builds its canvas. */
function open(doc: BdmDocument, preview: PreviewResult) {
  const { actions } = useAppStore.getState();
  actions.setProject(projectFixture({ document: doc, contentHash: preview.contentHash ?? '' }));
  actions.setAnalysis({ preview });
  const workspace = renderedWorkspace();
  buildBlocks(workspace, doc.modules[0]?.workspace.blocks ?? []);
  return workspace;
}

describe.skipIf(!built && !required)('with the real compiler core', () => {
  it('shows a dangling reference’s E0201 on the ask block and in Problems', async () => {
    const doc = danglingReference();
    const preview = previewOf(doc);
    const e0201 = preview.diagnostics.filter((diagnostic) => diagnostic.code === 'B2C-E0201');
    expect(e0201.map((diagnostic) => diagnostic.primary.block)).toEqual(['b005']);

    const workspace = open(doc, preview);
    const detach = diagnosticsPlugin.attach(editorContext(workspace));
    await settle();

    const badged = workspace
      .getAllBlocks(false)
      .filter((block) => block.getIcon(DIAGNOSTIC_ICON_TYPE) !== undefined)
      .map((block) => block.id);
    expect(badged).toEqual(['b005']);
    const summary = getBlockDiagnostics(blockById(workspace, 'b005'));
    expect(summary?.severity).toBe('error');
    expect(summary?.tooltip).toContain('(B2C-E0201)');

    render(<ConnectedProblemsPanel />);
    const grid = screen.getByRole('grid', { name: 'Problems' });
    const row = within(grid)
      .getAllByRole('row')
      .find((candidate) => candidate.textContent.includes('B2C-E0201'));
    expect(row?.textContent).toContain('main › repeat until › ask');
    detach();
  });

  it('selects the ask block for a click on its line of C++, or the collapsed loop', () => {
    const doc = loadGuessingGame();
    const preview = previewOf(doc);
    expect(preview.diagnostics).toEqual([]);
    const workspace = open(doc, preview);
    const selected: [string, SelectBlockOptions | undefined][] = [];
    setEditorHandle(
      editorHandle(workspace, (id, opts) => {
        selected.push([id, opts]);
      }),
    );

    const { container } = render(<ConnectedCodePanel />);
    const element = container.querySelector<HTMLElement>('.cm-editor');
    const view = element === null ? null : EditorView.findFromDOM(element);
    if (view === null) {
      throw new Error('no code view');
    }
    const text = view.state.doc.toString();
    const line = 'guess = b2c::ask<int>("Your guess: ");';
    expect(text).toContain(line);
    const click = () => {
      act(() => {
        view.dispatch({
          selection: { anchor: text.indexOf(line) + 2 },
          userEvent: 'select.pointer',
        });
      });
    };

    click();
    blockById(workspace, 'b010').setCollapsed(true);
    click();
    expect(selected).toEqual([
      ['b005', undefined],
      ['b010', undefined],
    ]);
  });
});
