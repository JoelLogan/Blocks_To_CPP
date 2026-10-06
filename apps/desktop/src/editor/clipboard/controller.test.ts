/**
 * The clipboard actions when something goes wrong, with a scripted compiler core: no core yet, a
 * core that traps (restarted once, unless the preview already replaced it), a paste target the
 * core cannot see (retried on the canvas), no randomness, a refused document or payload, a paste
 * that would break the file limits (its size measured on the canonical text), blocks whose create
 * or delete events cannot be made (a statement chain too long for Blockly, simulated here; see
 * chains.test.ts for real ones), and a canvas that cannot be edited. Nothing changes on the canvas
 * or its undo stack in any of these cases, and nothing throws.
 */
import {
  type BdmBlock,
  type CanonicalResult,
  type ClipboardMakeResult,
  CoreError,
  CoreTrap,
  type CoreWasm,
  MAX_DOCUMENT_BYTES,
  type PastePrepareResult,
} from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { resetAppStore, useAppStore } from '../../app/store';
import { diagnosticFixture, projectFixture } from '../../app/testing/fixtures';
import { guessingGame } from '../diagnostics/testing';
import { loadModule } from '../sync/bdmToWorkspace';
import { readTopBlocks } from '../sync/workspaceToBdm';
import { disposeWorkspaces, headlessWorkspace } from '../sync/testing';
import { anchorForBlock, ON_CANVAS } from './anchor';
import { ClipboardController, type ClipboardControllerOptions } from './controller';
import { ClipboardMemory } from './memory';
import type { ClipboardNotice } from './notices';

const SEED = 'ab'.repeat(32);

/** A pasted print statement. */
const PRINT: BdmBlock = {
  id: 'blk_pasted0000000000000',
  type: 'io.print',
  v: 1,
  extra: { itemCount: 1 },
  fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
  inputs: { ITEM0: { expr: [{ str: 'pasted' }] } },
};

const REFUSAL = diagnosticFixture({ code: 'B2C-E0105', source: 'loader' });

/** A compiler core whose clipboard calls answer as scripted (by default: everything works). */
function scriptedCore() {
  const clipboardMake = vi.fn<CoreWasm['clipboardMake']>((): ClipboardMakeResult => ({
    ok: true,
    payload: 'PAYLOAD',
    text: 'f();\n',
    diagnostics: [],
  }));
  const pastePrepare = vi.fn<CoreWasm['pastePrepare']>((): PastePrepareResult => ({
    ok: true,
    blocks: [PRINT],
    unresolved: [],
    diagnostics: [],
  }));
  const canonical = vi.fn<CoreWasm['canonical']>((): CanonicalResult => ({
    ok: true,
    text: '{}',
    hash: '0'.repeat(64),
    diagnostics: [],
  }));
  const unused = () => {
    throw new Error('not used by the clipboard');
  };
  const core: CoreWasm = {
    version: unused,
    load: unused,
    preview: unused,
    symbolsInScope: unused,
    conversionTable: unused,
    clipboardMake,
    pastePrepare,
    canonical,
  };
  return { core, clipboardMake, pastePrepare, canonical };
}

let workspace: Blockly.Workspace;
let notices: ClipboardNotice[];
let memory: ClipboardMemory;

beforeEach(() => {
  resetAppStore();
  const doc = guessingGame();
  useAppStore.getState().actions.setProject(projectFixture({ document: doc }));
  workspace = headlessWorkspace();
  loadModule(workspace, doc, 'mod_main');
  notices = [];
  memory = new ClipboardMemory();
});

afterEach(() => {
  disposeWorkspaces();
  resetAppStore();
});

function controller(overrides: Partial<ClipboardControllerOptions> = {}): ClipboardController {
  return new ClipboardController({
    workspace,
    store: useAppStore,
    core: () => scriptedCore().core,
    activeModuleId: () => 'mod_main',
    memory,
    notify: (notice) => notices.push(notice),
    seed: () => SEED,
    restartCore: null,
    ...overrides,
  });
}

function block(id: string): Blockly.Block {
  const found = workspace.getBlockById(id);
  if (found === null) {
    throw new Error(`no block ${id}`);
  }
  return found;
}

/** The canvas as BDM text, to show that nothing changed. */
function canvasText(): string {
  return JSON.stringify(readTopBlocks(workspace));
}

describe('with a working core', () => {
  it('copies into the in-app copy and pastes what the core prepared', () => {
    const scripted = scriptedCore();
    const clipboard = controller({ core: () => scripted.core });
    expect(clipboard.copy(block('b004'))).toEqual({ payload: 'PAYLOAD', text: 'f();\n' });
    expect(scripted.clipboardMake).toHaveBeenCalledWith(expect.any(String), ['b004']);
    expect(memory.get()).toEqual({ payload: 'PAYLOAD', text: 'f();\n' });

    const outcome = clipboard.paste(null, anchorForBlock(block('b004')));
    expect(outcome).toMatchObject({ kind: 'pasted', attached: true });
    const [text, , target, seed] = scripted.pastePrepare.mock.calls[0] ?? [];
    expect(text).toBe('PAYLOAD');
    expect(target).toEqual({ module: 'mod_main', block: 'b004', input: null });
    expect(seed).toBe(SEED);
    expect(workspace.getBlockById(PRINT.id)?.getPreviousBlock()?.id).toBe('b004');
    expect(scripted.canonical).toHaveBeenCalledOnce();
    expect(notices).toEqual([]);
  });

  it('keeps a copy without C++ as such', () => {
    const scripted = scriptedCore();
    scripted.clipboardMake.mockReturnValue({ ok: true, payload: 'P', diagnostics: [] });
    expect(controller({ core: () => scripted.core }).copy(block('b004'))).toEqual({
      payload: 'P',
      text: null,
    });
  });

  it('copies nothing but project blocks, and pastes nothing without a copy', () => {
    const clipboard = controller();
    const slot = block('b003').getInput('VALUE')?.connection?.targetBlock() ?? null;
    expect(clipboard.canCopy(slot)).toBe(false);
    expect(clipboard.canCopy(null)).toBe(false);
    expect(clipboard.paste(null, ON_CANVAS)).toEqual({ kind: 'nothing' });
  });
});

describe('without a usable core', () => {
  it('tells the user that the core is still starting', () => {
    const before = canvasText();
    const clipboard = controller({ core: () => null });
    expect(clipboard.copy(block('b004'))).toBeNull();
    expect(clipboard.paste('PAYLOAD', ON_CANVAS)).toEqual({ kind: 'failed' });
    expect(clipboard.duplicate(block('b004'))).toEqual({ kind: 'failed' });
    expect(notices.map((notice) => notice.kind === 'unavailable' && notice.reason)).toEqual([
      'coreNotStarted',
      'coreNotStarted',
      'coreNotStarted',
    ]);
    expect(memory.get()).toBeNull();
    expect(canvasText()).toBe(before);
  });

  it('restarts a core that trapped, once', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const scripted = scriptedCore();
    scripted.clipboardMake.mockImplementation(() => {
      throw new CoreTrap('clipboardMake: the compiler core stopped');
    });
    const restartCore = vi.fn(() => Promise.resolve());
    const clipboard = controller({ core: () => scripted.core, restartCore });
    expect(clipboard.copy(block('b004'))).toBeNull();
    expect(restartCore).toHaveBeenCalledOnce();
    expect(notices).toEqual([{ kind: 'unavailable', action: 'copy', reason: 'coreStopped' }]);
    expect(memory.get()).toBeNull();
  });

  it('leaves the restart to the preview when it has already replaced the trapped core', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const trapped = scriptedCore();
    trapped.pastePrepare.mockImplementation(() => {
      throw new CoreTrap('pastePrepare: the compiler core stopped');
    });
    const fresh = scriptedCore();
    // The call gets the trapped instance; by the time the trap is handled, a new one is current.
    let calls = 0;
    const restartCore = vi.fn(() => Promise.resolve());
    const clipboard = controller({
      core: () => (calls++ === 0 ? trapped.core : fresh.core),
      restartCore,
    });
    expect(clipboard.paste('PAYLOAD', ON_CANVAS)).toEqual({ kind: 'failed' });
    expect(restartCore).not.toHaveBeenCalled();
    expect(fresh.pastePrepare).not.toHaveBeenCalled();
    expect(notices).toEqual([{ kind: 'unavailable', action: 'paste', reason: 'coreStopped' }]);
  });

  it('logs a restart that fails, without throwing', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const scripted = scriptedCore();
    scripted.clipboardMake.mockImplementation(() => {
      throw new CoreTrap('clipboardMake: the compiler core stopped');
    });
    const restartCore = vi.fn(() => Promise.reject(new Error('no module')));
    expect(controller({ core: () => scripted.core, restartCore }).copy(block('b004'))).toBeNull();
    await vi.waitFor(() => {
      expect(error).toHaveBeenCalledWith(
        'The compiler core could not be restarted',
        expect.any(Error),
      );
    });
  });

  it('reports an unexpected error as internal', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const scripted = scriptedCore();
    scripted.pastePrepare.mockImplementation(() => {
      throw new CoreError('protocol', 'pastePrepare: not an object');
    });
    const before = canvasText();
    expect(controller({ core: () => scripted.core }).paste('PAYLOAD', ON_CANVAS)).toEqual({
      kind: 'failed',
    });
    expect(notices).toEqual([{ kind: 'unavailable', action: 'paste', reason: 'internal' }]);
    expect(canvasText()).toBe(before);
  });
});

describe('a paste target the core cannot see', () => {
  it('is retried on the canvas with the same seed', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const scripted = scriptedCore();
    scripted.pastePrepare.mockImplementationOnce(() => {
      throw new CoreError('invalidArguments', 'pastePrepare: unknown block');
    });
    const outcome = controller({ core: () => scripted.core }).paste(
      'PAYLOAD',
      anchorForBlock(block('b006')),
    );
    expect(outcome).toMatchObject({ kind: 'pasted', attached: false });
    const targets = scripted.pastePrepare.mock.calls.map((call) => call[2]);
    expect(targets).toEqual([
      { module: 'mod_main', block: 'b006', input: null },
      { module: 'mod_main', block: null, input: null },
    ]);
    expect(new Set(scripted.pastePrepare.mock.calls.map((call) => call[3]))).toEqual(
      new Set([SEED]),
    );
    expect(workspace.getBlockById(PRINT.id)?.getParent()).toBeNull();
    expect(notices).toEqual([]);
  });

  it('that the canvas refuses too is reported as internal', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const scripted = scriptedCore();
    scripted.pastePrepare.mockImplementation(() => {
      throw new CoreError('invalidArguments', 'pastePrepare: bad seed');
    });
    expect(controller({ core: () => scripted.core }).paste('PAYLOAD', ON_CANVAS)).toEqual({
      kind: 'failed',
    });
    expect(scripted.pastePrepare).toHaveBeenCalledOnce();
    expect(notices).toEqual([{ kind: 'unavailable', action: 'paste', reason: 'internal' }]);
  });
});

describe('refusals', () => {
  it('of the payload change nothing and list the loader’s problems', () => {
    const scripted = scriptedCore();
    scripted.pastePrepare.mockReturnValue({ ok: false, unresolved: [], diagnostics: [REFUSAL] });
    const before = canvasText();
    const outcome = controller({ core: () => scripted.core }).paste(
      'PAYLOAD',
      anchorForBlock(block('b004')),
    );
    expect(outcome).toEqual({ kind: 'refused', diagnostics: [REFUSAL] });
    expect(notices).toEqual([{ kind: 'refused', action: 'paste', diagnostics: [REFUSAL] }]);
    expect(scripted.canonical).not.toHaveBeenCalled();
    expect(canvasText()).toBe(before);
  });

  it('of the document by a copy keep the old copy', () => {
    const scripted = scriptedCore();
    scripted.clipboardMake.mockReturnValue({ ok: false, diagnostics: [REFUSAL] });
    memory.set({ payload: 'OLD', text: null });
    const clipboard = controller({ core: () => scripted.core });
    expect(clipboard.copy(block('b004'))).toBeNull();
    expect(clipboard.cut(block('b004'))).toBeNull();
    expect(memory.get()).toEqual({ payload: 'OLD', text: null });
    expect(workspace.getBlockById('b004')).not.toBeNull();
    expect(notices.map((notice) => notice.kind)).toEqual(['refused', 'refused']);
  });

  it('of the document with the blocks inserted keep the paste from happening', () => {
    const scripted = scriptedCore();
    const deep = diagnosticFixture({ code: 'B2C-E0104', source: 'loader' });
    scripted.canonical.mockReturnValue({ ok: false, diagnostics: [deep] });
    const before = canvasText();
    const outcome = controller({ core: () => scripted.core }).duplicate(block('b004'));
    expect(outcome).toEqual({ kind: 'refused', diagnostics: [deep] });
    expect(notices).toEqual([{ kind: 'limits', action: 'duplicate', diagnostics: [deep] }]);
    expect(canvasText()).toBe(before);
    // The document checked is the canvas with the block after b004.
    const checked = JSON.parse(scripted.canonical.mock.calls[0]?.[0] ?? 'null') as {
      modules: { workspace: { blocks: { statements: { BODY: { id: string }[] } }[] } }[];
    };
    const body = checked.modules[0]?.workspace.blocks[0]?.statements.BODY.map((node) => node.id);
    expect(body).toEqual(['b002', 'b003', 'b004', PRINT.id, 'b010']);
  });
});

describe('no randomness', () => {
  it('pastes nothing and says so', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const scripted = scriptedCore();
    const before = canvasText();
    const clipboard = controller({
      core: () => scripted.core,
      seed: () => {
        throw new Error('no cryptographic random source');
      },
    });
    expect(clipboard.paste('PAYLOAD', ON_CANVAS)).toEqual({ kind: 'failed' });
    expect(scripted.pastePrepare).not.toHaveBeenCalled();
    expect(notices).toEqual([{ kind: 'unavailable', action: 'paste', reason: 'internal' }]);
    expect(canvasText()).toBe(before);
  });
});

describe('blocks the canvas cannot build', () => {
  it('are removed again and reported', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const scripted = scriptedCore();
    scripted.pastePrepare.mockReturnValue({
      ok: true,
      blocks: [PRINT, { ...PRINT, id: 'blk_second000000000000' }],
      unresolved: [],
      diagnostics: [],
    });
    // Blockly fails on the second block.
    const newBlock = workspace.newBlock.bind(workspace);
    vi.spyOn(workspace, 'newBlock').mockImplementation((type, id) => {
      if (id === 'blk_second000000000000') {
        throw new Error('out of memory');
      }
      return newBlock(type, id);
    });
    const before = canvasText();
    const outcome = controller({ core: () => scripted.core }).paste('PAYLOAD', ON_CANVAS);
    expect(outcome).toEqual({ kind: 'failed' });
    expect(workspace.getBlockById(PRINT.id)).toBeNull();
    expect(canvasText()).toBe(before);
    expect(notices).toEqual([{ kind: 'unavailable', action: 'paste', reason: 'internal' }]);
  });
});

/** What V8 and JavaScriptCore throw when the stack runs out (Blockly's walks of a long chain). */
function stackOverflow(): RangeError {
  return new RangeError('Maximum call stack size exceeded');
}

/** Lets Blockly fire its queued events (they run on a timer), so that undo has recorded them. */
async function settle(): Promise<void> {
  for (let round = 0; round < 3; round++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

/** Makes Blockly's create event for the block with `id` throw `error`, as a long chain does. */
function failCreateEvent(id: string, error: Error): void {
  const Real = Blockly.Events.BlockCreate;
  vi.spyOn(Blockly.Events, 'BlockCreate').mockImplementation(function (block?: Blockly.Block) {
    if (block?.id === id) {
      throw error;
    }
    return new Real(block);
  });
}

/** Makes Blockly's delete event for the block with `id` throw `error`, as a long chain does. */
function failDeleteEvent(id: string, error: Error): void {
  const Real = Blockly.Events.BlockDelete;
  vi.spyOn(Blockly.Events, 'BlockDelete').mockImplementation(function (block?: Blockly.Block) {
    if (block?.id === id) {
      throw error;
    }
    return new Real(block);
  });
}

/** The ID of every block on the canvas, shadows included. */
function blockIds(): string[] {
  return workspace
    .getAllBlocks(false)
    .map((found) => found.id)
    .sort();
}

const SECOND: BdmBlock = { ...PRINT, id: 'blk_second000000000000' };

describe('pasted blocks whose create events cannot be made', () => {
  it('are refused whole for a stack overflow (a chain too long for Blockly): nothing is built, fired or kept', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const scripted = scriptedCore();
    scripted.pastePrepare.mockReturnValue({
      ok: true,
      blocks: [PRINT, SECOND],
      unresolved: [],
      diagnostics: [],
    });
    // The first block's event is made; the second one's runs out of stack.
    failCreateEvent(SECOND.id, stackOverflow());
    const before = canvasText();
    const ids = blockIds();
    const outcome = controller({ core: () => scripted.core }).paste('PAYLOAD', ON_CANVAS);

    const problem = expect.objectContaining({
      code: 'B2C-E0104',
      message: expect.stringContaining('A chain of 1 statement is') as unknown,
    }) as unknown;
    expect(outcome).toEqual({ kind: 'refused', diagnostics: [problem] });
    expect(notices).toEqual([{ kind: 'limits', action: 'paste', diagnostics: [problem] }]);
    expect(blockIds()).toEqual(ids);
    expect(canvasText()).toBe(before);
    await settle();
    expect(workspace.getUndoStack()).toEqual([]);
  });

  it('are removed again for any other failure, reported as internal', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const scripted = scriptedCore();
    scripted.pastePrepare.mockReturnValue({
      ok: true,
      blocks: [PRINT, SECOND],
      unresolved: [],
      diagnostics: [],
    });
    // After b004, both blocks are one chain: its first block's event serialises it.
    failCreateEvent(PRINT.id, new Error('a bug'));
    const ids = blockIds();
    const outcome = controller({ core: () => scripted.core }).duplicate(block('b004'));
    expect(outcome).toEqual({ kind: 'failed' });
    expect(notices).toEqual([{ kind: 'unavailable', action: 'duplicate', reason: 'internal' }]);
    expect(blockIds()).toEqual(ids);
    expect(block('b004').getNextBlock()?.id).toBe('b010');
    await settle();
    expect(workspace.getUndoStack()).toEqual([]);
  });

  it('measure the longest chain they hold for the notice', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const scripted = scriptedCore();
    const stack = Array.from({ length: 6 }, (_unused, index) => ({
      ...PRINT,
      id: `blk_stacked${String(index).padStart(11, '0')}`,
    }));
    scripted.pastePrepare.mockReturnValue({
      ok: true,
      blocks: [{ ...PRINT, stack }],
      unresolved: [],
      diagnostics: [],
    });
    failCreateEvent(PRINT.id, stackOverflow());
    const outcome = controller({ core: () => scripted.core }).paste('PAYLOAD', ON_CANVAS);
    expect(outcome.kind === 'refused' && outcome.diagnostics[0]?.message).toContain(
      'A chain of 7 statements',
    );
    expect(workspace.getBlockById(PRINT.id)).toBeNull();
  });
});

describe('a cut whose delete event cannot be made', () => {
  /** The guessing game with a loose stack (`loose`, then `stacked`) on the canvas. */
  function withLooseStack(): void {
    const doc = guessingGame();
    doc.modules[0]?.workspace.blocks.push({
      ...PRINT,
      id: 'loose',
      x: 900,
      y: 40,
      stack: [{ ...PRINT, id: 'stacked' }],
    });
    useAppStore.getState().actions.setProject(projectFixture({ document: doc }));
    loadModule(workspace, doc, 'mod_main');
  }

  it('deletes nothing and keeps the old copy, for a top-level block too long a chain to serialise', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    withLooseStack();
    failDeleteEvent('loose', stackOverflow());
    memory.set({ payload: 'OLD', text: null });
    const before = canvasText();
    const ids = blockIds();

    expect(controller().cut(block('loose'))).toBeNull();
    expect(memory.get()).toEqual({ payload: 'OLD', text: null });
    expect(notices).toEqual([
      {
        kind: 'limits',
        action: 'cut',
        diagnostics: [
          expect.objectContaining({
            code: 'B2C-E0104',
            message: expect.stringContaining('A chain of 2 statements') as unknown,
          }),
        ],
      },
    ]);
    expect(block('loose').isDeadOrDying()).toBe(false);
    expect(block('loose').getNextBlock()?.id).toBe('stacked');
    expect(blockIds()).toEqual(ids);
    expect(canvasText()).toBe(before);
    await settle();
    expect(workspace.getUndoStack()).toEqual([]);
  });

  it('deletes nothing, for a block in a list whose blocks cannot be serialised', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    vi.spyOn(Blockly.serialization.blocks, 'save').mockImplementation(() => {
      throw stackOverflow();
    });
    const before = canvasText();
    expect(controller().cut(block('b010'))).toBeNull();
    expect(memory.get()).toBeNull();
    expect(notices.map((notice) => notice.kind)).toEqual(['limits']);
    expect(block('b010').getPreviousBlock()?.id).toBe('b004');
    expect(canvasText()).toBe(before);
    await settle();
    expect(workspace.getUndoStack()).toEqual([]);
  });

  it('reports any other failure as internal', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    withLooseStack();
    failDeleteEvent('loose', new Error('a bug'));
    expect(controller().cut(block('loose'))).toBeNull();
    expect(memory.get()).toBeNull();
    expect(notices).toEqual([{ kind: 'unavailable', action: 'cut', reason: 'internal' }]);
    expect(block('loose').isDeadOrDying()).toBe(false);
  });

  it('when it can be made, cuts the stack as one undo step that undo and redo replay', async () => {
    withLooseStack();
    const before = canvasText();
    expect(controller().cut(block('loose'))).toEqual({ payload: 'PAYLOAD', text: 'f();\n' });
    expect(memory.get()).toEqual({ payload: 'PAYLOAD', text: 'f();\n' });
    expect(workspace.getBlockById('loose')).toBeNull();
    expect(workspace.getBlockById('stacked')).toBeNull();
    await settle();
    expect(workspace.getUndoStack()).toHaveLength(1);

    workspace.undo(false);
    await settle();
    expect(canvasText()).toBe(before);
    workspace.undo(true);
    await settle();
    expect(workspace.getBlockById('loose')).toBeNull();
    expect(workspace.getBlockById('stacked')).toBeNull();
  });
});

describe('a paste whose project file would be larger than 32 MiB', () => {
  /** A paste on the canvas for which the core's canonical text of the document is `text`. */
  function pasteWithCanonical(text: string) {
    const scripted = scriptedCore();
    scripted.canonical.mockReturnValue({ ok: true, text, hash: '0'.repeat(64), diagnostics: [] });
    notices = [];
    return controller({ core: () => scripted.core }).paste('PAYLOAD', ON_CANVAS);
  }

  it('is refused with B2C-E0101, measured in UTF-8 on the canonical text as a save measures it', () => {
    const before = canvasText();
    const tooLarge = expect.objectContaining({ code: 'B2C-E0101' }) as unknown;
    // Longer than the limit in characters already.
    expect(pasteWithCanonical('x'.repeat(MAX_DOCUMENT_BYTES + 1))).toEqual({
      kind: 'refused',
      diagnostics: [tooLarge],
    });
    expect(notices).toEqual([{ kind: 'limits', action: 'paste', diagnostics: [tooLarge] }]);
    // Fewer characters than the limit, but two bytes each in UTF-8.
    const twoByte = 'é'.repeat(MAX_DOCUMENT_BYTES / 2 + 1);
    expect(pasteWithCanonical(twoByte).kind).toBe('refused');
    expect(notices.map((notice) => notice.kind)).toEqual(['limits']);
    expect(canvasText()).toBe(before);
  });

  it('is not refused at the limit', () => {
    expect(pasteWithCanonical('é'.repeat(MAX_DOCUMENT_BYTES / 2)).kind).toBe('pasted');
    expect(notices).toEqual([]);
    expect(workspace.getBlockById(PRINT.id)).not.toBeNull();
  });
});

describe('a canvas that cannot be edited', () => {
  it('copies but does not cut, paste or duplicate', () => {
    const scripted = scriptedCore();
    const clipboard = controller({ core: () => scripted.core });
    workspace.setIsReadOnly(true);
    expect(clipboard.canEdit()).toBe(false);
    expect(clipboard.canCopy(block('b004'))).toBe(true);
    expect(clipboard.canCut(block('b004'))).toBe(false);
    expect(clipboard.canDuplicate(block('b004'))).toBe(false);
    expect(clipboard.cut(block('b004'))).toBeNull();
    memory.set({ payload: 'PAYLOAD', text: null });
    expect(clipboard.paste(null, ON_CANVAS)).toEqual({ kind: 'nothing' });
    expect(clipboard.duplicate(block('b004'))).toEqual({ kind: 'nothing' });
    expect(scripted.pastePrepare).not.toHaveBeenCalled();
  });

  it('or no project: nothing at all', () => {
    useAppStore.getState().actions.setProject(null);
    const clipboard = controller();
    expect(clipboard.canCopy(block('b004'))).toBe(false);
    expect(clipboard.canEdit()).toBe(false);
    expect(clipboard.copy(block('b004'))).toBeNull();
    expect(notices).toEqual([]);
  });

  it('or another module than the document has: nothing at all', () => {
    const clipboard = controller({ activeModuleId: () => 'mod_gone' });
    expect(clipboard.canCopy(block('b004'))).toBe(false);
    expect(clipboard.paste('PAYLOAD', ON_CANVAS)).toEqual({ kind: 'nothing' });
  });
});
