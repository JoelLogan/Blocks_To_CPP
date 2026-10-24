/**
 * Wrapping statements in an empty C-block with real pointer drags (docs/spec/03-block-language.md
 * §3.3 "Placing blocks"; the wrap cases of the placement flows, W5-2 and W5-3 of the M2 polish
 * plan), in the real webview:
 *
 * - an empty *if* taken from the toolbox, with its top on the gap above the second of `main`'s
 *   three *print* blocks: the drag preview stands in the second's place with the second and the
 *   third in its mouth; moved away, no preview; back and dropped there, `main` is
 *   `[a, if{b, c}]`. Ctrl+Z takes the whole drop back (the *if* is gone, `main` exactly as it
 *   was) and Ctrl+Y wraps again;
 * - an *if* dropped on the top of a loop's body wraps the loop's statements (a nested mouth);
 * - at the zooms 0.5, 0.9 and 1.5 (projects saved with that zoom), an empty *if* taken from the
 *   toolbox, brought in from the right at the height of the gap above the third of `main`'s five
 *   prints and dropped with its top there, exactly and 10 canvas units off in each direction,
 *   wraps the third print and everything below it, the preview before the release showed what
 *   the drop did, and Ctrl+Z puts everything back. It comes from the right because a preview
 *   stays on a place the pointer passed while that place is in reach and no other place is 20
 *   units nearer (03 §3.3), and a statement preview moves the statements below it down: coming
 *   down the list from above, the *if* keeps wrapping the second print at the gap below it (the
 *   sweep counts such drops as swaps).
 *
 * With `B2C_E2E_WRAP_SWEEP=1` the last test instead drops the *if* on a 20-unit grid over ±60
 * units around the gap, each point twice (the pointer coming from above and from below), and
 * reports the counts of the W5-2 spike per zoom: drops whose preview differed from what they did
 * (it fails on any), points where the two approaches did different things, insertions, and drops
 * that left the *if* loose and took two Ctrl+Z to take back (Blockly bumped it). They are
 * printed as one JSON line and written to `wrap-sweep.json` in the test's artifact folder
 * (`B2C_E2E_ARTIFACTS`). On Windows the same run is the E2E job (WebView2 through `msedgedriver`)
 * with that variable set.
 */
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

import { Button, By, Origin, type WebDriver } from 'selenium-webdriver';
import { describe, expect, it } from 'vitest';

import type { E2ePoint } from '../../../src/e2e/contract';
import { artifactDir, writeArtifact } from '../../support/artifacts';
import {
  type BlockNode,
  moduleNodes,
  NodeIds,
  onlyTopBlock,
  print,
  type ProjectDocument,
  statementsOf,
} from '../../support/bdm';
import { bringIntoView, scrollIntoView } from '../../support/canvas';
import { releaseButton } from '../../support/drag';
import {
  centerOn,
  currentDocument,
  flyoutGrabPoint,
  foldCodePanel,
  waitForDelivery,
} from '../../support/editor';
import { harnessSettings } from '../../support/env';
import { clickTestId, openCategory } from '../../support/ui';
import { sleep, waitFor } from '../../support/wait';
import { type FlowApp, startFlow } from './lib/launch';
import {
  FIXTURES,
  forever,
  mainBody,
  moduleBlocks,
  newEmptyProject,
  openFromStartPage,
} from './lib/project';
import { pressCtrl, UI_TIMEOUT_MS } from './lib/ui';

/** How long Blockly takes to finish a drop (snapping, rendering) in the next frames. */
const DROP_SETTLE_MS = 300;

/** How many moves the pointer makes to each point of a drag, and how long each takes. */
const STEPS = 8;
const STEP_MS = 20;

/**
 * The window of the zoom test (CSS pixels): the Windows runner's size, which the window cannot
 * exceed there. With the docks folded away, the drops around the gap stay on the canvas at the
 * zoom 1.5.
 */
const WINDOW = { width: 1028, height: 749 } as const;

/** The five prints of `main` in the zoom projects. */
const PRINT_IDS = ['e2e_wrap_p1', 'e2e_wrap_p2', 'e2e_wrap_p3', 'e2e_wrap_p4', 'e2e_wrap_p5'];

/**
 * How far right of the gap (CSS pixels) the pointer brings a new *if* out of the toolbox before
 * its drop is aimed: on the prints, more than a snap radius right of their connections.
 */
const LEAVE_TOOLBOX_PX = 200;

/** A point with whole pixels (WebDriver's pointer coordinates are integers). */
function whole(point: E2ePoint): { x: number; y: number } {
  return { x: Math.round(point.x), y: Math.round(point.y) };
}

/**
 * Moves the pointer from `from` through `path` in steps; with `press`, presses the left button at
 * `from` first. The button stays down (WebDriver keeps it pressed across actions) until
 * `releaseButton`.
 */
async function follow(
  driver: WebDriver,
  from: E2ePoint,
  path: readonly E2ePoint[],
  press: boolean,
): Promise<void> {
  let at = whole(from);
  let actions = driver.actions({ async: true }).move({ ...at, origin: Origin.VIEWPORT });
  if (press) {
    actions = actions.press(Button.LEFT).pause(100);
  }
  for (const point of path) {
    const to = whole(point);
    for (let step = 1; step <= STEPS; step += 1) {
      actions = actions.move({
        x: Math.round(at.x + ((to.x - at.x) * step) / STEPS),
        y: Math.round(at.y + ((to.y - at.y) * step) / STEPS),
        origin: Origin.VIEWPORT,
        duration: STEP_MS,
      });
    }
    at = to;
  }
  await actions.pause(100).perform();
}

/**
 * Runs in the webview: what the drag preview shows, from the editor's DOM. `none` without a
 * preview; `wrap <id>` when the preview (Blockly's insertion marker) holds the first of the listed
 * blocks that is inside it; `insert` when it holds none of them.
 */
const PREVIEW_SCRIPT = `
  const hook = window.__B2C_E2E__;
  const marker = document.querySelector(
    '[data-testid="workspace"] .blocklyBlockCanvas .blocklyInsertionMarker',
  );
  if (marker === null) {
    return 'none';
  }
  for (const id of arguments[0]) {
    const element = hook.blockElement(id);
    if (element !== null && marker.contains(element)) {
      return 'wrap ' + id;
    }
  }
  return 'insert';
`;

/** Runs in the webview: the ID of the block being dragged, or `null`. */
const DRAGGED_SCRIPT = `
  const dragged = document.querySelector('[data-testid="workspace"] .blocklyDragging[data-id]');
  return dragged === null ? null : dragged.getAttribute('data-id');
`;

/** What the drag preview shows now (see {@link PREVIEW_SCRIPT}). */
function previewOf(app: FlowApp, ids: readonly string[]): Promise<string> {
  return app.driver.executeScript<string>(PREVIEW_SCRIPT, ids);
}

/** Runs in the webview: the canvas's zoom, from its block layer's `transform`, or `null`. */
const CANVAS_SCALE_SCRIPT = `
  const layer = document.querySelector(
    '[data-testid="workspace"] svg.blocklySvg > g.blocklyWorkspace > g.blocklyBlockCanvas',
  );
  const match = /scale\\(([0-9.e+-]+)\\)/.exec(layer === null ? '' : layer.getAttribute('transform') || '');
  return match === null ? null : Number(match[1]);
`;

/**
 * What a drop did with the *if* `ifId`, as {@link PREVIEW_SCRIPT} names a preview: `none` when it
 * is not in `main`'s body, `wrap <id>` when its first mouth starts with `<id>`, `insert` when the
 * mouth is empty.
 */
function resultOf(doc: ProjectDocument, ifId: string): string {
  const node = statementsOf(onlyTopBlock(doc, 'program.main'), 'BODY').find(
    (block) => block.id === ifId,
  );
  if (node === undefined) {
    return 'none';
  }
  const first = statementsOf(node, 'DO0')[0];
  return first === undefined ? 'insert' : `wrap ${first.id}`;
}

/** A statement list as text: IDs, with an *if*'s first mouth or a loop's body in braces. */
function listOf(nodes: readonly BlockNode[]): string {
  return nodes
    .map((node) => {
      const mouth =
        node.type === 'control.if' ? 'DO0' : node.type === 'control.forever' ? 'BODY' : null;
      return mouth === null ? node.id : `${node.id}{${listOf(statementsOf(node, mouth))}}`;
    })
    .join(' ');
}

/** The blocks of the open project as text, to compare. */
async function blocksText(app: FlowApp): Promise<string> {
  return JSON.stringify(moduleBlocks(await currentDocument(app)));
}

/** A connection's point on screen, once the block is shown. */
function connectionPoint(app: FlowApp, blockId: string, connection: string): Promise<E2ePoint> {
  return waitFor(() => app.hook.connectionPoint(blockId, connection), {
    timeout: UI_TIMEOUT_MS,
    message: `the ${connection} connection of block ${blockId}`,
  });
}

/**
 * Opens the Control category and returns its *if*, with the pointer position that brings the
 * *if*'s top (`dx`, `dy` pixels off) onto connection `connection` of block `blockId` when it is
 * pressed at `grab`.
 */
async function toolboxIfOnto(
  app: FlowApp,
  blockId: string,
  connection: string,
): Promise<{ grab: E2ePoint; drop: E2ePoint }> {
  await openCategory(app.driver, 'Control');
  const toolboxIf = await waitFor(() => app.hook.flyoutBlockId('control.if', {}), {
    timeout: UI_TIMEOUT_MS,
    message: 'an if in the Control category',
  });
  const grab = await flyoutGrabPoint(app, toolboxIf);
  const own = await connectionPoint(app, toolboxIf, 'previous');
  const target = await connectionPoint(app, blockId, connection);
  return { grab, drop: { x: target.x + (grab.x - own.x) + 3, y: target.y + (grab.y - own.y) + 3 } };
}

/**
 * Presses Ctrl+Z until the project's blocks are `blocks` again, at most `steps` times (a press
 * that has changed nothing after 2 s is taken as done), and returns how many it took. The first
 * press comes once Blockly has delivered the drop: Undo does not have it before that.
 */
async function undoDrop(
  app: FlowApp,
  blocks: string,
  facts: string,
  steps: number,
): Promise<number> {
  await waitForDelivery(app.driver);
  for (let step = 1; ; step += 1) {
    await pressCtrl(app.driver, 'z');
    if (step >= steps) {
      await waitForBlocks(app, blocks, `Ctrl+Z (${String(step)}) after the drop at ${facts}`);
      return step;
    }
    const back = await waitFor(async () => ((await blocksText(app)) === blocks ? true : null), {
      timeout: 2_000,
      message: 'the blocks to be back',
    }).catch(() => false);
    if (back) {
      return step;
    }
  }
}

/** Waits until Ctrl+Z (or anything) has brought the project's blocks back to `blocks`. */
function waitForBlocks(app: FlowApp, blocks: string, message: string): Promise<true> {
  return waitFor(async () => ((await blocksText(app)) === blocks ? true : null), {
    timeout: UI_TIMEOUT_MS,
    message,
  });
}

describe('Wrapping statements in an empty C-block', () => {
  it('wraps the statement an if from the toolbox is dropped on, in one undo step', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);
    const ids = new NodeIds('wrap');
    const [a, b, c] = [print(ids, 'a'), print(ids, 'b'), print(ids, 'c')];
    await app.hook.insertBlocks(mainId, 'BODY', [a, b, c]);
    const initial = await blocksText(app);

    await centerOn(app, b.id);
    await bringIntoView(app.driver, () => connectionPoint(app, a.id, 'next'));
    const { grab, drop } = await toolboxIfOnto(app, a.id, 'next');
    // Away from every connection (they are at the stack's left edge) and inside the window at
    // every size the flows run at: to the right at the drop's height, clear of the trash can.
    const width = await app.driver.executeScript<number>('return window.innerWidth;');
    const away = { x: Math.min(drop.x + 300, width - 80), y: drop.y };
    expect(away.x - drop.x, 'the move away, beyond the snap radius').toBeGreaterThan(150);
    const prints = [a.id, b.id, c.id];

    await follow(app.driver, grab, [drop], true);
    expect(await previewOf(app, prints)).toBe(`wrap ${b.id}`);
    await follow(app.driver, drop, [away], false);
    expect(await previewOf(app, prints)).toBe('none');
    await follow(app.driver, away, [drop], false);
    expect(await previewOf(app, prints)).toBe(`wrap ${b.id}`);
    await releaseButton(app.driver);
    await sleep(DROP_SETTLE_MS);

    const wrapper = await waitFor(
      async () => {
        const body = await mainBody(app);
        const added = body[1];
        return body.length === 2 && added?.type === 'control.if' ? added : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'main to be [a, if]' },
    );
    const wrapped = `${a.id} ${wrapper.id}{${b.id} ${c.id}}`;
    expect(listOf(await mainBody(app))).toBe(wrapped);

    // One Ctrl+Z takes the whole drop back, once Blockly has delivered it; Ctrl+Y wraps again.
    await waitForDelivery(app.driver);
    await pressCtrl(app.driver, 'z');
    await waitForBlocks(app, initial, 'Ctrl+Z to put main back exactly');
    expect(moduleNodes(await currentDocument(app)).some((node) => node.id === wrapper.id)).toBe(
      false,
    );
    await pressCtrl(app.driver, 'y');
    await waitFor(async () => (listOf(await mainBody(app)) === wrapped ? true : null), {
      timeout: UI_TIMEOUT_MS,
      message: 'Ctrl+Y to wrap b and c again',
    });
  }, 180_000);

  it('wraps a loop’s statements from the top of its body', async (context) => {
    const flow = startFlow(context);
    const app = await flow.launch();
    const mainId = await newEmptyProject(app);
    await foldCodePanel(app);
    const ids = new NodeIds('nest');
    const [x, y] = [print(ids, 'x'), print(ids, 'y')];
    const loop = forever(ids, [x, y]);
    await app.hook.insertBlocks(mainId, 'BODY', [loop]);

    await centerOn(app, x.id);
    await bringIntoView(app.driver, () => connectionPoint(app, loop.id, 'BODY'));
    const { grab, drop } = await toolboxIfOnto(app, loop.id, 'BODY');
    await follow(app.driver, grab, [drop], true);
    expect(await previewOf(app, [x.id, y.id])).toBe(`wrap ${x.id}`);
    await releaseButton(app.driver);
    await waitFor(
      async () => {
        const text = listOf(await mainBody(app));
        return /^\S+\{\S+\{/.test(text) && text.endsWith(`{${x.id} ${y.id}}}`) ? true : null;
      },
      { timeout: UI_TIMEOUT_MS, message: 'the loop’s body to be [if{x, y}]' },
    );
    const body = statementsOf(present((await mainBody(app))[0]), 'BODY');
    expect(body.map((node) => node.type)).toEqual(['control.if']);
  }, 120_000);

  it('wraps the statement below the gap at three zooms, as the preview shows', async (context) => {
    const sweep = (process.env['B2C_E2E_WRAP_SWEEP'] ?? '') !== '';
    const flow = startFlow(context);
    const counts: Record<string, unknown>[] = [];
    for (const scale of [0.5, 0.9, 1.5]) {
      const file = path.join(flow.folders.projects, `wrap-${String(scale)}.b2c`);
      writeProject(file, scale);
      // Large enough for the prints and the if below them at the zoom 1.5.
      const app = await flow.launch({ dialogs: { open: [file] }, windowSize: WINDOW });
      await openFromStartPage(app);
      // The C++ panel and the bottom dock are folded away to give the canvas the room (the app
      // remembers them folded from the launch before).
      if (
        (await app.driver.findElements(By.css('aside.right-dock[data-collapsed="false"]'))).length
      ) {
        await foldCodePanel(app);
      }
      if (
        (await app.driver.findElements(By.css('section.bottom-dock[data-collapsed="false"]')))
          .length
      ) {
        await clickTestId(app.driver, 'bottom-dock-toggle');
        await waitFor(
          async () =>
            (await app.driver.findElements(By.css('section.bottom-dock[data-collapsed="true"]')))
              .length > 0,
          { timeout: UI_TIMEOUT_MS, message: 'the bottom dock to fold away' },
        );
      }
      const shown = await app.driver.executeScript<number | null>(CANVAS_SCALE_SCRIPT);
      expect(shown, 'the saved zoom').toBeCloseTo(scale, 3);
      const initial = await blocksText(app);

      const offsets: [number, number][] = [];
      if (sweep) {
        for (let dy = -60; dy <= 60; dy += 20) {
          for (let dx = -60; dx <= 60; dx += 20) {
            offsets.push([dx, dy]);
          }
        }
      } else {
        offsets.push([0, 0], [10, 0], [-10, 0], [0, 10], [0, -10]);
      }
      const tally = {
        points: offsets.length,
        drops: 0,
        mismatches: 0,
        lags: 0,
        swaps: 0,
        insertions: 0,
        bumpUndos: 0,
      };
      const results: Record<string, number> = {};
      for (const [dx, dy] of offsets) {
        const outcomes = new Set<string>();
        // The pointer comes from above (-1) or below (1) in the sweep, from the right (0) else.
        for (const from of sweep ? [-1, 1] : [0]) {
          // The gap well inside the canvas, and an if of the toolbox's Control category taken out
          // to its right (Blockly makes the new block at the canvas's zoom).
          const gap = await scrollIntoView(app.driver, app.hook, () =>
            connectionPoint(app, present(PRINT_IDS[1]), 'next'),
          );
          await openCategory(app.driver, 'Control');
          const toolboxIf = await waitFor(() => app.hook.flyoutBlockId('control.if', {}), {
            timeout: UI_TIMEOUT_MS,
            message: 'an if in the Control category',
          });
          const grab = await flyoutGrabPoint(app, toolboxIf);
          const out = { x: gap.x + LEAVE_TOOLBOX_PX, y: gap.y };
          await follow(app.driver, grab, [out], true);
          const ifId = await waitFor(
            () => app.driver.executeScript<string | null>(DRAGGED_SCRIPT),
            {
              timeout: UI_TIMEOUT_MS,
              message: 'the if taken from the toolbox to be dragged',
            },
          );
          // Blockly moves a dragged block's connections only at the drop: the top is still where
          // the new block was made, under the press, and the block has moved with the pointer.
          const own = await connectionPoint(app, ifId, 'previous');
          const target = {
            x: gap.x + (grab.x - own.x) + dx * scale,
            y: gap.y + (grab.y - own.y) + dy * scale,
          };
          await follow(
            app.driver,
            out,
            [{ x: target.x + 60, y: target.y + 120 * from }, target],
            false,
          );
          const arrival = await previewOf(app, PRINT_IDS);
          // A person's hand settles before letting go: a pixel down and back.
          await follow(app.driver, target, [{ x: target.x, y: target.y + 1 }, target], false);
          const preview = await previewOf(app, PRINT_IDS);
          await releaseButton(app.driver);
          await sleep(DROP_SETTLE_MS);
          const result = resultOf(await currentDocument(app), ifId);
          const side = from < 0 ? 'above' : from > 0 ? 'below' : 'the right';
          const facts = `zoom ${String(scale)}, ${String(dx)}, ${String(dy)} units off, from ${side}: preview "${arrival}" on arrival, "${preview}" settled, drop "${result}"`;
          tally.drops += 1;
          tally.lags += arrival === preview ? 0 : 1;
          tally.mismatches += preview === result ? 0 : 1;
          tally.insertions += result === 'insert' ? 1 : 0;
          results[result] = (results[result] ?? 0) + 1;
          outcomes.add(result);
          expect(preview, facts).toBe(result);
          if (!sweep) {
            expect(result, facts).toBe(`wrap ${present(PRINT_IDS[2])}`);
          }
          // Ctrl+Z takes the new if away again, and main is as it was: in one step after a wrap.
          // An if left loose next to `main` may take a second, as Blockly bumped it away from
          // main's connections in a step of its own (counted).
          const undos = await undoDrop(app, initial, facts, result === 'none' ? 3 : 1);
          tally.bumpUndos += undos > 1 ? 1 : 0;
        }
        tally.swaps += outcomes.size > 1 ? 1 : 0;
      }
      counts.push({ scale, ...tally, results });
      await app.quit();
    }
    if (sweep) {
      const line = JSON.stringify({ wrapSweep: counts });
      console.log(line);
      writeArtifact(
        artifactDir(harnessSettings().artifacts, context.task.name),
        'wrap-sweep.json',
        `${line}\n`,
      );
    }
  }, 1_800_000);
});

/** `value`, which the test expects to be there. */
function present<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) {
    throw new Error('missing');
  }
  return value;
}

/**
 * Writes a project (the flows' fixture's header) whose `main` holds the five prints, saved at zoom
 * `scale`.
 */
function writeProject(file: string, scale: number): void {
  const template = JSON.parse(readFileSync(path.join(FIXTURES, 'scopes.b2c'), 'utf8')) as Record<
    string,
    unknown
  >;
  const ids = new NodeIds('x');
  const prints = PRINT_IDS.map((id, index) => ({ ...print(ids, `p${String(index + 1)}`), id }));
  const main = {
    id: 'e2e_wrap_main',
    type: 'program.main',
    v: 1,
    x: 40,
    y: 40,
    statements: { BODY: prints },
  };
  const project = {
    ...template,
    modules: [
      {
        id: 'mod_main',
        name: 'main',
        workspace: { blocks: [main], viewport: { x: 0, y: 0, scale } },
      },
    ],
  };
  writeFileSync(file, JSON.stringify(project));
}
