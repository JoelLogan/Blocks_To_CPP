/**
 * The collection blocks in the real webview (docs/plans/m3-contracts/w2.md §1.2 and §5.2, package
 * A):
 *
 * - **Menus.** The list menu of *add … to* offers only the variables that hold a collection it can
 *   add to and may change: the list `scores`, not the whole number `n` or the constant list
 *   `fixed`.
 * - **A place-typed block.** *list of*, taken from the Collections category towards
 *   `create [int] n`'s value, does not snap: `n` keeps its `0` and the list lies loose beside it.
 *   Taken towards `create [list of int] scores`' value, it snaps, and one Ctrl+Z takes it out
 *   again.
 *
 * Every drag is made with real pointer input, every key pressed as a person presses it; what the
 * canvas holds is read from the test hook's document.
 */
import { describe, expect, it } from 'vitest';

import { type BlockNode, moduleNodes, NodeIds, num, type ProjectDocument } from '../../support/bdm';
import { currentDocument, fieldOf, foldCodePanel, waitForDelivery } from '../../support/editor';
import { clickMiddle } from '../../support/ui';
import { waitFor } from '../../support/wait';
import { closeMenu, dragValueFromToolbox, menuLabels } from './lib/editor';
import { startFlow } from './lib/launch';
import { declare, newEmptyProject } from './lib/project';
import { pressCtrl, UI_TIMEOUT_MS } from './lib/ui';

/** The *list of* toolbox entry. */
const LIST_OF = { category: 'Collections', type: 'coll.list_of' } as const;

/** A list of whole numbers, as a type field holds it (docs/plans/m3-contracts/types.md §2.1). */
const LIST_OF_INT = { 'std::vector': 'int' } as const;

/** A block of the canvas by ID, or `undefined`. */
function nodeOf(doc: ProjectDocument, id: string): BlockNode | undefined {
  return moduleNodes(doc).find((node) => node.id === id);
}

/** The top-level blocks of the first module. */
function topBlocks(doc: ProjectDocument): readonly BlockNode[] {
  return doc.modules[0]?.workspace.blocks ?? [];
}

/** A `create` block of `type` without a value: a collection starts empty. */
function createEmpty(
  ids: NodeIds,
  sym: string,
  name: string,
  type: unknown,
  isConst = false,
): BlockNode {
  return {
    id: ids.next(),
    type: 'var.declare',
    v: 1,
    fields: { CONST: isConst, NAME: { sym, name }, TYPE: type },
  };
}

describe('Collections', () => {
  it('lists only changeable collections in add’s menu, and snaps list of only where a list goes', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);
    const ids = new NodeIds('coll');
    const n = declare(ids, { sym: 's_coll_n', name: 'n', type: 'int', value: num(0) });
    const scores = createEmpty(ids, 's_coll_scores', 'scores', LIST_OF_INT);
    const fixed = createEmpty(ids, 's_coll_fixed', 'fixed', LIST_OF_INT, true);
    const add: BlockNode = { id: ids.next(), type: 'coll.add', v: 1, inputs: { VALUE: num(1) } };
    await app.hook.insertBlocks(mainId, 'BODY', [n, scores, fixed, add]);
    await waitForDelivery(app.driver);

    // add … to: only the list that may change.
    await waitFor(
      async () => {
        await clickMiddle(app.driver, await fieldOf(app, add.id, 'LIST'));
        const labels = await menuLabels(app.driver);
        if (labels.includes('scores')) {
          return labels;
        }
        // The menu was opened before the preview knew the variables: try again.
        await closeMenu(app.driver);
        return null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the list menu of add … to with the variables' },
    ).then((labels) => {
      expect(labels).toEqual(['scores']);
    });
    await closeMenu(app.driver);

    // list of towards create [int]'s value: refused, so n keeps its 0 and the list lies loose.
    await dragValueFromToolbox(app, LIST_OF, n.id, 'VALUE');
    await waitFor(
      async () =>
        topBlocks(await currentDocument(app)).some((block) => block.type === LIST_OF.type),
      { timeout: UI_TIMEOUT_MS, message: 'the refused list of loose on the canvas' },
    );
    expect(nodeOf(await currentDocument(app), n.id)?.inputs?.['VALUE']).toEqual(num(0));

    // list of towards create [list of int]'s value: it snaps.
    await dragValueFromToolbox(app, LIST_OF, scores.id, 'VALUE');
    const taken = await waitFor(
      async () => {
        const value = nodeOf(await currentDocument(app), scores.id)?.inputs?.['VALUE'];
        return typeof value === 'object' && 'block' in value ? value.block : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'list of in create [list of int]' },
    );
    expect(taken.type).toBe(LIST_OF.type);

    // One Ctrl+Z takes it out again.
    await waitForDelivery(app.driver);
    await pressCtrl(app.driver, 'z');
    await waitFor(
      async () =>
        nodeOf(await currentDocument(app), scores.id)?.inputs?.['VALUE'] === undefined
          ? true
          : null,
      { timeout: UI_TIMEOUT_MS, message: 'Ctrl+Z to take list of out of create [list of int]' },
    );
    expect(nodeOf(await currentDocument(app), n.id)?.inputs?.['VALUE']).toEqual(num(0));
  }, 180_000);
});
