// The toolbox with the real compiler core (packages/b2c-core-wasm, built with
// `pnpm --filter @blocks2cpp/b2c-core-wasm build`): the scope query decides what the Variables
// category lists, and the analysis's symbols what My Blocks lists. Skipped without a build unless
// B2C_REQUIRE_WASM is set (as in CI).
import type { BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { B2cSymbolDeclField } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import exitCodeText from '../../../../../examples/exit_code.b2c?raw';
import factorialText from '../../../../../examples/factorial.b2c?raw';
import guessingGameText from '../../../../../examples/guessing_game.b2c?raw';
import { setCore } from '../../app/core';
import type { DialogService } from '../../app/dialogs';
import { resetAppStore, useAppStore } from '../../app/store';
import {
  MAKE_VARIABLE_BUTTON,
  functionsContents,
  variablesContents,
  type ContentsContext,
} from './contents';
import { CONTINUOUS_REFRESH_DELAY_MS } from './continuous';
import { argLabelField } from './inflater';
import type { MakeVariableResult } from './makeVariable';
import { analysisSymbolSource, createToolboxPlugin } from './plugin';
import { B2C_BLOCK_KIND, type B2cBlockInfo } from './presets';
import {
  analyse,
  disposeTestWorkspaces,
  editorContext,
  injectedWorkspace,
  loadDocument,
  loadModule,
  realCoreOrSkip,
  setUpToolboxBlocks,
} from './testing';

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
    selected: () => null,
    modules: () => document.modules.map(({ id, name }) => ({ id, name })),
  };
}

function blocksOf(items: readonly Blockly.utils.toolbox.FlyoutItemInfo[]): B2cBlockInfo[] {
  return items.filter((item): item is B2cBlockInfo => item.kind === B2C_BLOCK_KIND);
}

// Each test opens a whole project in a real workspace and runs the compiler core on it, which takes
// seconds under coverage on a CI runner.
describe.skipIf(core === null)('the toolbox with the compiler core', { timeout: 20_000 }, () => {
  it('lists guess below “create int guess”, and not above it', async () => {
    const workspace = open(guessingGameText);
    await settle();
    // b004 (print) is below both declarations; b002 (create secret) is above create guess.
    select(workspace, 'b004');
    await settle();
    expect(variableBlocks(workspace)).toEqual([
      ['var.get', 's_guess'],
      ['var.set', 's_guess'],
      ['var.change', 's_guess'],
      ['var.update', 's_guess'],
      ['var.get', 's_secret'],
      ['var.set', 's_secret'],
      ['var.change', 's_secret'],
      ['var.update', 's_secret'],
    ]);

    select(workspace, 'b002');
    await settle();
    expect(variableBlocks(workspace).map(([, ref]) => ref)).not.toContain('s_guess');
    expect(variableBlocks(workspace)).toEqual([]);

    // Inside the loop, both are visible.
    select(workspace, 'b005');
    await settle();
    expect(variableBlocks(workspace).filter(([type]) => type === 'var.get')).toEqual([
      ['var.get', 's_guess'],
      ['var.get', 's_secret'],
    ]);
  });

  it('lists what is visible at the end of main when nothing is selected', async () => {
    const workspace = open(guessingGameText);
    await settle();
    expect(
      variableBlocks(workspace)
        .filter(([type]) => type === 'var.get')
        .map(([, ref]) => ref),
    ).toEqual(['s_guess', 's_secret']);
  });

  it('leaves constants out of set, change and update', async () => {
    const workspace = open(guessingGameText, {}, (document) => {
      const main = document.modules[0]?.workspace.blocks.find((block) => block.id === 'b011');
      const secret = main?.statements?.['BODY']?.find((block) => block.id === 'b002');
      if (secret?.fields !== undefined) {
        secret.fields['CONST'] = true;
      }
    });
    await settle();
    select(workspace, 'b004');
    await settle();
    expect(variableBlocks(workspace).filter(([, ref]) => ref === 's_secret')).toEqual([
      ['var.get', 's_secret'],
    ]);
    expect(variableBlocks(workspace).filter(([, ref]) => ref === 's_guess')).toHaveLength(4);
  });

  it('shows factorial in My Blocks as a reporter with its argument labelled', async () => {
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

  it('lists the variables of the scope query at a counted loop', () => {
    const context = headlessContext(factorialText);
    // Inside the loop's print (b006), the counter i is visible.
    const inLoop = blocksOf(
      variablesContents({ ...context, selected: () => context.workspace.getBlockById('b006') }),
    );
    expect(
      inLoop
        .filter((item) => item.type === 'var.get')
        .map((item) => item.fields?.['VAR'] as unknown),
    ).toEqual([{ ref: 's_i' }]);
  });

  it('makes a variable before the selected statement, with a name free there', async () => {
    const results: MakeVariableResult[] = [];
    const prompt = vi.fn<DialogService['prompt']>(() => Promise.resolve('tries'));
    const workspace = open(guessingGameText, {
      dialogs: { prompt, alert: vi.fn(() => Promise.resolve()) },
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
