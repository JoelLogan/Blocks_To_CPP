// The toolbox with the real compiler core (packages/b2c-core-wasm, built with
// `pnpm --filter @blocks2cpp/b2c-core-wasm build`): the analysis's symbols decide what the
// Variables category and Your functions list, and the scope query what a block sees where it is
// dropped. Skipped without a build unless B2C_REQUIRE_WASM is set (as in CI).
import type { BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { B2cSymbolDeclField } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import exitCodeText from '../../../../../examples/exit_code.b2c?raw';
import factorialText from '../../../../../examples/factorial.b2c?raw';
import guessingGameText from '../../../../../examples/guessing_game.b2c?raw';
import { setCore } from '../../app/core';
import type { ChoiceOptions, DialogService } from '../../app/dialogs';
import { resetAppStore, useAppStore } from '../../app/store';
import {
  MAKE_VARIABLE_BUTTON,
  functionsContents,
  variablesContents,
  type ContentsContext,
} from './contents';
import { symbolsAtBlock, symbolsAtConnection } from './scope';
import { CONTINUOUS_REFRESH_DELAY_MS } from './continuous';
import { argLabelField } from './inflater';
import type { MakeVariableResult } from './makeVariable';
import { analysisSymbolSource, createToolboxPlugin } from './plugin';
import { B2C_BLOCK_KIND, type B2cBlockInfo } from './presets';
import {
  FLYOUT_TEST_TIMEOUT_MS,
  analyse,
  disposeTestWorkspaces,
  editorContext,
  injectedWorkspace,
  loadDocument,
  loadModule,
  realCoreOrSkip,
  setUpToolboxBlocks,
} from './testing';

// Each test here shows the flyout of every catalog block (FLYOUT_TEST_TIMEOUT_MS): with W2's
// package C2's blocks its slowest took 2.8 s in the suite under coverage with two workers.
vi.setConfig({ testTimeout: FLYOUT_TEST_TIMEOUT_MS });

const core: CoreWasm | null = await realCoreOrSkip();

beforeAll(() => {
  setUpToolboxBlocks();
});

beforeEach(() => {
  resetAppStore();
});

const detachers: (() => void)[] = [];

afterEach(() => {
  for (const detach of detachers.splice(0)) {
    detach();
  }
  setCore(null);
  disposeTestWorkspaces();
  resetAppStore();
});

/** The core, which the suite only runs with. */
function theCore(): CoreWasm {
  if (core === null) {
    throw new Error('The compiler core is not built.');
  }
  return core;
}

/** Waits for Blockly's events, the plugin's refresh and the continuous toolbox's rebuild. */
function settle(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, CONTINUOUS_REFRESH_DELAY_MS + 50));
}

/** Loads and analyses a project, and puts it on a canvas with the toolbox plugin attached. */
function open(
  text: string,
  options: Parameters<typeof createToolboxPlugin>[0] = {},
  edit?: (document: BdmDocument) => void,
): Blockly.WorkspaceSvg {
  const document = loadDocument(theCore(), text);
  edit?.(document);
  const workspace = injectedWorkspace('continuous');
  loadModule(workspace, document);
  analyse(theCore(), document);
  detachers.push(createToolboxPlugin(options).attach(editorContext(workspace, theCore())));
  return workspace;
}

function present<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) {
    throw new Error('missing');
  }
  return value;
}

/** The blocks the flyout shows, in order. */
function flyoutBlocks(workspace: Blockly.WorkspaceSvg): Blockly.BlockSvg[] {
  const flyout = workspace.getToolbox()?.getFlyout();
  return (flyout?.getContents() ?? []).flatMap((item) => {
    const element = item.getElement();
    return element instanceof Blockly.BlockSvg ? [element] : [];
  });
}

/** For each variable block of the flyout, its type and the symbol it is preset to. */
function variableBlocks(workspace: Blockly.WorkspaceSvg): [string, unknown][] {
  return flyoutBlocks(workspace)
    .filter((block) => ['var.get', 'var.set', 'var.change', 'var.update'].includes(block.type))
    .map((block) => [block.type, block.getFieldValue('VAR')]);
}

/** Selects a canvas block the way Blockly 12 does: by focusing it. */
function select(workspace: Blockly.WorkspaceSvg, id: string): void {
  const block = workspace.getBlockById(id);
  if (!(block instanceof Blockly.BlockSvg)) {
    throw new Error(`no block ${id}`);
  }
  Blockly.common.setSelected(block);
}

/** The context the contents functions read, on a headless canvas with `text`. */
function headlessContext(text: string): ContentsContext {
  const document = loadDocument(theCore(), text);
  const workspace = new Blockly.Workspace();
  loadModule(workspace, document);
  analyse(theCore(), document);
  return {
    workspace,
    symbols: analysisSymbolSource(useAppStore, () => theCore()),
    modules: () => document.modules.map(({ id, name }) => ({ id, name })),
    activeModuleId: () => document.modules[0]?.id ?? '',
    programModuleId: () => document.modules[0]?.id ?? null,
  };
}

function blocksOf(items: readonly Blockly.utils.toolbox.FlyoutItemInfo[]): B2cBlockInfo[] {
  return items.filter((item): item is B2cBlockInfo => item.kind === B2C_BLOCK_KIND);
}

// Each test opens a whole project in a real workspace and runs the compiler core on it, which takes
// seconds under coverage on a CI runner.
describe.skipIf(core === null)('the toolbox with the compiler core', { timeout: 20_000 }, () => {
  it('lists every variable of main, whatever is selected, with one set and one change', async () => {
    const workspace = open(guessingGameText);
    await settle();
    const listed = [
      ['var.set', 's_guess'],
      ['var.change', 's_guess'],
      ['var.get', 's_guess'],
      ['var.get', 's_secret'],
    ];
    expect(variableBlocks(workspace)).toEqual(listed);
    // b002 (create secret) sees only secret, b005 (the ask) both: the toolbox stays as it is.
    const show = vi.spyOn(present(workspace.getToolbox()?.getFlyout()), 'show');
    for (const id of ['b002', 'b005', 'b011']) {
      select(workspace, id);
      await settle();
    }
    expect(show).not.toHaveBeenCalled();
    expect(variableBlocks(workspace)).toEqual(listed);
  });

  it('presets set and change to a variable that may be changed', async () => {
    const workspace = open(guessingGameText, {}, (document) => {
      const main = document.modules[0]?.workspace.blocks.find((block) => block.id === 'b011');
      const guess = main?.statements?.['BODY']?.find((block) => block.id === 'b003');
      if (guess?.fields !== undefined) {
        guess.fields['CONST'] = true;
      }
    });
    await settle();
    expect(variableBlocks(workspace)).toEqual([
      ['var.set', 's_secret'],
      ['var.change', 's_secret'],
      ['var.get', 's_guess'],
      ['var.get', 's_secret'],
    ]);
  });

  it('groups the variables by the function that declares them', () => {
    const items = blocksOf(variablesContents(headlessContext(factorialText)));
    expect(
      items
        .filter((item) => item.type === 'var.get')
        .map((item) => item.fields?.['VAR'] as unknown),
    ).toEqual([{ ref: 's_i' }, { ref: 's_n' }]);
  });

  it('gives a block dropped loose on the canvas main’s variables, from the core', async () => {
    const document = loadDocument(theCore(), guessingGameText);
    const workspace = open(guessingGameText);
    await settle();
    const symbols = analysisSymbolSource(useAppStore, () => theCore());
    const loose = workspace.newBlock('var.set');
    loose.initSvg();
    loose.render();
    // Not analysed yet: what its place gives it, the end of main.
    expect(theCore().symbolsInScope(loose.id, null)).toBeNull();
    expect(symbolsAtConnection(workspace, loose, symbols).map((symbol) => symbol.name)).toEqual([
      'guess',
      'secret',
    ]);
    // Once analysed, the loose block is parked: the core itself answers with the end of main.
    document.modules[0]?.workspace.blocks.push({
      id: loose.id,
      type: 'var.set',
      v: 1,
      x: 600,
      y: 40,
    });
    analyse(theCore(), document);
    expect(symbolsAtBlock(workspace, loose, null, symbols).map((symbol) => symbol.name)).toEqual([
      'guess',
      'secret',
    ]);
  });

  it('gives what a disabled loop sees, and what the statement before sees, to blocks without an answer', async () => {
    const loop = (document: BdmDocument) =>
      document.modules[0]?.workspace.blocks
        .find((block) => block.id === 'b011')
        ?.statements?.['BODY']?.find((block) => block.id === 'b010');
    const workspace = open(guessingGameText, {}, (document) => {
      const found = loop(document);
      if (found !== undefined) {
        found.disabled = true;
      }
    });
    await settle();
    const symbols = analysisSymbolSource(useAppStore, () => theCore());
    const names = (block: Blockly.Block) =>
      symbolsAtBlock(workspace, block, null, symbols).map((symbol) => symbol.name);
    // The ask block (b005) is inside the disabled loop (b010): the core answers like the loop.
    expect(names(present(workspace.getBlockById('b005')))).toEqual(['guess', 'secret']);
    // A block the analysis has not seen yet, just after `create int guess` (b003).
    const fresh = workspace.newBlock('io.print');
    fresh.initSvg();
    fresh.render();
    present(workspace.getBlockById('b003')).nextConnection.connect(
      present(fresh.previousConnection),
    );
    expect(theCore().symbolsInScope(fresh.id, null)).toBeNull();
    expect(names(fresh)).toEqual(['guess', 'secret']);
    // Just after `create int secret` (b002), guess is not visible yet.
    const early = workspace.newBlock('io.print');
    early.initSvg();
    early.render();
    present(workspace.getBlockById('b002')).nextConnection.connect(
      present(early.previousConnection),
    );
    expect(names(early)).toEqual(['secret']);
  });

  it('shows factorial under Your functions as a reporter with its argument labelled', async () => {
    const items = blocksOf(functionsContents(headlessContext(factorialText)));
    const call = items.find((item) => item.type === 'func.call');
    expect(call?.fields).toEqual({ FUNC: { ref: 's_factorial' } });
    expect(call?.extraState).toEqual({ argCount: 1 });
    expect(call?.argLabels).toEqual(['n:']);
    expect(items.some((item) => item.type === 'func.call_stmt')).toBe(false);

    const workspace = open(factorialText);
    await settle();
    const block = flyoutBlocks(workspace).find((candidate) => candidate.type === 'func.call');
    expect(block?.outputConnection).not.toBeNull();
    expect(block?.previousConnection).toBeNull();
    expect(block?.getField(argLabelField(0))?.getText()).toBe('n:');
  });

  it('shows the void function of exit_code.b2c as a statement', async () => {
    const items = blocksOf(functionsContents(headlessContext(exitCodeText)));
    const call = items.find((item) => item.type === 'func.call_stmt');
    expect(call?.fields).toEqual({ FUNC: { ref: 's_stop' } });
    expect(call?.extraState).toEqual({ argCount: 0 });
    expect(items.some((item) => item.type === 'func.call')).toBe(false);

    const workspace = open(exitCodeText);
    await settle();
    const block = flyoutBlocks(workspace).find((candidate) => candidate.type === 'func.call_stmt');
    expect(block?.getFieldValue('FUNC')).toBe('s_stop');
    expect(block?.previousConnection).not.toBeNull();
    expect(block?.outputConnection).toBeNull();
  });

  it('makes a variable before the selected statement, with a name free there', async () => {
    const results: MakeVariableResult[] = [];
    const prompt = vi.fn<DialogService['prompt']>(() => Promise.resolve('tries'));
    const spy = vi.fn((options: ChoiceOptions<string>) =>
      Promise.resolve(options.choices[0]?.id ?? options.cancel),
    );
    // A spy's call signature is not generic, so it is cast to the service's.
    const choose = spy as typeof spy & DialogService['choose'];
    const workspace = open(guessingGameText, {
      dialogs: { prompt, choose, alert: vi.fn(() => Promise.resolve()) },
      onMakeVariable: (result) => results.push(result),
    });
    await settle();
    select(workspace, 'b005');
    await settle();
    workspace.getButtonCallback(MAKE_VARIABLE_BUTTON)?.({} as Blockly.FlyoutButton);
    await settle();
    const options = prompt.mock.calls[0]?.[0];
    expect(options?.defaultValue).toBe('value');
    expect(options?.validate?.('guess')).toMatch(/already something called/);
    expect(options?.validate?.('secret')).toMatch(/already something called/);
    expect(results[0]?.status).toBe('created');
    const inserted = workspace.getBlockById('b005')?.getPreviousBlock();
    expect(inserted?.type).toBe('var.declare');
    expect((inserted?.getField('NAME') as B2cSymbolDeclField).getDecl()?.name).toBe('tries');
  });
});
