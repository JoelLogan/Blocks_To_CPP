/** Shared helpers for the blockly-ext tests: registration, workspaces and fake services. */
import * as Blockly from 'blockly/core';
import { afterEach } from 'vitest';

import {
  b2cLightTheme,
  installIdGenerator,
  registerB2cBlocks,
  resetEditorServices,
  setEditorServices,
  DEFAULT_EDITOR_SERVICES,
  type SymbolInfo,
} from '../src';
import { registerStubMutators } from './stub-mutators';

/** Installs the ID generator, the (stub) mutators and every block type. */
export function setUpBlocks(): void {
  installIdGenerator();
  registerStubMutators();
  registerB2cBlocks();
}

const workspaces: Blockly.Workspace[] = [];

afterEach(() => {
  for (const workspace of workspaces.splice(0)) {
    workspace.dispose();
  }
  Blockly.WidgetDiv.hide();
  Blockly.DropDownDiv.hideWithoutAnimation();
  document.body.replaceChildren();
  resetEditorServices();
});

/** A headless workspace, disposed after the test. */
export function headlessWorkspace(): Blockly.Workspace {
  const workspace = new Blockly.Workspace();
  workspaces.push(workspace);
  return workspace;
}

/** A workspace injected into the document with Zelos and the light theme, disposed after the test. */
export function renderedWorkspace(): Blockly.WorkspaceSvg {
  const host = document.createElement('div');
  document.body.append(host);
  const workspace = Blockly.inject(host, {
    renderer: 'zelos',
    theme: b2cLightTheme,
    sounds: false,
  });
  workspaces.push(workspace);
  return workspace;
}

/** A rendered block of a type, initialised and drawn. */
export function renderedBlock(workspace: Blockly.WorkspaceSvg, type: string): Blockly.BlockSvg {
  const block = workspace.newBlock(type);
  block.initSvg();
  block.render();
  return block;
}

/** A symbol for fake scope answers. */
export function symbol(
  id: string,
  name: string,
  kind: SymbolInfo['kind'] = 'variable',
  extra: Partial<SymbolInfo> = {},
): SymbolInfo {
  return { id, name, kind, type: 'int', module: 'mod_main', declBlock: 'blk_decl', ...extra };
}

/**
 * Installs fake symbol services: `scopes` maps a block ID (and `blockId/input` for an input) to the
 * symbols in scope there; `names` maps symbol IDs to current names.
 */
export function fakeSymbols(
  scopes: Record<string, readonly SymbolInfo[]>,
  names: Record<string, string>,
): string[] {
  const queries: string[] = [];
  setEditorServices({
    ...DEFAULT_EDITOR_SERVICES,
    symbols: {
      symbolsAt: (blockId, input) => {
        const key = input === null ? blockId : `${blockId}/${input}`;
        queries.push(key);
        return scopes[key] ?? [];
      },
      nameOf: (symId) => names[symId] ?? null,
    },
  });
  return queries;
}
