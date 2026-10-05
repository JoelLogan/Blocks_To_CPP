/**
 * Test support for the toolbox tests (not used by the app): block registration, injected
 * workspaces, a small stand-in for the document sync's loader, a fake compiler core, and the real
 * compiler core when it has been built.
 */
import {
  initCore,
  type BdmBlock,
  type BdmDocument,
  type CoreWasm,
  type SymbolInfo,
} from '@blocks2cpp/b2c-core-wasm';
import {
  b2cLightTheme,
  catalogBlock,
  exprShadowState,
  installIdGenerator,
  registerB2cBlocks,
  registerB2cMutators,
} from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import type { EditorContext } from '../../app/editor-types';
import { useAppStore } from '../../app/store';
import { documentFixture, projectFixture } from '../../app/testing/fixtures';
import { registerToolboxComponents, toolboxInjectOptions } from './register';
import type { SymbolSource } from './scope';

/** Registers the ID generator, the mutators, every block type and the toolbox's classes. */
export function setUpToolboxBlocks(): void {
  installIdGenerator();
  registerB2cMutators();
  registerB2cBlocks();
  registerToolboxComponents();
}

const workspaces: Blockly.Workspace[] = [];

/** Disposes every workspace made by the helpers here, and clears what Blockly left behind. */
export function disposeTestWorkspaces(): void {
  for (const workspace of workspaces.splice(0)) {
    workspace.dispose();
  }
  Blockly.WidgetDiv.hide();
  Blockly.DropDownDiv.hideWithoutAnimation();
  document.body.replaceChildren();
}

/** A headless workspace. */
export function headlessWorkspace(): Blockly.Workspace {
  const workspace = new Blockly.Workspace();
  workspaces.push(workspace);
  return workspace;
}

/**
 * A rendered workspace with the Blocks2Cpp toolbox: the continuous toolbox (the app's choice) or
 * Blockly's category toolbox.
 */
export function injectedWorkspace(
  toolbox: 'continuous' | 'category' = 'continuous',
): Blockly.WorkspaceSvg {
  const host = document.createElement('div');
  document.body.append(host);
  const options = toolboxInjectOptions();
  const workspace = Blockly.inject(host, {
    renderer: 'zelos',
    theme: b2cLightTheme,
    sounds: false,
    toolbox: options.toolbox,
    ...(toolbox === 'continuous' ? { plugins: { ...options.plugins } } : {}),
  });
  workspaces.push(workspace);
  return workspace;
}

/** The type class of a value input (a repeated input by its numbered name). */
function inputCheck(type: string, name: string) {
  const inputs = catalogBlock(type)?.inputs ?? [];
  const input =
    inputs.find((candidate) => candidate.name === name) ??
    inputs.find(
      (candidate) =>
        candidate.repeat !== null &&
        name.startsWith(candidate.name) &&
        /^\d+$/.test(name.slice(candidate.name.length)),
    );
  return input?.check ?? 'any';
}

/** The Blockly state of a list of statements: the first, with the others chained as `next`. */
function chainState(list: readonly BdmBlock[]): Blockly.serialization.blocks.State | null {
  let next: Blockly.serialization.blocks.State | null = null;
  for (const node of [...list].reverse()) {
    const state = blockStateOf(node);
    if (next !== null) {
      state.next = { block: next };
    }
    next = state;
  }
  return next;
}

/** The Blockly state of one project block (a stand-in for the document sync's loader). */
function blockStateOf(node: BdmBlock): Blockly.serialization.blocks.State {
  const state: Blockly.serialization.blocks.State = { type: node.type, id: node.id };
  if (node.x !== undefined && node.y !== undefined) {
    state.x = node.x;
    state.y = node.y;
  }
  if (node.disabled === true) {
    state.disabledReasons = [Blockly.constants.MANUALLY_DISABLED];
  }
  if (node.extra !== undefined) {
    state.extraState = node.extra;
  }
  if (node.fields !== undefined) {
    state.fields = node.fields;
  }
  const inputs: Record<string, Blockly.serialization.blocks.ConnectionState> = {};
  for (const [name, value] of Object.entries(node.inputs ?? {})) {
    inputs[name] =
      'block' in value
        ? { block: blockStateOf(value.block) }
        : {
            shadow: exprShadowState(
              value.expr,
              value.draft === true,
              inputCheck(node.type, name),
              false,
            ),
          };
  }
  for (const [name, list] of Object.entries(node.statements ?? {})) {
    const first = chainState(list);
    if (first !== null) {
      inputs[name] = { block: first };
    }
  }
  if (Object.keys(inputs).length > 0) {
    state.inputs = inputs;
  }
  return state;
}

/** Puts a module of a loaded document on a workspace, with the project's block IDs. */
export function loadModule(workspace: Blockly.Workspace, document: BdmDocument, index = 0): void {
  const module = document.modules[index];
  if (module === undefined) {
    throw new Error(`The document has no module ${String(index)}.`);
  }
  Blockly.Events.disable();
  try {
    for (const node of module.workspace.blocks) {
      const state = blockStateOf(node);
      if (node.stack !== undefined) {
        const stack = chainState(node.stack);
        if (stack !== null) {
          state.next = { block: stack };
        }
      }
      Blockly.serialization.blocks.append(state, workspace);
    }
  } finally {
    Blockly.Events.enable();
  }
}

/**
 * A document whose `main` (ID `main`) holds `create int guess = 0` (ID `decl`, symbol `s_guess`) and
 * then `print "hi"` (ID `print`), next to `define factorial (n)` (ID `fn`, symbols `s_fact`, `s_n`).
 */
export function guessDocument(): BdmDocument {
  const document = documentFixture();
  const main = document.modules[0];
  if (main !== undefined) {
    main.workspace.blocks = [
      {
        id: 'main',
        type: 'program.main',
        v: 1,
        x: 40,
        y: 40,
        statements: {
          BODY: [
            {
              id: 'decl',
              type: 'var.declare',
              v: 1,
              fields: { TYPE: 'int', NAME: { sym: 's_guess', name: 'guess' }, CONST: false },
              inputs: { VALUE: { expr: [{ num: '0' }] } },
            },
            {
              id: 'print',
              type: 'io.print',
              v: 1,
              extra: { itemCount: 1 },
              inputs: { ITEM0: { expr: [{ str: 'hi' }] } },
            },
          ],
        },
      },
      {
        id: 'fn',
        type: 'func.define',
        v: 1,
        x: 400,
        y: 40,
        extra: { params: [{ sym: 's_n', name: 'n', type: 'int', mode: 'copy' }] },
        fields: { NAME: { sym: 's_fact', name: 'factorial' }, RETURNS: 'int' },
      },
    ];
  }
  return document;
}

/** Whether the tests must run the real core (CI sets `B2C_REQUIRE_WASM`). */
export const CORE_REQUIRED =
  ((globalThis as { process?: { env?: Record<string, string | undefined> } }).process?.env?.[
    'B2C_REQUIRE_WASM'
  ] ?? '') !== '';

/**
 * The real compiler core, from its embedded build (`pnpm --filter @blocks2cpp/b2c-core-wasm
 * build`), or `null` when it has not been built and the tests may skip it.
 */
export async function realCoreOrSkip(): Promise<CoreWasm | null> {
  try {
    return await initCore();
  } catch (error: unknown) {
    if (CORE_REQUIRED) {
      throw error;
    }
    return null;
  }
}

/** Loads project text with the compiler core (the only way documents enter the editor). */
export function loadDocument(core: CoreWasm, text: string): BdmDocument {
  const loaded = core.load(new TextEncoder().encode(text));
  if (!loaded.ok) {
    throw new Error(`The project does not load: ${JSON.stringify(loaded.diagnostics)}`);
  }
  return loaded.document;
}

/** Runs the preview (and so the analysis the scope query answers from) and stores it. */
export function analyse(core: CoreWasm, document: BdmDocument): void {
  const preview = core.preview(JSON.stringify(document), { indentWidth: 4 });
  useAppStore.setState({
    project: projectFixture({ document }),
    analysis: { seq: 1, notice: null, preview },
  });
}

/**
 * Fake analysis symbols: `scopes` maps `blockId` (or `blockId/input`) to the symbols visible there,
 * `all` is every symbol of the program.
 */
export function fakeSymbols(
  scopes: Readonly<Record<string, readonly SymbolInfo[]>> = {},
  all: readonly SymbolInfo[] = [],
): SymbolSource {
  return {
    symbolsAt: (blockId, input) => scopes[input === null ? blockId : `${blockId}/${input}`] ?? [],
    allSymbols: () => all,
  };
}

/**
 * A symbol for fake scope answers: a non-constant `int` variable unless `overrides` say otherwise
 * (for example `{kind: 'function', params: [], returns: 'void'}`).
 */
export function symbolFixture(
  id: string,
  name: string,
  overrides: Partial<Record<string, unknown>> = {},
): SymbolInfo {
  return {
    id,
    name,
    kind: 'variable',
    isConst: false,
    type: 'int',
    module: 'mod_main',
    declBlock: 'blk_decl',
    ...overrides,
  };
}

/**
 * A compiler core that answers only the scope query: `scopes` maps `blockId` (or `blockId/input`)
 * to the symbols visible there. Everything else throws.
 */
export function fakeCore(scopes: Readonly<Record<string, readonly SymbolInfo[]>>): CoreWasm {
  const unused = (): never => {
    throw new Error('not used by the toolbox');
  };
  return {
    version: unused,
    load: unused,
    canonical: unused,
    preview: unused,
    conversionTable: unused,
    clipboardMake: unused,
    pastePrepare: unused,
    symbolsInScope: (blockId, input) => [
      ...(scopes[input === null ? blockId : `${blockId}/${input}`] ?? []),
    ],
  };
}

/** Puts a project and an analysis with `symbols` in the app store. */
export function storeAnalysis(document: BdmDocument, symbols: readonly SymbolInfo[]): void {
  useAppStore.setState({
    project: projectFixture({ document }),
    analysis: {
      seq: 1,
      notice: null,
      preview: {
        stage: 'generate',
        diagnostics: [],
        files: [],
        sourceMap: null,
        buildable: true,
        placeholders: 0,
        contentHash: 'a'.repeat(64),
        blockTypes: {},
        symbols: [...symbols],
      },
    },
  });
}

/** The editor context the toolbox plugin gets. */
export function editorContext(workspace: Blockly.WorkspaceSvg, core: CoreWasm): EditorContext {
  return {
    workspace,
    store: useAppStore,
    core: () => core,
    selectBlock: () => undefined,
    activeModuleId: () => useAppStore.getState().project?.activeModuleId ?? 'mod_main',
  };
}
