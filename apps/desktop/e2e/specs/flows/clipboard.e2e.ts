/**
 * Copy, cut and paste in the block editor (docs/spec/05-project-format.md §5.12, 04 §4.7) with the
 * keyboard, in the real webview (WebKitGTK on Linux, WebView2 on Windows): a block is clicked,
 * Ctrl+C and Ctrl+V put a copy after it with a new block ID (and, for a declaration, a new symbol
 * ID), and Ctrl+X takes a block away until Ctrl+V puts it back.
 */
import { describe, expect, it } from 'vitest';

import { type BlockNode, declaredSymbol, NodeIds, print } from '../../support/bdm';
import { foldCodePanel } from '../../support/editor';
import { waitFor } from '../../support/wait';
import { clickBlock } from './lib/editor';
import { type FlowApp, startFlow } from './lib/launch';
import { declare, mainBody, newEmptyProject } from './lib/project';
import { pressCtrl, UI_TIMEOUT_MS } from './lib/ui';

const TEXT = 'B2C-COPY-ME';

/** Waits until main's body has `count` blocks, and returns them. */
function waitForBody(app: FlowApp, count: number): Promise<readonly BlockNode[]> {
  return waitFor(
    async () => {
      const body = await mainBody(app);
      return body.length === count ? body : null;
    },
    {
      timeout: UI_TIMEOUT_MS,
      message: async () =>
        `${String(count)} blocks in main (it has ${(await mainBody(app)).map((node) => node.type).join(', ')})`,
    },
  );
}

/** The text item of a print block. */
function printedText(node: BlockNode | undefined): unknown {
  const item = node?.inputs?.['ITEM0'];
  return item !== undefined && 'expr' in item ? item.expr : null;
}

describe('Clipboard', () => {
  it('copies, pastes and cuts blocks with the keyboard', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);
    const ids = new NodeIds('clip');
    const decl = declare(ids, {
      sym: 'sym_e2e_x',
      name: 'x',
      type: 'int',
      value: { expr: [{ num: '5' }] },
    });
    const original = print(ids, TEXT);
    await app.hook.insertBlocks(mainId, 'BODY', [decl, original]);

    // (1) Copy the print and paste: the copy goes after it, with a new ID and the same text.
    await clickBlock(app, original.id);
    await pressCtrl(app.driver, 'c');
    await pressCtrl(app.driver, 'v');
    const afterPaste = await waitForBody(app, 3);
    const copy = afterPaste[2];
    expect(afterPaste.map((node) => node.id).slice(0, 2)).toEqual([decl.id, original.id]);
    expect(copy?.type).toBe('io.print');
    expect(copy?.id).not.toBe(original.id);
    expect(printedText(copy)).toEqual([{ str: TEXT }]);

    // (2) Copy the declaration and paste: a new block that declares a new symbol.
    await clickBlock(app, decl.id);
    await pressCtrl(app.driver, 'c');
    await pressCtrl(app.driver, 'v');
    const afterSecond = await waitForBody(app, 4);
    const pastedDecl = afterSecond[1];
    expect(pastedDecl?.type).toBe('var.declare');
    expect(pastedDecl?.id).not.toBe(decl.id);
    const symbol = pastedDecl === undefined ? null : declaredSymbol(pastedDecl, 'NAME');
    expect(symbol).not.toBeNull();
    expect(symbol?.sym).not.toBe('sym_e2e_x');

    // (3) Cut the pasted print, then paste it back after the original print.
    const copyId = copy?.id ?? '';
    await clickBlock(app, copyId);
    await pressCtrl(app.driver, 'x');
    const afterCut = await waitForBody(app, 3);
    expect(afterCut.map((node) => node.id)).not.toContain(copyId);
    await clickBlock(app, original.id);
    await pressCtrl(app.driver, 'v');
    const afterCutPaste = await waitForBody(app, 4);
    const back = afterCutPaste[afterCutPaste.findIndex((node) => node.id === original.id) + 1];
    expect(back?.type).toBe('io.print');
    expect(printedText(back)).toEqual([{ str: TEXT }]);
  }, 180_000);
});
