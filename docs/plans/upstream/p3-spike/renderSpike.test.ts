/**
 * P-3 spike (kept as evidence for M3-2, not part of the app's suite; ./README.md says how to run
 * it): what Blockly's render management does after an edit at the end of a long statement list,
 * with and without the patch (../blockly-render-management.bundle.patch); and that the patched
 * render leaves every block exactly where a full render puts it. The render count fails without
 * the patch when B2C_SPIKE_PATCHED is set (503 blocks drawn against at most 4).
 */
import type { BdmBlock, BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import * as Blockly from 'blockly/core';
import { appendFileSync } from 'node:fs';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { generateDocument } from '../../../e2e/bench/document';
import { documentFixture } from '../../app/testing/fixtures';
import { insertBlocks } from '../../e2e/blocks';
import { loadModule } from './bdmToWorkspace';
import {
  disposeWorkspaces,
  loadText,
  present,
  renderedWorkspace,
  seededRandom,
  testCore,
} from './testing';
import { allBlocks } from './traverse';

const core = await testCore();
const PATCHED = String(process.env['B2C_SPIKE_PATCHED'] ?? '') !== '';
const OUT = String(process.env['B2C_SPIKE_OUT'] ?? '/dev/null');

function report(line: string): void {
  appendFileSync(OUT, `${PATCHED ? 'patched' : 'stock'} ${line}\n`);
}

afterEach(() => {
  disposeWorkspaces();
});

let printSerial = 0;

/** A print of `text`, as the edit benchmark inserts it. */
function print(text: string): BdmBlock {
  printSerial += 1;
  return {
    id: `x${String(printSerial)}`,
    type: 'io.print',
    v: 1,
    extra: { itemCount: 1 },
    fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
    inputs: { ITEM0: { expr: [{ str: text }] } },
  };
}

/** A document whose main holds `count` prints. */
function chainDocument(count: number): BdmDocument {
  const doc = documentFixture('Chain');
  const body = Array.from({ length: count }, (_unused, index) => print(`line ${String(index)}`));
  doc.modules = [
    {
      id: 'mod_main',
      name: 'main',
      workspace: { blocks: [{ id: 'main', type: 'program.main', v: 1, x: 0, y: 0, statements: { BODY: body } }] },
    },
  ];
  return doc;
}

async function settle(): Promise<void> {
  await Blockly.renderManagement.finishQueuedRenders();
  // The loader turns connection tracking on after 1 ms.
  await new Promise((resolve) => setTimeout(resolve, 5));
  await Blockly.renderManagement.finishQueuedRenders();
}

async function shown(doc: BdmDocument, renderer = 'zelos'): Promise<Blockly.WorkspaceSvg> {
  const workspace = renderedWorkspace({ renderer });
  loadModule(workspace, loadText(present(core, 'core'), JSON.stringify(doc)), doc.modules[0]?.id ?? '');
  await settle();
  return workspace;
}

interface Counts {
  renders: number;
  componentUpdates: number;
  connectionMoves: number;
  ms: number;
}

/** Runs `edit`, then the render it queues, and counts what Blockly did in that render. */
async function counted(edit: () => void): Promise<Counts> {
  const renders = vi.spyOn(Blockly.BlockSvg.prototype, 'renderEfficiently');
  const updates = vi.spyOn(Blockly.BlockSvg.prototype, 'updateComponentLocations');
  const moves = vi.spyOn(Blockly.RenderedConnection.prototype, 'moveTo');
  edit();
  renders.mockClear();
  updates.mockClear();
  moves.mockClear();
  const start = performance.now();
  Blockly.renderManagement.triggerQueuedRenders();
  const ms = performance.now() - start;
  const counts = {
    renders: renders.mock.calls.length,
    componentUpdates: updates.mock.calls.length,
    connectionMoves: moves.mock.calls.length,
    ms: Math.round(ms * 10) / 10,
  };
  renders.mockRestore();
  updates.mockRestore();
  moves.mockRestore();
  await settle();
  return counts;
}

/** The last statement of main's BODY. */
function lastStatement(workspace: Blockly.WorkspaceSvg, mainId: string): Blockly.BlockSvg {
  let block = present(workspace.getBlockById(mainId), 'main').getInputTargetBlock('BODY');
  while (block?.getNextBlock()) {
    block = block.getNextBlock();
  }
  return present(block as Blockly.BlockSvg | null, 'last statement');
}

/** Where everything is: each block's place, size and outline, and each connection's place. */
function snapshot(workspace: Blockly.WorkspaceSvg): string[] {
  const lines: string[] = [];
  for (const block of allBlocks(workspace) as Blockly.BlockSvg[]) {
    const xy = block.getRelativeToSurfaceXY();
    lines.push(
      `${block.id} at ${String(xy.x)},${String(xy.y)} size ${String(block.width)}x${String(block.height)} ` +
        `path ${block.pathObject.svgPath.getAttribute('d') ?? ''}`,
    );
    for (const connection of block.getConnections_(true)) {
      lines.push(
        `  ${block.id} conn ${String(connection.type)} at ${String(connection.x)},${String(connection.y)}`,
      );
    }
  }
  // The connection databases: every tracked connection, filed under its current y.
  workspace.connectionDBList.forEach((db, type) => {
    const connections = (db as unknown as { connections: Blockly.RenderedConnection[] }).connections;
    const filed = connections.map(
      (connection) =>
        `db ${String(type)} ${connection.getSourceBlock().id} ${String(connection.x)},${String(connection.y)}`,
    );
    lines.push(...filed.sort());
    // Sorted by y, as the database's binary search needs.
    for (let index = 1; index < connections.length; index += 1) {
      if ((connections[index - 1]?.y ?? 0) > (connections[index]?.y ?? 0)) {
        lines.push(`db ${String(type)} out of order at ${String(index)}`);
      }
    }
  });
  return lines;
}

/** Renders every block again, as `WorkspaceSvg.render` does: the reference for `snapshot`. */
function fullRender(workspace: Blockly.WorkspaceSvg): void {
  workspace.render();
}

describe.skipIf(core === null)(`render management (${PATCHED ? 'patched' : 'stock'})`, () => {
  it('counts the renders when a print is appended to a 500-statement chain', async () => {
    const n = Number(process.env['B2C_SPIKE_N'] ?? '500');
    const t0 = performance.now();
    const workspace = await shown(chainDocument(n));
    report(`loaded ${String(n)} in ${String(Math.round(performance.now() - t0))} ms`);
    const append = await counted(() => {
      insertBlocks(workspace, 'main', 'BODY', [print('appended')]);
    });
    const last = lastStatement(workspace, 'main');
    const remove = await counted(() => {
      last.dispose(true);
    });
    report(`chain-500 append ${JSON.stringify(append)} delete ${JSON.stringify(remove)}`);
    if (PATCHED) {
      expect(append.renders).toBeLessThanOrEqual(4);
    } else {
      expect(append.renders).toBeGreaterThan(n);
    }
  }, 900_000);

  it.skip('counts the renders when a print is appended to the 1,000-block benchmark main', async () => {
    const doc = generateDocument({ blocks: 1000, dragHandle: false }) as unknown as BdmDocument;
    const workspace = await shown(doc);
    const mainId = 'b0';
    const append = await counted(() => {
      insertBlocks(workspace, mainId, 'BODY', [print('appended')]);
    });
    const last = lastStatement(workspace, mainId);
    const remove = await counted(() => {
      last.dispose(true);
    });
    report(`bench-1000 append ${JSON.stringify(append)} delete ${JSON.stringify(remove)}`);
  });

  for (const renderer of ['zelos', 'geras']) {
    it(`places every block as a full render does after random edits (${renderer})`, async () => {
      const doc = generateDocument({ blocks: 120, dragHandle: false }) as unknown as BdmDocument;
      const workspace = await shown(doc, renderer);
      const random = seededRandom(renderer === 'zelos' ? 3 : 7);
      const pick = <T>(items: readonly T[]): T | undefined => items[Math.floor(random() * items.length)];
      const statements = (): Blockly.BlockSvg[] =>
        (allBlocks(workspace) as Blockly.BlockSvg[]).filter(
          (block) => !block.isShadow() && block.previousConnection !== null && !block.isDeadOrDying(),
        );
      const mouths = (): Blockly.Connection[] =>
        (allBlocks(workspace) as Blockly.BlockSvg[]).flatMap((block) =>
          block.inputList
            .filter((input) => input.type === Blockly.inputs.inputTypes.STATEMENT)
            .map((input) => present(input.connection, 'mouth')),
        );
      const fresh = (): Blockly.BlockSvg => {
        const node = print(`edit ${String(printSerial)}`);
        const block = Blockly.serialization.blocks.append(
          {
            type: 'io.print',
            id: node.id,
            extraState: { itemCount: 1 },
            fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
          },
          workspace,
        ) as Blockly.BlockSvg;
        return block;
      };
      const edits: Record<string, () => void> = {
        append: () => {
          insertBlocks(workspace, 'b0', 'BODY', [print('appended')]);
        },
        insertAfter: () => {
          const target = pick(statements());
          target?.nextConnection?.connect(present(fresh().previousConnection, 'prev'));
        },
        insertIntoMouth: () => {
          const mouth = pick(mouths());
          mouth?.connect(present(fresh().previousConnection, 'prev'));
        },
        deleteHealing: () => {
          pick(statements())?.dispose(true);
        },
        moveStack: () => {
          const target = pick(statements());
          if (target === undefined) {
            return;
          }
          target.unplug(false);
          const mouth = pick(
            mouths().filter((candidate) => candidate.getSourceBlock().getRootBlock() !== target),
          );
          mouth?.connect(present(target.previousConnection, 'prev'));
        },
        collapse: () => {
          const target = pick(
            statements().filter((block) =>
              block.inputList.some((input) => input.type === Blockly.inputs.inputTypes.STATEMENT),
            ),
          );
          target?.setCollapsed(!target.isCollapsed());
        },
      };
      const names = Object.keys(edits);
      for (let step = 0; step < 40; step += 1) {
        const name = present(pick(names), 'edit');
        present(edits[name], name)();
        await settle();
        const after = snapshot(workspace);
        fullRender(workspace);
        await settle();
        const reference = snapshot(workspace);
        expect(after, `step ${String(step)}: ${name}`).toEqual(reference);
      }
    }, 120_000);
  }

  it('times getDescendants on long headless chains', () => {
    Blockly.Blocks['p3_statement'] = {
      init(this: Blockly.Block) {
        this.setPreviousStatement(true);
        this.setNextStatement(true);
      },
    };
    for (const length of [1_000, 5_000, 10_000, 20_000]) {
      const workspace = new Blockly.Workspace();
      Blockly.Events.disable();
      try {
        const first = workspace.newBlock('p3_statement');
        let last = first;
        for (let index = 1; index < length; index += 1) {
          const block = workspace.newBlock('p3_statement');
          present(last.nextConnection, 'next').connect(present(block.previousConnection, 'previous'));
          last = block;
        }
        const start = performance.now();
        let outcome: string;
        try {
          const rounds = 5;
          for (let round = 0; round < rounds; round += 1) {
            first.getDescendants(false);
          }
          outcome = `${((performance.now() - start) / rounds).toFixed(2)} ms per call`;
        } catch (error) {
          outcome = `throws ${String(error)} after ${(performance.now() - start).toFixed(0)} ms`;
        }
        report(`getDescendants headless chain of ${String(length)}: ${outcome}`);
      } finally {
        // Disposed while events are off: a delete event serialises the chain recursively.
        try {
          workspace.dispose();
        } catch (error) {
          report(`dispose of a headless chain of ${String(length)} throws ${String(error)}`);
        }
        Blockly.Events.enable();
      }
    }
  }, 600_000);

  it('lists descendants in the recursive order, and fast', async () => {
    const workspace = await shown(chainDocument(150));
    const main = present(workspace.getBlockById('main'), 'main');
    const recursive = (block: Blockly.Block, ordered: boolean): Blockly.Block[] => [
      block,
      ...block.getChildren(ordered).flatMap((child) => recursive(child, ordered)),
    ];
    expect(main.getDescendants(false)).toEqual(recursive(main, false));
    expect(main.getDescendants(true)).toEqual(recursive(main, true));
    const start = performance.now();
    for (let round = 0; round < 20; round += 1) {
      main.getDescendants(false);
    }
    const perCall = (performance.now() - start) / 20;
    report(`getDescendants 150-statement chain (301 blocks): ${perCall.toFixed(2)} ms per call`);
  }, 120_000);
});
