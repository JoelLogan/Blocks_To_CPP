import type { BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import { B2cSymbolDeclField, GENERATED_ID_PATTERN } from '@blocks2cpp/blockly-ext';
import { act, fireEvent, render, screen } from '@testing-library/react';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import { DialogHost } from '../../app/dialogs/DialogHost';
import { createDialogQueue, type DialogService } from '../../app/dialogs/service';
import { expectNoAxeViolations } from '../../test/axe';
import {
  MAKE_VARIABLE_QUESTION,
  MAKE_VARIABLE_TITLE,
  insertDeclaration,
  makeVariable,
  type MakeVariableContext,
} from './makeVariable';
import type { SymbolSource } from './scope';
import {
  disposeTestWorkspaces,
  fakeSymbols,
  guessDocument,
  headlessWorkspace,
  loadModule,
  setUpToolboxBlocks,
  symbolFixture,
} from './testing';

beforeAll(() => {
  setUpToolboxBlocks();
});

afterEach(() => {
  disposeTestWorkspaces();
});

/** Lets Blockly fire its queued events (and so record them for undo), as it does: after a frame. */
function events(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      setTimeout(resolve, 0);
    });
  });
}

/** A headless canvas with `document`'s first module (the guess document by default). */
function canvas(document: BdmDocument = guessDocument()): Blockly.Workspace {
  const workspace = headlessWorkspace();
  loadModule(workspace, document);
  return workspace;
}

/** Dialogs that answer the prompt with `answer` and record what they were asked. */
function answering(answer: string | null) {
  return {
    prompt: vi.fn<DialogService['prompt']>(() => Promise.resolve(answer)),
    alert: vi.fn<DialogService['alert']>(() => Promise.resolve()),
  };
}

/** The context of *Make a variable* on `workspace`, with nothing selected by default. */
function context(
  workspace: Blockly.Workspace,
  overrides: Partial<MakeVariableContext> & { selectedId?: string } = {},
): MakeVariableContext {
  const { selectedId, ...rest } = overrides;
  return {
    workspace,
    symbols: fakeSymbols(),
    dialogs: answering('score'),
    selected: () => (selectedId === undefined ? null : workspace.getBlockById(selectedId)),
    mainInOtherModule: () => false,
    ...rest,
  };
}

/** A block of the canvas. */
function get(workspace: Blockly.Workspace, id: string): Blockly.Block {
  const block = workspace.getBlockById(id);
  if (block === null) {
    throw new Error(`no block ${id}`);
  }
  return block;
}

/** The declaration a `var.declare` block makes. */
function declOf(block: Blockly.Block | null | undefined) {
  const field = block?.getField('NAME');
  return field instanceof B2cSymbolDeclField ? field.getDecl() : null;
}

/** The IDs of a statement list, from its first block. */
function listIds(first: Blockly.Block | null | undefined): string[] {
  const ids: string[] = [];
  for (let block = first ?? null; block !== null; block = block.getNextBlock()) {
    ids.push(block.id);
  }
  return ids;
}

/** The new block of a created result. */
async function created(promise: ReturnType<typeof makeVariable>): Promise<Blockly.Block> {
  const result = await promise;
  if (result.status !== 'created') {
    throw new Error(`Make a variable ended ${result.status}`);
  }
  return result.block;
}

describe('Make a variable: where the declaration goes', () => {
  it('goes to the top of main when nothing is selected', async () => {
    const workspace = canvas();
    const block = await created(makeVariable(context(workspace)));
    expect(block.type).toBe('var.declare');
    expect(listIds(workspace.getBlockById('main')?.getInputTargetBlock('BODY'))).toEqual([
      block.id,
      'decl',
      'print',
    ]);
    expect(declOf(block)).toMatchObject({ name: 'score' });
    expect(block.getFieldValue('TYPE')).toBe('int');
  });

  it('goes just before the selected statement, so the statement can use it', async () => {
    const workspace = canvas();
    const block = await created(makeVariable(context(workspace, { selectedId: 'print' })));
    expect(listIds(workspace.getBlockById('main')?.getInputTargetBlock('BODY'))).toEqual([
      'decl',
      block.id,
      'print',
    ]);
  });

  it('goes before the statement that holds a selected value (or its shadow)', async () => {
    const workspace = canvas();
    const shadow = workspace.getBlockById('print')?.getInputTargetBlock('ITEM0');
    expect(shadow?.isShadow()).toBe(true);
    const block = await created(
      makeVariable(context(workspace, { selected: () => shadow ?? null })),
    );
    expect(block.getNextBlock()?.id).toBe('print');
  });

  it('goes to the top of a selected main or function', async () => {
    const workspace = canvas();
    const inMain = await created(makeVariable(context(workspace, { selectedId: 'main' })));
    expect(workspace.getBlockById('main')?.getInputTargetBlock('BODY')).toBe(inMain);
    const inFunction = await created(makeVariable(context(workspace, { selectedId: 'fn' })));
    expect(workspace.getBlockById('fn')?.getInputTargetBlock('BODY')).toBe(inFunction);
  });

  it('goes to the top of main when the selected block is not part of the program', async () => {
    const workspace = canvas();
    const loose = Blockly.serialization.blocks.append(
      { type: 'io.print', id: 'loose', x: 600, y: 600, extraState: { itemCount: 1 } },
      workspace,
    );
    const block = await created(makeVariable(context(workspace, { selectedId: loose.id })));
    expect(block.getNextBlock()?.id).toBe('decl');
    expect(loose.getPreviousBlock()).toBeNull();
  });

  it('creates main when the canvas has none, and puts the declaration in it', async () => {
    const document = guessDocument();
    const module = document.modules[0];
    if (module !== undefined) {
      module.workspace.blocks = [];
    }
    const workspace = canvas(document);
    const block = await created(makeVariable(context(workspace)));
    const main = block.getParent();
    expect(main?.type).toBe('program.main');
    expect(main?.getInputTargetBlock('BODY')).toBe(block);
    expect(main?.id).toMatch(GENERATED_ID_PATTERN.blk);
    expect(workspace.getTopBlocks(false)).toEqual([main]);
  });

  it('refuses to create main when another module has it', async () => {
    const document = guessDocument();
    const module = document.modules[0];
    if (module !== undefined) {
      module.workspace.blocks = [];
    }
    const workspace = canvas(document);
    const dialogs = answering('score');
    const result = await makeVariable(
      context(workspace, { dialogs, mainInOtherModule: () => true }),
    );
    expect(result).toEqual({ status: 'refused', reason: 'noMain' });
    expect(dialogs.alert).toHaveBeenCalledTimes(1);
    expect(dialogs.prompt).not.toHaveBeenCalled();
    expect(workspace.getAllBlocks(false)).toHaveLength(0);
  });

  it('looks for the place again when its block went away while the dialog was open', async () => {
    const workspace = canvas();
    let selected: Blockly.Block | null = workspace.getBlockById('print');
    const dialogs = {
      prompt: vi.fn<DialogService['prompt']>(() => {
        workspace.getBlockById('print')?.dispose(true);
        selected = null;
        return Promise.resolve('score');
      }),
      alert: vi.fn<DialogService['alert']>(() => Promise.resolve()),
    };
    const block = await created(
      makeVariable(context(workspace, { dialogs, selected: () => selected })),
    );
    expect(listIds(workspace.getBlockById('main')?.getInputTargetBlock('BODY'))).toEqual([
      block.id,
      'decl',
    ]);
  });
});

describe('Make a variable: the name and the new symbol', () => {
  const scope: SymbolSource = fakeSymbols({
    'main/BODY': [
      symbolFixture('s_value', 'value'),
      symbolFixture('s_fact', 'factorial', { kind: 'function', params: [], returns: 'int' }),
    ],
  });

  it('suggests the first free name and refuses names in use where it goes', async () => {
    const workspace = canvas();
    const dialogs = answering('score');
    await makeVariable(context(workspace, { dialogs, symbols: scope }));
    const options = dialogs.prompt.mock.calls[0]?.[0];
    expect(options).toMatchObject({
      title: MAKE_VARIABLE_TITLE,
      message: MAKE_VARIABLE_QUESTION,
      defaultValue: 'value2',
      maxLength: 64,
    });
    const validate = options?.validate;
    // Visible at the top of main, or declared in the same list (guess, further down).
    expect(validate?.('value')).toMatch(/already something called “value”/);
    expect(validate?.('factorial')).toMatch(/already something called/);
    expect(validate?.('guess')).toMatch(/already something called “guess”/);
    expect(validate?.('int')).toMatch(/keyword/);
    expect(validate?.('score')).toBeNull();
  });

  it('declares a new symbol each time', async () => {
    const workspace = canvas();
    const first = declOf(await created(makeVariable(context(workspace))));
    const second = declOf(
      await created(makeVariable(context(workspace, { dialogs: answering('other') }))),
    );
    expect(first?.sym).toMatch(GENERATED_ID_PATTERN.sym);
    expect(second?.sym).toMatch(GENERATED_ID_PATTERN.sym);
    expect(second?.sym).not.toBe(first?.sym);
    expect([first?.sym, second?.sym]).not.toContain('s_guess');
  });

  it('starts the value at 0, saved with the block', async () => {
    const workspace = canvas();
    const block = await created(makeVariable(context(workspace)));
    const shadow = block.getInputTargetBlock('VALUE');
    expect(shadow?.isShadow()).toBe(true);
    expect(shadow?.getFieldValue('VALUE')).toBe('0');
  });

  it('changes nothing when cancelled', async () => {
    const workspace = canvas();
    const before = workspace.getAllBlocks(false).length;
    expect(await makeVariable(context(workspace, { dialogs: answering(null) }))).toEqual({
      status: 'cancelled',
    });
    expect(workspace.getAllBlocks(false)).toHaveLength(before);
  });

  it('refuses a name that breaks the rules, even if the dialog let it through', async () => {
    const workspace = canvas();
    for (const name of ['guess', '2fast', 'int', 'a b', 'b2cValue', 'x'.repeat(65)]) {
      expect(await makeVariable(context(workspace, { dialogs: answering(name) }))).toEqual({
        status: 'refused',
        reason: 'invalidName',
      });
    }
    expect(listIds(workspace.getBlockById('main')?.getInputTargetBlock('BODY'))).toEqual([
      'decl',
      'print',
    ]);
  });

  it('does nothing on a read-only canvas', async () => {
    const workspace = canvas();
    workspace.setIsReadOnly(true);
    const dialogs = answering('score');
    expect(await makeVariable(context(workspace, { dialogs }))).toEqual({
      status: 'refused',
      reason: 'readOnly',
    });
    expect(dialogs.prompt).not.toHaveBeenCalled();
  });

  it('does nothing when the canvas became read-only while the dialog was open', async () => {
    const workspace = canvas();
    const dialogs = {
      prompt: vi.fn<DialogService['prompt']>(() => {
        workspace.setIsReadOnly(true);
        return Promise.resolve('score');
      }),
      alert: vi.fn<DialogService['alert']>(() => Promise.resolve()),
    };
    expect(await makeVariable(context(workspace, { dialogs }))).toEqual({
      status: 'refused',
      reason: 'readOnly',
    });
  });
});

describe('insertDeclaration', () => {
  it('is one undo step, also when it creates main', async () => {
    const workspace = canvas();
    workspace.clearUndo();
    insertDeclaration(workspace, { kind: 'top', container: get(workspace, 'main') }, 'score');
    await events();
    workspace.undo(false);
    await events();
    expect(listIds(workspace.getBlockById('main')?.getInputTargetBlock('BODY'))).toEqual([
      'decl',
      'print',
    ]);

    const empty = headlessWorkspace();
    insertDeclaration(empty, { kind: 'newMain' }, 'score');
    await events();
    expect(
      empty
        .getAllBlocks(false)
        .map((block) => block.type)
        .sort(),
    ).toContain('program.main');
    empty.undo(false);
    await events();
    expect(empty.getAllBlocks(false)).toHaveLength(0);
  });

  it('joins the event group that is already open', async () => {
    const workspace = canvas();
    const groups = new Set<string>();
    workspace.addChangeListener((event) => {
      if (event.isUiEvent) {
        return;
      }
      groups.add(event.group);
    });
    Blockly.Events.setGroup('outer');
    try {
      insertDeclaration(workspace, { kind: 'before', block: get(workspace, 'print') }, 'score');
    } finally {
      Blockly.Events.setGroup(false);
    }
    await events();
    expect([...groups]).toEqual(['outer']);
  });
});

describe('the Make a variable dialog', () => {
  it('is an accessible, labelled dialog that checks the name as it is typed', async () => {
    const queue = createDialogQueue();
    render(<DialogHost queue={queue} />);
    const workspace = canvas();
    let result!: ReturnType<typeof makeVariable>;
    await act(async () => {
      result = makeVariable(context(workspace, { dialogs: queue }));
      await Promise.resolve();
    });

    const dialog = screen.getByRole('dialog', { name: MAKE_VARIABLE_TITLE });
    const input = screen.getByRole<HTMLInputElement>('textbox', { name: MAKE_VARIABLE_QUESTION });
    expect(input.value).toBe('value');
    expect(document.activeElement).toBe(input);
    await expectNoAxeViolations(dialog);

    fireEvent.change(input, { target: { value: 'guess' } });
    expect(screen.getByRole('alert').textContent).toMatch(/already something called “guess”/);
    expect(input.getAttribute('aria-invalid')).toBe('true');
    await expectNoAxeViolations(dialog);

    fireEvent.change(input, { target: { value: 'score' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    const block = await created(result);
    expect(declOf(block)?.name).toBe('score');
  });
});
