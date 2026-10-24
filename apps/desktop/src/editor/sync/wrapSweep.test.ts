/**
 * How predictable wrapping is (docs/spec/03-block-language.md §3.3 "Placing blocks"; the W5-2
 * spike of docs/plans/m2-polish-plan.md): an empty *if* is dragged with pointer events through
 * Blockly's own gestures onto `main`'s list of five *print* statements, its top dropped at
 * offsets of up to ±60 canvas units in x and y around the gap between the second and third, at
 * the zooms 0.5, 0.9 and 1.5. Each drop is made twice, the pointer coming from above and from
 * below in ten moves; at the drop point the pointer then shakes by one pixel five times before
 * the release. The test records the preview after each move and what the drop did.
 *
 * - A **mismatch** is a drop that did something else than its preview showed at the release.
 * - A **flicker** is a drop whose preview goes back to one it showed before while the pointer
 *   shakes: between two places (**place flicker**), or between a place and none (**edge
 *   flicker**, at the edge of the search radius, as for any block).
 * - A **lag** is a drop whose preview changes once at the drop point and then stays: the preview
 *   of the place before still stood in the way (a preview moves the statements below it, as
 *   Blockly's insertion previews do, so the next place down is found a move late), or the pointer
 *   was at the boundary between two places.
 * - A **swap** is a drop point where the two approaches give different results (Blockly prefers
 *   the place it already previews, so the boundary between two places depends on the way the
 *   pointer came); an **insert/wrap swap** is one where one approach inserts the *if* and the
 *   other wraps.
 *
 * Happy-dom lays nothing out, but Blockly places blocks and connections from the Zelos renderer's
 * constants, so the geometry is the real one apart from text widths; drops with real pointer
 * input in the webviews are e2e/specs/flows/placement-wrap.e2e.ts.
 *
 * Every run checks the editor's rule on a 20-unit grid with both radii below: no mismatch, no
 * place flicker, no insert/wrap swap, never an insertion, and drops out of reach as the radius
 * says (so the radius measured is the one the drag uses). With `B2C_WRAP_SWEEP=1` it measures a
 * 5-unit grid instead, with Blockly's radius (28 units, a preference of 8 for the place already
 * previewed) and the editor's zoom-aware radius of W5-1 (44 screen pixels, 48 to 96 units, a
 * preference of 20, `EDITOR_REACH` in ./drag.ts), and prints the table recorded in the plan's W5-2
 * status, for these rules:
 *
 * - **top**, the editor's: the C-block's previous connection against the connection above the
 *   statement (blockly-ext's `wrapPlace`);
 * - **mouth**, D13 (a) as the plan first described it: the C-block's first mouth against the
 *   statement's previous connection, and no insertion between statements;
 * - **either**, D13 (b) without a tie-break: that mouth pair, and Blockly's insertion;
 * - **print**, the baseline: a *print* dragged instead of the *if*, which Blockly inserts.
 *
 * The radius reaches the drag through the strategy the dragger gives each block
 * (`B2cBlockDragger.dragStrategyFor`, which the sweep's draggers override), since the editor's
 * strategy does not read Blockly's configuration; that configuration keeps Blockly's values, so
 * bumping is the same in every row.
 *
 * A rule takes a few minutes per zoom; run each rule and zoom in a process of its own, for
 * example (Vitest shows the table only with `--disableConsoleIntercept`):
 *
 * ```sh
 * B2C_WRAP_SWEEP=mouth B2C_WRAP_SWEEP_ZOOMS=0.9 \
 *   pnpm --filter @blocks2cpp/desktop exec vitest run src/editor/sync/wrapSweep.test.ts \
 *   -t measures --disableConsoleIntercept
 * ```
 *
 * (on Windows, set the variables with `$env:B2C_WRAP_SWEEP = 'mouth'` and so on in PowerShell).
 */
import {
  B2cConnectionChecker,
  B2cConnectionPreviewer,
  firstMouth,
  isEmptyStatementCBlock,
  previewedWrap,
  wrapPlace,
  type WrapPlace,
} from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';

import { B2cBlockDragger, B2cBlockDragStrategy, EDITOR_REACH, type SnapReach } from './drag';
import { disposeWorkspaces, present, renderedWorkspace, setViewSize } from './testing';

const PREVIOUS_STATEMENT: number = Blockly.ConnectionType.PREVIOUS_STATEMENT;
const NEXT_STATEMENT: number = Blockly.ConnectionType.NEXT_STATEMENT;

/**
 * The wrap of the **mouth** rule: the first mouth of an empty statement C-block dragged with the
 * pointer, against the previous connection of a statement in a list (looking through the wrap
 * preview's marker for the connection above it).
 */
function mouthWrap(a: Blockly.Connection, b: Blockly.Connection): WrapPlace | null {
  const block = a.getSourceBlock();
  if (
    a.type !== NEXT_STATEMENT ||
    b.type !== PREVIOUS_STATEMENT ||
    !Blockly.common.draggingConnections.includes(a) ||
    !isEmptyStatementCBlock(block) ||
    firstMouth(block) !== a
  ) {
    return null;
  }
  const wrapped = b.getSourceBlock();
  if (wrapped.isInsertionMarker() || !wrapped.isMovable()) {
    return null;
  }
  let place = b.targetConnection;
  const holder = place?.getSourceBlock() ?? null;
  if (holder?.isInsertionMarker() === true) {
    place = holder.previousConnection?.targetConnection ?? null;
  }
  if (place === null || place.getSourceBlock().isInsertionMarker()) {
    return null;
  }
  return { block, mouth: a, place, wrapped };
}

/** The **mouth** rule's checker: the mouth pair, and no insertion of an empty C-block. */
class MouthChecker extends B2cConnectionChecker {
  override doDragChecks(
    a: Blockly.RenderedConnection,
    b: Blockly.RenderedConnection,
    distance: number,
  ): boolean {
    if (mouthWrap(a, b) !== null) {
      return a.distanceFrom(b) <= distance;
    }
    if (wrapPlace(a, b) !== null) {
      return false;
    }
    return super.doDragChecks(a, b, distance);
  }
}

/** The **either** rule's checker: the mouth pair, and Blockly's own insertion. */
class EitherChecker extends B2cConnectionChecker {
  override doDragChecks(
    a: Blockly.RenderedConnection,
    b: Blockly.RenderedConnection,
    distance: number,
  ): boolean {
    if (mouthWrap(a, b) !== null) {
      return a.distanceFrom(b) <= distance;
    }
    return Blockly.ConnectionChecker.prototype.doDragChecks.call(this, a, b, distance);
  }
}

/** Previews the mouth pair as a wrap (and every other pair as Blockly does). */
class MouthPreviewer extends B2cConnectionPreviewer {
  protected override wrapAt(
    draggedConn: Blockly.RenderedConnection,
    staticConn: Blockly.RenderedConnection,
  ): WrapPlace | null {
    return mouthWrap(draggedConn, staticConn);
  }
}

/**
 * The editor's dragger, finishing a mouth-pair wrap: Blockly connects the mouth to the statement
 * (taking it out of its list), and the C-block is then connected where the statement was.
 */
class MouthDragger extends B2cBlockDragger {
  override onDragEnd(e: PointerEvent, totalDelta?: Blockly.utils.Coordinate): void {
    const wrap = this.draggable instanceof Blockly.BlockSvg ? previewedWrap(this.draggable) : null;
    super.onDragEnd(e, totalDelta);
    const own = wrap?.block.previousConnection ?? null;
    if (
      wrap !== null &&
      own !== null &&
      own.targetConnection === null &&
      wrap.place.targetConnection === null &&
      wrap.mouth.targetBlock() === wrap.wrapped
    ) {
      own.connect(wrap.place);
    }
  }
}

/** A rule to measure: the block dragged and the components the canvas injects. */
interface Rule {
  readonly name: 'top' | 'mouth' | 'either' | 'print';
  readonly dragged: 'control.if' | 'io.print';
  readonly plugins: {
    readonly connectionChecker: typeof B2cConnectionChecker;
    readonly connectionPreviewer: typeof B2cConnectionPreviewer;
    /** The dragger, which drags with the radius measured ({@link reaching}). */
    readonly blockDragger: typeof B2cBlockDragger;
  };
}

const EDITOR_PLUGINS: Rule['plugins'] = {
  connectionChecker: B2cConnectionChecker,
  connectionPreviewer: B2cConnectionPreviewer,
  blockDragger: B2cBlockDragger,
};

const MOUTH_PLUGINS: Rule['plugins'] = {
  connectionChecker: MouthChecker,
  connectionPreviewer: MouthPreviewer,
  blockDragger: MouthDragger,
};

const TOP: Rule = { name: 'top', dragged: 'control.if', plugins: EDITOR_PLUGINS };

const RULES: readonly Rule[] = [
  TOP,
  { name: 'mouth', dragged: 'control.if', plugins: MOUTH_PLUGINS },
  {
    name: 'either',
    dragged: 'control.if',
    plugins: { ...MOUTH_PLUGINS, connectionChecker: EitherChecker },
  },
  { name: 'print', dragged: 'io.print', plugins: EDITOR_PLUGINS },
];

/** A search radius and preference for the place already previewed (canvas units) to measure. */
interface Radius {
  readonly name: string;
  readonly reach: SnapReach;
}

/** Blockly's own: 28 units at every zoom, and a preference of 8. */
const BLOCKLY_RADIUS: Radius = {
  name: 'Blockly (28, 8)',
  reach: { radius: () => 28, statementPreference: 8, valuePreference: 8 },
};

/** The editor's (W5-1). */
const ZOOM_AWARE_RADIUS: Radius = { name: 'W5-1 (44 px, 48–96, 20)', reach: EDITOR_REACH };

const RADII = [BLOCKLY_RADIUS, ZOOM_AWARE_RADIUS] as const;

/** `Dragger`, dragging every block with the editor's strategy at `radius`'s reach. */
function reaching(Dragger: typeof B2cBlockDragger, radius: Radius): typeof B2cBlockDragger {
  return class extends Dragger {
    protected override dragStrategyFor(block: Blockly.BlockSvg): Blockly.IDragStrategy {
      return new B2cBlockDragStrategy(block, radius.reach);
    }
  };
}

const ZOOMS = [0.5, 0.9, 1.5] as const;

const POINTER = {
  bubbles: true,
  cancelable: true,
  pointerId: 1,
  isPrimary: true,
  button: 0,
  buttons: 1,
  pointerType: 'mouse',
} as const;

/** Where the press on the dragged block is, in client pixels. */
const PRESS = { x: 100, y: 100 } as const;

/** `main`'s statements. */
const PRINTS = ['p1', 'p2', 'p3', 'p4', 'p5'] as const;

/** The ID of the dragged block, which the lists show as `C` (as they show its preview). */
const DRAGGED = 'C';

/** Where the dragged block rests between drops (canvas units). */
const REST = { x: 900, y: 700 } as const;

/**
 * The canvas of a sweep, dragging with `radius`: `main` with the five prints, and the dragged
 * block far away.
 */
function sweepCanvas(rule: Rule, radius: Radius, scale: number): Blockly.WorkspaceSvg {
  const plugins = { ...rule.plugins, blockDragger: reaching(rule.plugins.blockDragger, radius) };
  const svg = renderedWorkspace({ plugins, move: { scrollbars: true, drag: true } });
  setViewSize(svg, 1600, 1200);
  Blockly.svgResize(svg);
  svg.setScale(scale);
  let body: Blockly.serialization.blocks.State | undefined;
  for (const id of [...PRINTS].reverse()) {
    body = { type: 'io.print', id, ...(body === undefined ? {} : { next: { block: body } }) };
  }
  Blockly.serialization.blocks.append(
    {
      type: 'program.main',
      id: 'main',
      x: 40,
      y: 40,
      inputs: { BODY: { block: present(body, 'the prints') } },
    },
    svg,
  );
  Blockly.serialization.blocks.append({ type: rule.dragged, id: DRAGGED, ...REST }, svg);
  return svg;
}

/** Puts the prints back in `main` and the dragged block where it rests, with events off. */
function restoreBlocks(svg: Blockly.WorkspaceSvg): Blockly.BlockSvg {
  Blockly.Events.disable();
  try {
    const dragged = present(svg.getBlockById(DRAGGED), 'the dragged block');
    dragged.unplug(false);
    const prints = PRINTS.map((id) => present(svg.getBlockById(id), id));
    for (const print of prints) {
      if (print.previousConnection.isConnected()) {
        print.previousConnection.disconnect();
      }
    }
    let above = present(svg.getBlockById('main')?.getInput('BODY')?.connection, 'BODY');
    for (const print of prints) {
      above.connect(present(print.previousConnection, 'previous'));
      above = present(print.nextConnection, 'next');
    }
    dragged.moveTo(new Blockly.utils.Coordinate(REST.x, REST.y));
    Blockly.renderManagement.triggerQueuedRenders();
    svg.clearUndo();
    return dragged;
  } finally {
    Blockly.Events.enable();
  }
}

/** A list as text: block IDs, a C-block's first mouth in braces, a preview as `C`. */
function listText(first: Blockly.Block | null): string {
  const parts: string[] = [];
  for (let block = first; block !== null; block = block.getNextBlock()) {
    const name = block.isInsertionMarker() ? DRAGGED : block.id;
    const mouth = firstMouth(block);
    parts.push(mouth === null ? name : `${name}{${listText(mouth.targetBlock())}}`);
  }
  return parts.join(' ');
}

/** `main`'s list now. */
function mainText(svg: Blockly.WorkspaceSvg): string {
  return listText(present(svg.getBlockById('main'), 'main').getInputTargetBlock('BODY'));
}

/** `main`'s list with the dragged block (or its preview) nowhere in it. */
const UNCHANGED = PRINTS.join(' ');

type Kind = 'wrap' | 'insert' | 'loose';

/** What a list says the dragged block does: wraps statements, is inserted, or is not in it. */
function kindOf(text: string): Kind {
  if (/C\{[^}]/.test(text)) {
    return 'wrap';
  }
  return text.split(' ').some((part) => part.startsWith(DRAGGED)) ? 'insert' : 'loose';
}

/** Moves the pointer to `point` (whole pixels, as a real pointer) and draws what changed. */
function moveTo(point: { x: number; y: number }): void {
  document.dispatchEvent(
    new PointerEvent('pointermove', {
      ...POINTER,
      clientX: Math.round(point.x),
      clientY: Math.round(point.y),
    }),
  );
  Blockly.renderManagement.triggerQueuedRenders();
}

/** One drop: the previews at the drop point, and the result. */
interface Drop {
  /** The preview after the approach's last move, then after each move of the shake. */
  readonly previews: readonly string[];
  readonly result: string;
}

/**
 * Drags the block so that its top ends `offset` canvas units from the gap below `p2`, coming from
 * 150 units right of the point and 150 above (`from` -1) or below (+1) it in 10 moves, shakes
 * the pointer by one pixel, and drops.
 */
function dropAt(svg: Blockly.WorkspaceSvg, offset: { x: number; y: number }, from: -1 | 1): Drop {
  const dragged = restoreBlocks(svg);
  // Where the gap and the dragged block's top are before the drag: a preview moves the
  // connections below it, so they are read once.
  const gapConnection = present(svg.getBlockById('p2')?.nextConnection, 'p2 next');
  const gap = { x: gapConnection.x, y: gapConnection.y };
  const ownConnection = present(dragged.previousConnection, 'the dragged block’s top');
  const own = { x: ownConnection.x, y: ownConnection.y };
  const scale = svg.scale;
  const pointer = (x: number, y: number) => ({
    x: PRESS.x + (gap.x + x - own.x) * scale,
    y: PRESS.y + (gap.y + y - own.y) * scale,
  });
  dragged.pathObject.svgPath.dispatchEvent(
    new PointerEvent('pointerdown', { ...POINTER, clientX: PRESS.x, clientY: PRESS.y }),
  );
  Blockly.common.setSelected(dragged);
  const steps = 10;
  for (let step = 0; step <= steps; step += 1) {
    const rest = (steps - step) / steps;
    moveTo(pointer(offset.x + 150 * rest, offset.y + 150 * rest * from));
  }
  const end = pointer(offset.x, offset.y);
  const previews = [mainText(svg)];
  for (const shake of [1, -1, 1, -1, 0]) {
    moveTo({ x: end.x, y: end.y + shake });
    previews.push(mainText(svg));
  }
  document.dispatchEvent(
    new PointerEvent('pointerup', {
      ...POINTER,
      clientX: Math.round(end.x),
      clientY: Math.round(end.y),
    }),
  );
  Blockly.renderManagement.triggerQueuedRenders();
  return { previews, result: mainText(svg) };
}

/** How a drop's previews behaved while the pointer shook. */
function shakeOf(drop: Drop): 'steady' | 'lag' | 'place flicker' | 'edge flicker' {
  // The previews while the pointer shook, each change once.
  const shown: string[] = [];
  for (const text of drop.previews.slice(1)) {
    if (shown.at(-1) !== text) {
      shown.push(text);
    }
  }
  if (new Set(shown).size < shown.length) {
    // Back to a preview shown before: between places, or between a place and none.
    const places = new Set(shown.filter((text) => text !== UNCHANGED));
    return places.size > 1 ? 'place flicker' : 'edge flicker';
  }
  // Changed at most once and stayed: the place a move before stood in the way, or the boundary
  // between two places was crossed.
  return shown.length > 1 || drop.previews[0] !== shown[0] ? 'lag' : 'steady';
}

/** The counts of a sweep. */
interface Sweep {
  readonly points: number;
  readonly drops: number;
  readonly mismatches: number;
  readonly placeFlickers: number;
  readonly edgeFlickers: number;
  readonly lags: number;
  readonly swaps: number;
  readonly insertWrapSwaps: number;
  /** How often each result came. */
  readonly results: Readonly<Record<string, number>>;
}

/** Sweeps the grid of `step` units over ±60 at `scale` with `rule` and `radius`. */
async function sweep(rule: Rule, radius: Radius, scale: number, step: number): Promise<Sweep> {
  const svg = sweepCanvas(rule, radius, scale);
  // Blockly tracks the connections of blocks it has just loaded a millisecond later.
  await new Promise((resolve) => setTimeout(resolve, 5));
  const counts = {
    points: 0,
    drops: 0,
    mismatches: 0,
    placeFlickers: 0,
    edgeFlickers: 0,
    lags: 0,
    swaps: 0,
    insertWrapSwaps: 0,
  };
  const results: Record<string, number> = {};
  try {
    for (let y = -60; y <= 60; y += step) {
      for (let x = -60; x <= 60; x += step) {
        const above = dropAt(svg, { x, y }, -1);
        const below = dropAt(svg, { x, y }, 1);
        counts.points += 1;
        for (const drop of [above, below]) {
          counts.drops += 1;
          counts.mismatches += drop.previews.at(-1) === drop.result ? 0 : 1;
          results[drop.result] = (results[drop.result] ?? 0) + 1;
          const shake = shakeOf(drop);
          counts.placeFlickers += shake === 'place flicker' ? 1 : 0;
          counts.edgeFlickers += shake === 'edge flicker' ? 1 : 0;
          counts.lags += shake === 'lag' ? 1 : 0;
        }
        counts.swaps += above.result === below.result ? 0 : 1;
        const kinds = new Set([kindOf(above.result), kindOf(below.result)]);
        counts.insertWrapSwaps += kinds.has('insert') && kinds.has('wrap') ? 1 : 0;
        // Let Blockly draw in an animation frame and run its timers (bringing a dropped block to
        // the front), as between two drags of a person.
        await Blockly.renderManagement.finishQueuedRenders();
        await new Promise((resolve) => setTimeout(resolve, 0));
      }
    }
  } finally {
    svg.cancelCurrentGesture();
    disposeWorkspaces();
  }
  return { ...counts, results };
}

/**
 * Blockly ripples a connection it makes with an SVG animation; happy-dom has no SMIL, so the
 * animation's `beginElement` is a no-op here.
 */
const svgElement = SVGElement.prototype as SVGElement & { beginElement?: () => void };
const hadBeginElement = typeof svgElement.beginElement === 'function';

/**
 * Blockly 12.5.1's render queue keeps every block disposed right after it was unplugged (an
 * upstream bug, see the plan's W5-2 status; blockly-ext's previewer works around it for its wrap
 * previews), so Blockly's own insertion markers stay in memory, and thousands of drags run out of
 * it. Here the canvas is drawn before an insertion marker is disposed, which takes the marker out
 * of the queue, as the previewer does for its own.
 */
// eslint-disable-next-line @typescript-eslint/unbound-method -- called with the block as `this`
const blockDispose = Blockly.BlockSvg.prototype.dispose;

function disposeDrawn(this: Blockly.BlockSvg, ...args: Parameters<typeof blockDispose>): void {
  if (this.isInsertionMarker() && this.getParent() === null && !this.isDeadOrDying()) {
    Blockly.renderManagement.triggerQueuedRenders(this.workspace);
  }
  blockDispose.apply(this, args);
}

/**
 * Blockly measures text on a canvas it adds to the page once, but happy-dom's canvas has no 2D
 * context, so Blockly would add another canvas for every text it measures (thousands in a sweep).
 * Here the canvas has a context that measures every text as 0 wide, as Blockly does without one.
 */
const canvasPrototype = HTMLCanvasElement.prototype;
// eslint-disable-next-line @typescript-eslint/unbound-method -- restored as the prototype's method
const canvasContext = canvasPrototype.getContext;
const hadCanvasContext = document.createElement('canvas').getContext('2d') !== null;
const zeroWidthText = { font: '', measureText: () => ({ width: 0 }) };

beforeAll(() => {
  if (!hadBeginElement) {
    svgElement.beginElement = () => undefined;
  }
  Blockly.BlockSvg.prototype.dispose = disposeDrawn;
  if (!hadCanvasContext) {
    canvasPrototype.getContext = (() => zeroWidthText) as unknown as typeof canvasContext;
  }
});

afterAll(() => {
  if (!hadBeginElement) {
    delete svgElement.beginElement;
  }
  Blockly.BlockSvg.prototype.dispose = blockDispose;
  canvasPrototype.getContext = canvasContext;
});

afterEach(() => {
  disposeWorkspaces();
});

/**
 * The rules the full measurement measures: `B2C_WRAP_SWEEP=1` for all, or a comma-separated list
 * of their names; none without it.
 */
const MEASURED: readonly Rule[] = ((): readonly Rule[] => {
  const wanted = String(import.meta.env['B2C_WRAP_SWEEP'] ?? '');
  if (wanted === '') {
    return [];
  }
  const names = wanted.split(',');
  return wanted === '1' ? RULES : RULES.filter((rule) => names.includes(rule.name));
})();

/** The zooms the full measurement measures: all, or those `B2C_WRAP_SWEEP_ZOOMS` lists. */
const MEASURED_ZOOMS: readonly number[] = ((): readonly number[] => {
  const wanted = String(import.meta.env['B2C_WRAP_SWEEP_ZOOMS'] ?? '');
  return wanted === '' ? ZOOMS : wanted.split(',').map(Number);
})();

describe('wrapping an empty C-block’s top onto a statement', () => {
  it.each(RADII.flatMap((radius) => ZOOMS.map((scale) => [radius.name, scale, radius] as const)))(
    'is predictable with the radius %s at zoom %s: no mismatch, no place flicker, no insert/wrap swap',
    async (_, scale, radius) => {
      const result = await sweep(TOP, radius, scale, 20);
      expect(result.points).toBe(49);
      expect(result.mismatches).toBe(0);
      expect(result.placeFlickers).toBe(0);
      expect(result.insertWrapSwaps).toBe(0);
      // Only wraps or nothing: the empty if is never inserted between two statements.
      for (const text of Object.keys(result.results)) {
        expect(kindOf(text), text).not.toBe('insert');
      }
      // Its top at the gap wraps the third print and everything below it.
      expect(result.results['p1 p2 C{p3 p4 p5}']).toBeGreaterThan(0);
      // The radius measured is the drag's: Blockly's 28 units leave the grid's far points out of
      // reach at every zoom, and W5-1's 88 units at zoom 0.5 none.
      const outOfReach = result.results[UNCHANGED] ?? 0;
      if (radius === BLOCKLY_RADIUS) {
        expect(outOfReach).toBeGreaterThan(0);
      } else if (scale === 0.5) {
        expect(outOfReach).toBe(0);
      }
    },
    60_000,
  );

  it.skipIf(MEASURED.length === 0)(
    'measures the rules (B2C_WRAP_SWEEP=1, or a list of rules)',
    async () => {
      const rows: string[] = [
        '| Rule | Radius | Zoom | Drops | Mismatches | Place flickers | Edge flickers | Lags | Swaps (of points) | Insert/wrap swaps |',
        '|---|---|---|---|---|---|---|---|---|---|',
      ];
      for (const rule of MEASURED) {
        for (const radius of RADII) {
          for (const scale of MEASURED_ZOOMS) {
            const r = await sweep(rule, radius, scale, 5);
            const cells = [
              String(r.drops),
              String(r.mismatches),
              String(r.placeFlickers),
              String(r.edgeFlickers),
              String(r.lags),
              `${String(r.swaps)} of ${String(r.points)}`,
              String(r.insertWrapSwaps),
            ];
            rows.push(
              `| ${rule.name} | ${radius.name} | ${String(scale)} | ${cells.join(' | ')} |`,
            );
            console.log(rows.at(-1), JSON.stringify(r.results));
          }
        }
      }
      console.log(rows.join('\n'));
    },
    3_600_000,
  );
});
