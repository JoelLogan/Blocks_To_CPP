/**
 * The diagnostics plugin on a rendered workspace holding the guessing game: which block shows
 * which diagnostic, the severity that wins, part marks, stale build diagnostics, collapsed blocks,
 * placeholders, modules, and keeping up with the store and the workspace.
 */
import type { BdmBlock } from '@blocks2cpp/b2c-core-wasm';
import {
  DIAGNOSTIC_ICON_TYPE,
  getBlockDiagnostics,
  getTokenHighlight,
  partMarkKeys,
} from '@blocks2cpp/blockly-ext';
import type { Diagnostic } from '@blocks2cpp/ipc-types';
import * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { resetAppStore, useAppStore } from '../../app/store';
import { diagnosticFixture, previewFixture, projectFixture } from '../../app/testing/fixtures';
import { BadgeApplier, MAX_BADGED_DIAGNOSTICS, planBadges } from './badges';
import { pathCatalog, providePathCatalog } from './catalog';
import { diagnosticInputsFrom } from './inputs';
import { BLOCKLY_EXT_PATH_CATALOG, diagnosticsPlugin } from './plugin';
import {
  blockById,
  buildBlocks,
  disposeWorkspaces,
  editorContext,
  guessingGame,
  renderedWorkspace,
  settle,
} from './testing';

/** E0201 on the ask block's VAR field: the reference names a variable that does not exist. */
const DANGLING: Diagnostic = diagnosticFixture({
  code: 'B2C-E0201',
  message: 'There is no variable called "gues" here.',
  primary: { module: 'mod_main', block: 'b005', part: { kind: 'field', name: 'VAR' } },
});

function diagnostic(block: string | undefined, overrides: Partial<Diagnostic> = {}): Diagnostic {
  return diagnosticFixture({
    primary: {
      module: 'mod_main',
      ...(block === undefined ? {} : { block }),
      part: { kind: 'whole' },
    },
    ...overrides,
  });
}

/** A compiler message of the last build, mapped to `block` by the backend. */
function compilerMessage(block: string): Diagnostic {
  return {
    code: 'C:error',
    severity: 'error',
    message: 'no match for operator<<',
    primary: { block, part: { kind: 'whole' } },
    source: 'compiler',
    raw: "main.cpp:12:5: error: no match for 'operator<<'",
  };
}

function setLive(diagnostics: Diagnostic[]): void {
  useAppStore.getState().actions.setAnalysis({ preview: previewFixture(diagnostics) });
}

/** The guessing game on a rendered canvas, with the plugin attached. */
function attachToGuessingGame(
  blocks: BdmBlock[] = guessingGame().modules[0]?.workspace.blocks ?? [],
) {
  const workspace = renderedWorkspace();
  buildBlocks(workspace, blocks);
  const detach = diagnosticsPlugin.attach(editorContext(workspace));
  return { workspace, detach };
}

/** The IDs of the blocks with a badge, sorted. */
function badged(workspace: Blockly.Workspace): string[] {
  return workspace
    .getAllBlocks(false)
    .filter((block) => block.getIcon(DIAGNOSTIC_ICON_TYPE) !== undefined)
    .map((block) => block.id)
    .sort();
}

beforeEach(() => {
  resetAppStore();
  useAppStore.getState().actions.setProject(projectFixture({ document: guessingGame() }));
});

afterEach(() => {
  disposeWorkspaces();
});

describe('the diagnostics plugin', () => {
  it('puts a dangling reference’s E0201 badge on the right block and marks its field', async () => {
    const { workspace } = attachToGuessingGame();
    setLive([DANGLING]);
    await settle();

    expect(badged(workspace)).toEqual(['b005']);
    const ask = blockById(workspace, 'b005');
    const summary = getBlockDiagnostics(ask);
    expect(summary?.severity).toBe('error');
    expect(summary?.tooltip).toBe('✖ Error: There is no variable called "gues" here. (B2C-E0201)');
    expect(partMarkKeys(ask)).toEqual(['field:VAR:error']);
  });

  it('shows the most serious severity when a block has several', async () => {
    const { workspace } = attachToGuessingGame();
    setLive([
      diagnostic('b009', { severity: 'info', code: 'B2C-I0001' }),
      diagnostic('b009', { severity: 'warning', code: 'B2C-W0501' }),
    ]);
    await settle();
    expect(getBlockDiagnostics(blockById(workspace, 'b009'))?.severity).toBe('warning');

    setLive([
      diagnostic('b009', { severity: 'warning', code: 'B2C-W0501' }),
      diagnostic('b009', { severity: 'error', code: 'B2C-E0301' }),
      diagnostic('b009', { severity: 'info', code: 'B2C-I0001' }),
    ]);
    await settle();
    const summary = getBlockDiagnostics(blockById(workspace, 'b009'));
    expect(summary?.severity).toBe('error');
    expect(summary?.count).toBe(3);
    expect(summary?.tooltip.split('\n')[0]).toMatch(/^✖ Error: /);
  });

  it('underlines a tokens part in the expression shadow of its input', async () => {
    const { workspace } = attachToGuessingGame();
    setLive([
      diagnosticFixture({
        primary: {
          module: 'mod_main',
          block: 'b010',
          part: { kind: 'tokens', input: 'COND', start: 2, end: 3 },
        },
      }),
    ]);
    await settle();

    const shadow = blockById(workspace, 'b010').getInputTargetBlock('COND');
    expect(shadow === null ? null : getTokenHighlight(shadow)).toEqual({ start: 2, end: 3 });

    setLive([]);
    await settle();
    expect(shadow === null ? null : getTokenHighlight(shadow)).toBeNull();
    expect(badged(workspace)).toEqual([]);
  });

  it('dims the last build’s compiler messages once the project has changed', async () => {
    const { workspace } = attachToGuessingGame();
    const { actions } = useAppStore.getState();
    actions.setBuild({ diagnostics: [compilerMessage('b007')], diagnosticsHash: 'a'.repeat(64) });
    await settle();
    expect(getBlockDiagnostics(blockById(workspace, 'b007'))?.dimmed).toBe(false);

    actions.updateProject({ contentHash: 'b'.repeat(64) });
    await settle();
    const summary = getBlockDiagnostics(blockById(workspace, 'b007'));
    expect(summary?.dimmed).toBe(true);
    expect(summary?.tooltip).toContain('from the last build');
  });

  it('leaves the build’s own analyser diagnostics to the live preview', async () => {
    const { workspace } = attachToGuessingGame();
    useAppStore.getState().actions.setBuild({
      diagnostics: [diagnostic('b006', { source: 'analyser' })],
      diagnosticsHash: 'a'.repeat(64),
    });
    await settle();
    expect(badged(workspace)).toEqual([]);
  });

  it('drops a build diagnostic whose block was deleted', async () => {
    const { workspace } = attachToGuessingGame();
    useAppStore.getState().actions.setBuild({
      diagnostics: [compilerMessage('b007'), compilerMessage('b008')],
      diagnosticsHash: 'a'.repeat(64),
    });
    await settle();
    expect(badged(workspace)).toEqual(['b007', 'b008']);

    blockById(workspace, 'b007').dispose(true);
    useAppStore.getState().actions.updateProject({ contentHash: 'c'.repeat(64) });
    await settle();
    expect(badged(workspace)).toEqual(['b008']);
    expect(getBlockDiagnostics(blockById(workspace, 'b008'))?.dimmed).toBe(true);
  });

  it('shows the diagnostics of blocks inside a collapsed block on that block', async () => {
    const { workspace } = attachToGuessingGame();
    setLive([DANGLING, diagnostic('b006', { severity: 'warning', code: 'B2C-W0501' })]);
    await settle();
    expect(badged(workspace)).toEqual(['b005', 'b006']);

    blockById(workspace, 'b010').setCollapsed(true);
    await settle();
    expect(badged(workspace)).toEqual(['b010']);
    const loop = blockById(workspace, 'b010');
    const summary = getBlockDiagnostics(loop);
    expect(summary?.severity).toBe('error');
    expect(summary?.tooltip).toContain('in a block inside this one');
    // The field belongs to the hidden block: nothing on the collapsed block is marked.
    expect(partMarkKeys(loop)).toEqual([]);

    loop.setCollapsed(false);
    await settle();
    expect(badged(workspace)).toEqual(['b005', 'b006']);
  });

  it('shows only the shown module’s diagnostics, and none without a block', async () => {
    const { workspace } = attachToGuessingGame();
    setLive([
      diagnostic('b006', {
        primary: { module: 'mod_other', block: 'b006', part: { kind: 'whole' } },
      }),
      diagnostic(undefined, { code: 'B2C-E0101' }),
      diagnostic('b_not_here'),
    ]);
    await settle();
    expect(badged(workspace)).toEqual([]);
  });

  it('shows a diagnostic of a block kept inside a placeholder on the placeholder', async () => {
    const unknown: BdmBlock = {
      id: 'b100',
      type: 'pack.unknown',
      v: 1,
      x: 400,
      y: 40,
      statements: { BODY: [{ id: 'b101', type: 'io.print', v: 1 }] },
    };
    const blocks = [...(guessingGame().modules[0]?.workspace.blocks ?? []), unknown];
    const { workspace } = attachToGuessingGame(blocks);
    setLive([diagnostic('b101', { code: 'B2C-E0604' })]);
    await settle();

    const [holder] = badged(workspace);
    expect(holder).toBeDefined();
    const summary = getBlockDiagnostics(blockById(workspace, holder ?? ''));
    expect(summary?.tooltip).toContain('in a block inside this one');
  });

  it('badges blocks that come back, such as a deletion undone', async () => {
    const { workspace } = attachToGuessingGame();
    setLive([diagnostic('b006')]);
    await settle();
    expect(badged(workspace)).toEqual(['b006']);

    blockById(workspace, 'b006').dispose(true);
    await settle();
    expect(badged(workspace)).toEqual([]);

    workspace.undo(false);
    await settle();
    expect(badged(workspace)).toEqual(['b006']);
  });

  it('records nothing in the undo history and removes every badge when detached', async () => {
    const { workspace, detach } = attachToGuessingGame();
    // Building the canvas records moves (Blockly keeps blocks in bounds); start from nothing.
    await settle();
    workspace.clearUndo();
    setLive([DANGLING, diagnostic('b010')]);
    await settle();
    expect(workspace.getUndoStack()).toEqual([]);

    detach();
    expect(badged(workspace)).toEqual([]);
    setLive([diagnostic('b006')]);
    await settle();
    expect(badged(workspace)).toEqual([]);
  });

  it('provides the block catalog for Problems’ block paths', () => {
    providePathCatalog(null);
    attachToGuessingGame();
    expect(pathCatalog()).toBe(BLOCKLY_EXT_PATH_CATALOG);
  });
});

describe('planBadges and BadgeApplier', () => {
  it('stop at MAX_BADGED_DIAGNOSTICS diagnostics', () => {
    const workspace = renderedWorkspace();
    buildBlocks(workspace, guessingGame().modules[0]?.workspace.blocks ?? []);
    const many = Array.from({ length: MAX_BADGED_DIAGNOSTICS + 5 }, () => diagnostic('b006'));
    const inputs = diagnosticInputsFrom({
      preview: previewFixture(many),
      buildDiagnostics: [compilerMessage('b007')],
      diagnosticsHash: null,
      contentHash: null,
    });
    const plan = planBadges(inputs, workspace, 'mod_main');
    expect(plan.get(blockById(workspace, 'b006'))).toHaveLength(MAX_BADGED_DIAGNOSTICS);
    expect(plan.has(blockById(workspace, 'b007'))).toBe(false);
  });

  it('ignore diagnostics with an unknown severity and blocks that are gone', () => {
    const workspace = renderedWorkspace();
    buildBlocks(workspace, guessingGame().modules[0]?.workspace.blocks ?? []);
    const odd = { ...diagnostic('b006'), severity: 'fatal' } as unknown as Diagnostic;
    const inputs = diagnosticInputsFrom({
      preview: previewFixture([odd, diagnostic('b008')]),
      buildDiagnostics: [],
      diagnosticsHash: null,
      contentHash: null,
    });
    const plan = planBadges(inputs, workspace, 'mod_main');
    expect([...plan.keys()].map((block) => block.id)).toEqual(['b008']);

    const applier = new BadgeApplier();
    const print = blockById(workspace, 'b008');
    print.dispose(true);
    expect(() => {
      applier.apply(plan);
    }).not.toThrow();
    expect(getBlockDiagnostics(print)).toBeNull();
    applier.clear();
  });
});

describe('Blockly events the plugin follows', () => {
  it('a module switch re-plans the badges for the shown module', async () => {
    let moduleId = 'mod_main';
    const workspace = renderedWorkspace();
    buildBlocks(workspace, guessingGame().modules[0]?.workspace.blocks ?? []);
    const detach = diagnosticsPlugin.attach(editorContext(workspace, undefined, () => moduleId));
    setLive([diagnostic('b006')]);
    await settle();
    expect(badged(workspace)).toEqual(['b006']);

    moduleId = 'mod_other';
    useAppStore.getState().actions.updateProject({ activeModuleId: 'mod_other' });
    await settle();
    expect(badged(workspace)).toEqual([]);
    detach();
    expect(Blockly.Events.isEnabled()).toBe(true);
  });
});
