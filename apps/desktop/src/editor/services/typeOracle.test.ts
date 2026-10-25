/**
 * The type oracle's scope answers (docs/plans/m3-contracts/slots.md §4.3, *Speed*), with the real
 * compiler core: a variable created above a block is visible there without a scope query, every
 * other case asks the core, the shortcut never answers differently from the query, and each query's
 * answer is kept until the next preview.
 */
import type { BdmBlock, PreviewResult } from '@blocks2cpp/b2c-core-wasm';
import type * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it } from 'vitest';

import { documentFixture } from '../../app/testing/fixtures';
import { loadModule } from '../sync/bdmToWorkspace';
import { disposeWorkspaces, headlessWorkspace, testCore } from '../sync/testing';
import { symbolsAtBlock } from '../toolbox/scope';
import { analysisScope, createTypeOracle } from './symbols';

const core = await testCore();

afterEach(() => {
  disposeWorkspaces();
});

function create(id: string, name: string, value: string): BdmBlock {
  return {
    id,
    type: 'var.declare',
    v: 1,
    fields: { TYPE: 'int', NAME: { sym: `s_${name}_${id}`, name }, CONST: false },
    inputs: { VALUE: { expr: [{ num: value }] } },
  };
}

function set(id: string, sym: string): BdmBlock {
  return {
    id,
    type: 'var.set',
    v: 1,
    fields: { VAR: { ref: sym } },
    inputs: { VALUE: { expr: [{ num: '1' }] } },
  };
}

/**
 * `main`: create x (`c1`), set x (`s1`), if { create y (`c2`), set y (`s2`), create x again
 * (`c3`, hiding the first x), set x (`s3`) }, set x (`s4`), set y (`s5`, out of scope); and a
 * loose `set x` (`s6`).
 */
function program(): ReturnType<typeof documentFixture> {
  const document = documentFixture();
  const body = [
    create('c1', 'x', '0'),
    set('s1', 's_x_c1'),
    {
      id: 'if1',
      type: 'control.if',
      v: 1,
      extra: { elseIfCount: 0, hasElse: false },
      inputs: { COND0: { expr: [{ kw: 'true' }] } },
      statements: {
        DO0: [
          create('c2', 'y', '0'),
          set('s2', 's_y_c2'),
          create('c3', 'x', '1'),
          set('s3', 's_x_c1'),
        ],
      },
    },
    set('s4', 's_x_c1'),
    set('s5', 's_y_c2'),
  ];
  const module = document.modules[0];
  if (module === undefined) {
    throw new Error('no module');
  }
  module.workspace.blocks = [
    { id: 'main', type: 'program.main', v: 1, x: 40, y: 40, statements: { BODY: body } },
    { ...set('s6', 's_x_c1'), x: 600, y: 40 },
  ];
  return document;
}

describe.skipIf(core === null)('the type oracle’s scope answers', () => {
  function setUp(): {
    workspace: Blockly.Workspace;
    oracle: ReturnType<typeof createTypeOracle>;
    queries: () => number;
    preview: { current: PreviewResult };
  } {
    const wasm = core;
    if (wasm === null) {
      throw new Error('no core');
    }
    const document = program();
    const workspace = headlessWorkspace();
    loadModule(workspace, document, document.modules[0]?.id ?? '');
    const preview = { current: wasm.update(JSON.stringify(document), { indentWidth: 4 }) };
    let queries = 0;
    const counted = new Proxy(wasm, {
      get(target, property, receiver) {
        if (property === 'symbolsInScope') {
          return (blockId: string, input: string | null) => {
            queries += 1;
            return target.symbolsInScope(blockId, input);
          };
        }
        const value: unknown = Reflect.get(target, property, receiver);
        return typeof value === 'function' ? (value.bind(target) as unknown) : value;
      },
    });
    const oracle = createTypeOracle({
      preview: () => preview.current,
      core: () => counted,
      workspace,
    });
    return { workspace, oracle, queries: () => queries, preview };
  }

  function at(workspace: Blockly.Workspace, id: string): Blockly.Block {
    const found = workspace.getBlockById(id);
    if (found === null) {
      throw new Error(`no block ${id}`);
    }
    return found;
  }

  it('answers a variable created above without a scope query', () => {
    const { workspace, oracle, queries } = setUp();
    expect(oracle.visibleAt?.('s_x_c1', at(workspace, 's1'), null)).toBe(true);
    expect(oracle.visibleAt?.('s_x_c1', at(workspace, 's4'), null)).toBe(true);
    expect(oracle.visibleAt?.('s_y_c2', at(workspace, 's2'), null)).toBe(true);
    expect(queries()).toBe(0);
  });

  it('asks the core where the canvas does not settle it, and agrees with it everywhere', () => {
    const { workspace, oracle, queries, preview } = setUp();
    // Hidden by the second x, out of scope after the `if`, on a loose stack, in its own value.
    expect(oracle.visibleAt?.('s_x_c1', at(workspace, 's3'), null)).toBe(false);
    expect(oracle.visibleAt?.('s_y_c2', at(workspace, 's5'), null)).toBe(false);
    expect(oracle.visibleAt?.('s_x_c1', at(workspace, 'c1'), 'VALUE')).toBe(false);
    expect(queries()).toBeGreaterThan(0);
    const scope = analysisScope({ core: () => core, preview: () => preview.current });
    const symbols = ['s_x_c1', 's_y_c2', 's_x_c3'];
    for (const block of workspace.getAllBlocks(false)) {
      for (const input of [null, 'VALUE']) {
        if (input !== null && block.getInput(input) === null) {
          continue;
        }
        const visible = symbolsAtBlock(workspace, block, input, scope).map((symbol) => symbol.id);
        for (const symbol of symbols) {
          expect(oracle.visibleAt?.(symbol, block, input), `${symbol} at ${block.id}`).toBe(
            visible.includes(symbol),
          );
        }
      }
    }
  });

  it('keeps each query’s answer until the next preview', () => {
    const { workspace, oracle, queries, preview } = setUp();
    const late = at(workspace, 's5');
    oracle.visibleAt?.('s_y_c2', late, null);
    oracle.visibleAt?.('s_x_c1', late, null);
    expect(queries()).toBe(1);
    preview.current = { ...preview.current };
    oracle.visibleAt?.('s_y_c2', late, null);
    expect(queries()).toBe(2);
  });
});
