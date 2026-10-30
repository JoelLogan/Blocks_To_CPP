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
import { toolboxDefinition } from './contents';
import { B2cBlockInflater } from './inflater';
import { B2C_BLOCK_KIND } from './presets';
import { registerToolboxComponents, toolboxInjectOptions } from './register';
import type { SymbolSource } from './scope';

/**
 * The test timeout of the test files whose tests show the continuous toolbox's flyout and whose
 * slowest test takes more than 2.5 s in the suite under coverage with two workers
 * (`vi.setConfig({ testTimeout: FLYOUT_TEST_TIMEOUT_MS })` after their imports; Vitest's default
 * is 5 s, and CI's runners take up to 1.7 times as long as a developer's machine).
 *
 * Showing the flyout again builds only the blocks whose items changed and the blocks that declare
 * something (./inflater.ts), so this no longer grows with the number of times a test shows it:
 * what it bounds is the first showing at injection, which draws every block of the catalog (62 in
 * W2's second round, about 1.5 s in happy-dom), and the declaring blocks of each showing. W2's
 * package T took the slowest test of the 14 files that set it from 2.9-15.3 s to 2.2-12.4 s (on
 * one machine, base and head runs of those files alternately, and the whole suite). The slowest
 * left are dominated by the injection, or turn the keeping of blocks off on purpose; three times
 * the slowest would exceed this limit, so it stays. The files whose slowest test fell to 2.5 s or
 * less (`toolbox/continuous`, `toolbox/frozen-prototype`) stopped setting it, and set it again
 * once W2's structs, enums, control and file blocks (73 toolbox blocks) took their slowest to 5.4 s
 * and 3.9 s. Every category W2 fills adds blocks to the injection, so remeasure after each one:
 * with W2's package C2's (global, flip, field defaults) `toolbox/core`'s slowest took 2.8 s, so it
 * sets it too, and the slowest of all, `toolbox/reuse`, 14.1 s, still below this limit. Package S's
 * six (use header, check at compile time, and the file blocks open, read line, write and exists)
 * took it to 16.2 s, and no file showing the flyout that leaves this limit unset went over 2.5 s.
 * After M3's W4 (the gated blocks leave the Program flyout; toolbox search; Quick Insert) and W5
 * (the kept toolbox blocks), in the whole suite under coverage with two workers (2026-10-30):
 * `toolbox/reuse` 12.4 s, `keyboard/mover` 14.4 s, `toolbox/plugin` 6.3 s, Quick Insert's 4.8 s
 * and the quick fixes' 2.2 s (so that file no longer sets it); none reached this limit.
 */
export const FLYOUT_TEST_TIMEOUT_MS = 20_000;

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

/** Runs the update (and so the analysis the scope query answers from) and stores its preview. */
export function analyse(core: CoreWasm, document: BdmDocument): void {
  const preview = core.update(JSON.stringify(document), { indentWidth: 4 });
  useAppStore.setState({
    project: projectFixture({ document }),
    analysis: { seq: 1, notice: null, syncCodes: [], preview },
  });
}

/**
 * Fake analysis symbols: `scopes` maps `blockId` (or `blockId/input`) to the symbols visible there,
 * and a block that is not in it has no answer (the analysis has not seen it); `all` is every symbol
 * of the program.
 */
export function fakeSymbols(
  scopes: Readonly<Record<string, readonly SymbolInfo[]>> = {},
  all: readonly SymbolInfo[] = [],
): SymbolSource {
  return {
    symbolsAt: (blockId, input) => scopes[input === null ? blockId : `${blockId}/${input}`] ?? null,
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
 * to the symbols visible there, and a block that is not in it has no answer (`null`, the analysis
 * has not seen it). Everything else throws.
 */
export function fakeCore(scopes: Readonly<Record<string, readonly SymbolInfo[]>>): CoreWasm {
  const unused = (): never => {
    throw new Error('not used by the toolbox');
  };
  return {
    version: unused,
    load: unused,
    canonical: unused,
    update: unused,
    conversionTable: unused,
    clipboardMake: unused,
    pastePrepare: unused,
    symbolsInScope: (blockId, input) => {
      const found = scopes[input === null ? blockId : `${blockId}/${input}`];
      return found === undefined ? null : [...found];
    },
  };
}

/** Puts a project and an analysis with `symbols` in the app store. */
export function storeAnalysis(document: BdmDocument, symbols: readonly SymbolInfo[]): void {
  useAppStore.setState({
    project: projectFixture({ document }),
    analysis: {
      seq: 1,
      notice: null,
      syncCodes: [],
      preview: {
        stage: 'generate',
        diagnostics: [],
        files: [],
        sourceMap: null,
        buildable: true,
        placeholders: 0,
        contentHash: 'a'.repeat(64),
        blockTypes: {},
        slotTypes: {},
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

/**
 * The inflater of the toolbox blocks of `workspace`'s flyout (its `stats()` count the blocks it
 * built, kept and disposed of), or `null` before the flyout has shown one.
 */
export function flyoutInflater(workspace: Blockly.WorkspaceSvg): B2cBlockInflater | null {
  const flyout = workspace.getFlyout();
  if (flyout === null) {
    return null;
  }
  // The flyout's inflaters are protected: read the map the way a subclass would.
  const inflater = (flyout as unknown as { inflaters: Map<string, unknown> }).inflaters.get(
    B2C_BLOCK_KIND,
  );
  return inflater instanceof B2cBlockInflater ? inflater : null;
}

/** Whether a flyout item is a block with a declaration field (`symbol_decl`). */
function declaresSomething(item: Blockly.utils.toolbox.ToolboxItemInfo): boolean {
  const type = (item as { type?: unknown }).type;
  return (
    typeof type === 'string' &&
    (catalogBlock(type)?.fields.some((field) => field.kind === 'symbol_decl') ?? false)
  );
}

/**
 * The full toolbox ({@link toolboxDefinition}) with every category whose contents are fixed shown
 * `times` times, the copies named `Logic 2`, `Logic 3` and so on, for tests of how the cost of
 * showing the flyout grows with the number of blocks. The copies leave out the blocks that declare
 * something (a block with a declaration field), which every showing builds again
 * (./inflater.ts), so a showing of the larger toolbox builds as many blocks as one of the full
 * toolbox.
 */
export function paddedToolbox(times: number): Blockly.utils.toolbox.ToolboxInfo {
  const definition = toolboxDefinition();
  const categories = definition.contents;
  const copies = [];
  for (let copy = 2; copy <= times; copy += 1) {
    for (const category of categories) {
      if ('contents' in category && 'name' in category) {
        const contents = category.contents.filter((item) => !declaresSomething(item));
        copies.push({ ...category, name: `${category.name} ${String(copy)}`, contents });
      }
    }
  }
  return { ...definition, contents: [...categories, ...copies] };
}
