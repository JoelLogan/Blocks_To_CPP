/**
 * Wrapping statements in an empty C-block with a pointer drag (docs/spec/03-block-language.md §3.3
 * "Placing blocks"), through the editor's block dragger, connection checker and previewer and
 * Blockly's own gestures, with pointer events, on the guessing game (`main`: b002, b003, b004 and
 * the loop b010, whose body is b005 and the `if` b009):
 *
 * - an empty `if` whose top is dropped at the gap above b004 takes b004's place, with b004 and the
 *   loop in its first mouth; its preview shows that, and moving away leaves the canvas as it was;
 * - one Undo takes the drop back and Redo wraps again; the drop is one event group, settled by the
 *   drop handler once;
 * - Escape during the preview changes nothing, and a drop back where the drag started leaves the
 *   document byte for byte as it was;
 * - inside a mouth, at exact alignment and 10 units off, at the end of a list (an insertion), at
 *   the top of a loose stack (the mouth wraps it), and over a delete area (only the `if` goes);
 * - a block moved with the keyboard is inserted, not wrapped;
 * - whatever the pointer previewed on its way, a drag Blockly reverts leaves the document byte for
 *   byte as it was (a property test), and an empty C-block taken out of a list and sent back over
 *   its old place goes back in place, wrapping nothing;
 * - a counted loop taken from the toolbox that wraps statements names its counter after what they
 *   declare too: wrapping `create i` or a `for i` loop, it counts with `j` (03 §3.6).
 *
 * The editor injects the checker and previewer these drags use. These are the cases of the plan's
 * W5-2 for the editor's drag tests; they sit beside drag.test.ts
 * so that the placement work on the drag strategy (W5-1) changes another file. The predictability
 * sweep is ./wrapSweep.test.ts, the end-to-end drops e2e/specs/flows/placement-wrap.e2e.ts.
 */
import type { BdmBlock, BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import { B2C_CHECKER_NAME, B2C_PREVIEWER_NAME, firstMouth } from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';
import { afterAll, afterEach, beforeAll, describe, expect, it } from 'vitest';

import { useAppStore } from '../../app/store';
import { editorInjectOptions } from '../EditorWorkspace';
import { dropTargets } from '../keyboard/targets';
import { createToolboxPlugin, toolboxInjectOptions } from '../toolbox';
import { B2cBlockDragger, handleBlockDrops } from './drag';
import {
  canonicalText,
  disposeWorkspaces,
  EXAMPLE_PROJECTS,
  loadText,
  present,
  renderedWorkspace,
  seededRandom,
  setViewSize,
  startSession,
  testCore,
  type TestSession,
} from './testing';
import { allBlocks } from './traverse';

const core = await testCore();

let running: TestSession | null = null;

/**
 * Blockly ripples a connection it makes with an SVG animation; happy-dom has no SMIL, so the
 * animation's `beginElement` is a no-op here.
 */
const svgElement = SVGElement.prototype as SVGElement & { beginElement?: () => void };
const hadBeginElement = typeof svgElement.beginElement === 'function';

beforeAll(() => {
  if (!hadBeginElement) {
    svgElement.beginElement = () => undefined;
  }
});

afterAll(() => {
  if (!hadBeginElement) {
    delete svgElement.beginElement;
  }
});

afterEach(() => {
  // A test that failed during a drag leaves it running: end it while its blocks still exist.
  const workspace = running?.workspace;
  if (workspace instanceof Blockly.WorkspaceSvg) {
    workspace.cancelCurrentGesture();
  }
  running?.dispose();
  running = null;
  disposeWorkspaces();
});

const POINTER = {
  bubbles: true,
  cancelable: true,
  pointerId: 1,
  isPrimary: true,
  button: 0,
  buttons: 1,
  pointerType: 'mouse',
} as const;

/** Where a press on the dragged block starts, in client pixels. */
const PRESS = { x: 100, y: 100 } as const;

/** Where the loose empty `if` starts, in canvas units. */
const START = { x: 600, y: 400 } as const;

/**
 * Waits until Blockly has rendered new blocks and put their connections in its connection
 * database, which it does a millisecond after a load (the editor's loader does the same).
 */
async function placed(): Promise<void> {
  await Blockly.renderManagement.finishQueuedRenders();
  await new Promise((resolve) => setTimeout(resolve, 5));
}

/** Waits until Blockly has delivered (and recorded for undo) the events fired so far. */
function eventsDelivered(): Promise<void> {
  return new Promise((resolve) => {
    requestAnimationFrame(() => {
      setTimeout(() => {
        setTimeout(resolve, 0);
      }, 0);
    });
  });
}

/**
 * The guessing game on a shown editor canvas with the editor's dragger, checker and previewer, at
 * the editor's start zoom (0.9), and a loose empty `if` (`C`) right of `main`.
 */
async function withLooseIf(): Promise<{ svg: Blockly.WorkspaceSvg; dragged: Blockly.BlockSvg }> {
  const wasm = present(core, 'core');
  const svg = renderedWorkspace({
    plugins: {
      blockDragger: B2cBlockDragger,
      connectionChecker: B2C_CHECKER_NAME,
      connectionPreviewer: B2C_PREVIEWER_NAME,
    },
    move: { scrollbars: true, drag: true },
  });
  setViewSize(svg, 1000, 700);
  Blockly.svgResize(svg);
  const doc = loadText(wasm, present(EXAMPLE_PROJECTS['guessing_game.b2c'], 'guessing game'));
  running = startSession(wasm, doc, { workspace: svg });
  await running.session.flush();
  svg.setScale(0.9);
  const dragged = Blockly.serialization.blocks.append(
    { type: 'control.if', id: 'C', ...START },
    svg,
  ) as Blockly.BlockSvg;
  await placed();
  await running.session.flush();
  await eventsDelivered();
  svg.clearUndo();
  return { svg, dragged };
}

/** A statement list as text: IDs, a C-block's first mouth in braces, a drag preview as `M`. */
function listText(first: Blockly.Block | null): string {
  const parts: string[] = [];
  for (let block = first; block !== null; block = block.getNextBlock()) {
    const name = block.isInsertionMarker() ? 'M' : block.id;
    const mouth = firstMouth(block);
    parts.push(mouth === null ? name : `${name}{${listText(mouth.targetBlock())}}`);
  }
  return parts.join(' ');
}

/** `main`'s statements now (b010's body shown), as {@link listText}. */
function mainText(svg: Blockly.WorkspaceSvg): string {
  return listText(present(svg.getBlockById('b011'), 'main').getInputTargetBlock('BODY'));
}

/** `main` before any drop. */
const MAIN_TEXT = 'b002 b003 b004 b010{b005 b009{b006}}';

/**
 * The pointer position (from a press at {@link PRESS}) that brings `block`'s previous connection
 * `dx`, `dy` canvas units from connection `target`.
 */
function pointerAt(
  svg: Blockly.WorkspaceSvg,
  block: Blockly.BlockSvg,
  target: Blockly.Connection,
  dx = 0,
  dy = 0,
): { x: number; y: number } {
  const own = present(block.previousConnection, 'previous');
  return {
    x: PRESS.x + (target.x - own.x + dx) * svg.scale,
    y: PRESS.y + (target.y - own.y + dy) * svg.scale,
  };
}

/** The connection below block `id` (its next connection). */
function below(svg: Blockly.WorkspaceSvg, id: string): Blockly.Connection {
  return present(svg.getBlockById(id)?.nextConnection, `${id} next`);
}

/** Presses on `block` (selecting it, as a browser's focus does). */
function press(block: Blockly.BlockSvg): void {
  block.pathObject.svgPath.dispatchEvent(
    new PointerEvent('pointerdown', { ...POINTER, clientX: PRESS.x, clientY: PRESS.y }),
  );
  Blockly.common.setSelected(block);
}

/** Moves the pointer, and lets Blockly draw what the move changed. */
async function moveTo(point: { x: number; y: number }): Promise<void> {
  document.dispatchEvent(
    new PointerEvent('pointermove', { ...POINTER, clientX: point.x, clientY: point.y }),
  );
  await Blockly.renderManagement.finishQueuedRenders();
}

async function release(point: { x: number; y: number }): Promise<void> {
  document.dispatchEvent(
    new PointerEvent('pointerup', { ...POINTER, clientX: point.x, clientY: point.y }),
  );
  await Blockly.renderManagement.finishQueuedRenders();
}

/** Far right of everything: no place to connect. */
const AWAY = { x: 900, y: 600 } as const;

/** Whether a drag preview (an insertion marker) is on the canvas. */
function previewShown(svg: Blockly.WorkspaceSvg): boolean {
  return allBlocks(svg).some((block) => block.isInsertionMarker());
}

/** The canonical text of the document the canvas holds now. */
function documentText(): string {
  const session = present(running, 'session').session;
  return canonicalText(present(core, 'core'), session.currentDocument());
}

/** The `main` node of a document. */
function mainNode(doc: BdmDocument): BdmBlock {
  return present(
    doc.modules[0]?.workspace.blocks.find((block) => block.type === 'program.main'),
    'main',
  );
}

describe.skipIf(core === null)('an empty C-block dropped on a statement', () => {
  it('previews the wrap, and moving away leaves the canvas as it was', async () => {
    const { svg, dragged } = await withLooseIf();
    const before = JSON.stringify(mainNode(present(running, 'session').session.currentDocument()));

    press(dragged);
    await moveTo(pointerAt(svg, dragged, below(svg, 'b003'), 4, 4));
    expect(mainText(svg)).toBe('b002 b003 M{b004 b010{b005 b009{b006}}}');
    await moveTo(AWAY);
    expect(previewShown(svg)).toBe(false);
    expect(mainText(svg)).toBe(MAIN_TEXT);
    await release(AWAY);

    expect(dragged.getParent()).toBeNull();
    await present(running, 'session').session.flush();
    expect(JSON.stringify(mainNode(present(running, 'session').session.currentDocument()))).toBe(
      before,
    );
  });

  it('takes the statement’s place with it and everything below it in its mouth', async () => {
    const { svg, dragged } = await withLooseIf();
    const at = pointerAt(svg, dragged, below(svg, 'b003'), 4, 4);
    press(dragged);
    await moveTo(at);
    await release(at);

    expect(mainText(svg)).toBe('b002 b003 C{b004 b010{b005 b009{b006}}}');
    expect(previewShown(svg)).toBe(false);
    // The program is read back from the canvas: [b002, b003, if{b004, b010}].
    await present(running, 'session').session.flush();
    const body =
      mainNode(present(running, 'session').session.currentDocument()).statements?.['BODY'] ?? [];
    expect(body.map((node) => node.id)).toEqual(['b002', 'b003', 'C']);
    expect(body[2]?.statements?.['DO0']?.map((node) => node.id)).toEqual(['b004', 'b010']);
  });

  it('is one Undo step, settled once in the drop’s group, and Redo wraps again', async () => {
    const { svg, dragged } = await withLooseIf();
    const before = documentText();
    const settled: { block: string; group: string }[] = [];
    const stop = handleBlockDrops(svg, (block) => {
      settled.push({ block: block.id, group: Blockly.Events.getGroup() });
    });
    const groups = new Set<string>();
    const record = (event: Blockly.Events.Abstract): void => {
      if (!event.isUiEvent) {
        groups.add(event.group);
      }
    };
    svg.addChangeListener(record);

    const at = pointerAt(svg, dragged, below(svg, 'b003'), 4, 4);
    press(dragged);
    await moveTo(at);
    await release(at);
    stop();
    await eventsDelivered();
    svg.removeChangeListener(record);
    expect(mainText(svg)).toBe('b002 b003 C{b004 b010{b005 b009{b006}}}');
    expect(settled).toEqual([{ block: 'C', group: present(settled[0], 'settled').group }]);
    expect([...groups]).toEqual([settled[0]?.group]);

    svg.undo(false);
    await eventsDelivered();
    expect(mainText(svg)).toBe(MAIN_TEXT);
    expect(dragged.getParent()).toBeNull();
    expect(documentText()).toBe(before);

    svg.undo(true);
    await eventsDelivered();
    expect(mainText(svg)).toBe('b002 b003 C{b004 b010{b005 b009{b006}}}');
  });

  it('changes nothing on Escape during the preview, and a drop where it started leaves the document as it was', async () => {
    const { svg, dragged } = await withLooseIf();
    const before = documentText();
    press(dragged);
    await moveTo(pointerAt(svg, dragged, below(svg, 'b003'), 4, 4));
    expect(previewShown(svg)).toBe(true);

    document.dispatchEvent(
      new KeyboardEvent('keydown', { key: 'Escape', code: 'Escape', keyCode: 27, bubbles: true }),
    );
    // Blockly 12.5.1's Escape only closes pop-ups; the drag and its preview go on unchanged.
    expect(svg.isDragging()).toBe(true);
    expect(mainText(svg)).toBe('b002 b003 M{b004 b010{b005 b009{b006}}}');

    await moveTo({ x: PRESS.x, y: PRESS.y + 1 });
    await moveTo(PRESS);
    expect(previewShown(svg)).toBe(false);
    await release(PRESS);
    expect(svg.isDragging()).toBe(false);
    expect(documentText()).toBe(before);
  });

  it('wraps inside a mouth: the statement of the loop’s body and what follows it', async () => {
    const { svg, dragged } = await withLooseIf();
    const at = pointerAt(svg, dragged, below(svg, 'b005'), 3, 3);
    press(dragged);
    await moveTo(at);
    expect(mainText(svg)).toBe('b002 b003 b004 b010{b005 M{b009{b006}}}');
    await release(at);
    expect(mainText(svg)).toBe('b002 b003 b004 b010{b005 C{b009{b006}}}');
  });

  it('wraps the first statement of a list from the list’s top', async () => {
    const { svg, dragged } = await withLooseIf();
    const top = present(svg.getBlockById('b011')?.getInput('BODY')?.connection, 'BODY');
    const at = pointerAt(svg, dragged, top, 2, 2);
    press(dragged);
    await moveTo(at);
    await release(at);
    expect(mainText(svg)).toBe('C{b002 b003 b004 b010{b005 b009{b006}}}');
  });

  it.each([
    [0, 0],
    [10, 0],
    [-10, 0],
    [0, 10],
    [0, -10],
  ])('wraps with its top %s, %s units from the gap', async (dx, dy) => {
    const { svg, dragged } = await withLooseIf();
    const at = pointerAt(svg, dragged, below(svg, 'b003'), dx, dy);
    press(dragged);
    await moveTo(at);
    await release(at);
    expect(mainText(svg)).toBe('b002 b003 C{b004 b010{b005 b009{b006}}}');
  });

  it('is inserted at the end of a list, where nothing is below to wrap', async () => {
    const { svg, dragged } = await withLooseIf();
    const at = pointerAt(svg, dragged, below(svg, 'b010'), 4, 4);
    press(dragged);
    await moveTo(at);
    expect(mainText(svg)).toBe('b002 b003 b004 b010{b005 b009{b006}} M{}');
    await release(at);
    expect(mainText(svg)).toBe('b002 b003 b004 b010{b005 b009{b006}} C{}');
  });

  it('wraps a loose stack with its mouth at the stack’s top, and a preview leaves it in place', async () => {
    const { svg, dragged } = await withLooseIf();
    const stack = Blockly.serialization.blocks.append(
      {
        type: 'io.print',
        id: 's1',
        x: 300,
        y: 600,
        next: { block: { type: 'io.print', id: 's2' } },
      },
      svg,
    ) as Blockly.BlockSvg;
    await placed();
    const where = stack.getRelativeToSurfaceXY();
    const mouth = present(firstMouth(dragged), 'mouth');
    const own = present(dragged.previousConnection, 'previous');
    const top = present(stack.previousConnection, 'stack top');
    const at = {
      x: PRESS.x + (top.x - mouth.x + 2) * svg.scale,
      y: PRESS.y + (top.y - mouth.y + 2) * svg.scale,
    };
    expect(own).not.toBe(mouth);
    press(dragged);
    await moveTo(at);
    expect(previewShown(svg)).toBe(true);
    await moveTo(AWAY);
    expect(previewShown(svg)).toBe(false);
    expect(stack.getParent()).toBeNull();
    expect(stack.getRelativeToSurfaceXY()).toEqual(where);
    await moveTo(at);
    await release(at);
    expect(listText(dragged)).toBe('C{s1 s2}');
    expect(mainText(svg)).toBe(MAIN_TEXT);
  });

  it('is deleted alone when dropped over a delete area after a wrap preview', async () => {
    const { svg, dragged } = await withLooseIf();
    const near = pointerAt(svg, dragged, below(svg, 'b003'), 4, 4);
    /** A delete area left of the pointer's x at `near` (as the toolbox's flyout is one). */
    class EdgeDeleteArea extends Blockly.DeleteArea {
      override id = 'test.deleteArea';
      override getClientRect(): Blockly.utils.Rect {
        return new Blockly.utils.Rect(-100_000, 100_000, -100_000, near.x - 2);
      }
    }
    svg.getComponentManager().addComponent({
      component: new EdgeDeleteArea(),
      weight: 1,
      capabilities: [
        Blockly.ComponentManager.Capability.DELETE_AREA,
        Blockly.ComponentManager.Capability.DRAG_TARGET,
      ],
    });
    svg.recordDragTargets();

    press(dragged);
    await moveTo(near);
    expect(previewShown(svg)).toBe(true);
    await moveTo({ x: near.x - 6, y: near.y });
    expect(previewShown(svg)).toBe(false);
    expect(mainText(svg)).toBe(MAIN_TEXT);
    await release({ x: near.x - 6, y: near.y });
    expect(dragged.isDeadOrDying()).toBe(true);
    expect(mainText(svg)).toBe(MAIN_TEXT);
  });
});

describe('the editor', () => {
  it('drags with the wrap’s checker and previewer', () => {
    const plugins = editorInjectOptions(Blockly.Themes.Zelos).plugins ?? {};
    expect(plugins['connectionChecker']).toBe(B2C_CHECKER_NAME);
    expect(plugins['connectionPreviewer']).toBe(B2C_PREVIEWER_NAME);
    expect(plugins['blockDragger']).toBe(B2cBlockDragger);
  });
});

describe.skipIf(core === null)('an empty C-block moved with the keyboard', () => {
  it('is offered the gap between two statements, and inserted there', async () => {
    const { svg, dragged } = await withLooseIf();
    const target = present(
      dropTargets(dragged).find((place) => place.kind === 'after' && place.owner.id === 'b003'),
      'after b003',
    );
    if (target.kind === 'canvas') {
      throw new Error('not a connection');
    }
    target.connection.connect(present(dragged.previousConnection, 'previous'));
    expect(mainText(svg)).toBe('b002 b003 C{} b004 b010{b005 b009{b006}}');
  });
});

/**
 * A drag target right of x = `edge` (client pixels; everywhere for a very negative `edge`) that
 * sends a dropped block back where its drag started (Blockly's `shouldPreventMove`), so the drop
 * reverts the drag.
 */
class ReturnTarget extends Blockly.DragTarget {
  override id = 'test.returnTarget';

  private readonly edge: number;

  constructor(edge: number) {
    super();
    this.edge = edge;
  }

  override getClientRect(): Blockly.utils.Rect {
    return new Blockly.utils.Rect(-100_000, 100_000, this.edge, 100_000);
  }

  override shouldPreventMove(): boolean {
    return true;
  }
}

describe.skipIf(core === null)('a drag Blockly reverts', () => {
  it('leaves the document byte for byte as it was, whatever was previewed on the way', async () => {
    const { svg, dragged } = await withLooseIf();
    const stack = Blockly.serialization.blocks.append(
      {
        type: 'io.print',
        id: 's1',
        x: 300,
        y: 600,
        next: { block: { type: 'io.print', id: 's2' } },
      },
      svg,
    ) as Blockly.BlockSvg;
    await placed();
    await present(running, 'session').session.flush();
    const before = documentText();
    const edge = 2000;
    svg.getComponentManager().addComponent({
      component: new ReturnTarget(edge),
      weight: 1,
      capabilities: [Blockly.ComponentManager.Capability.DRAG_TARGET],
    });
    svg.recordDragTargets();
    const mouth = present(firstMouth(dragged), 'mouth');
    const top = present(stack.previousConnection, 'stack top');
    // Places to pass: each gap of main and of the loop's body (wraps), the end of main (an
    // insertion), the loose stack's top (a mouth wrap), and nowhere.
    const places = [
      ...['b002', 'b003', 'b004', 'b005', 'b009', 'b010'].map(
        (id) => () => pointerAt(svg, dragged, below(svg, id), 3, 3),
      ),
      () =>
        pointerAt(
          svg,
          dragged,
          present(svg.getBlockById('b011')?.getInput('BODY')?.connection, 'BODY'),
          2,
          2,
        ),
      () => ({
        x: PRESS.x + (top.x - mouth.x + 2) * svg.scale,
        y: PRESS.y + (top.y - mouth.y + 2) * svg.scale,
      }),
      () => AWAY,
    ];
    const random = seededRandom(5_2);
    for (let run = 0; run < 25; run += 1) {
      press(dragged);
      const moves = 3 + Math.floor(random() * 8);
      for (let move = 0; move < moves; move += 1) {
        const place = present(places[Math.floor(random() * places.length)], 'place');
        await moveTo(place());
      }
      // Released over the drag target that sends the block back.
      await moveTo({ x: edge + 10, y: 300 });
      await release({ x: edge + 10, y: 300 });
      await placed();
      expect(previewShown(svg), `run ${String(run)}`).toBe(false);
      expect(documentText(), `run ${String(run)}`).toBe(before);
    }
  }, 60_000);

  it('puts an empty C-block taken out of a list back in place, wrapping nothing', async () => {
    const { svg, dragged } = await withLooseIf();
    // The empty if between b003 and b004, as a keyboard move inserts it.
    below(svg, 'b003').connect(present(dragged.previousConnection, 'previous'));
    await placed();
    await present(running, 'session').session.flush();
    expect(mainText(svg)).toBe('b002 b003 C{} b004 b010{b005 b009{b006}}');
    const before = documentText();
    svg.getComponentManager().addComponent({
      component: new ReturnTarget(-100_000),
      weight: 1,
      capabilities: [Blockly.ComponentManager.Capability.DRAG_TARGET],
    });
    svg.recordDragTargets();
    // Taken out alone (Ctrl held: Blockly closes the gap) and brought back over its old place,
    // where the preview wraps b004 and what follows; released over the drag target that sends
    // it back.
    const withCtrl = { ...POINTER, ctrlKey: true };
    dragged.pathObject.svgPath.dispatchEvent(
      new PointerEvent('pointerdown', { ...withCtrl, clientX: PRESS.x, clientY: PRESS.y }),
    );
    Blockly.common.setSelected(dragged);
    for (const point of [
      { x: PRESS.x + 200, y: PRESS.y + 200 },
      { x: PRESS.x + 3, y: PRESS.y + 3 },
    ]) {
      document.dispatchEvent(
        new PointerEvent('pointermove', { ...withCtrl, clientX: point.x, clientY: point.y }),
      );
      await Blockly.renderManagement.finishQueuedRenders();
    }
    expect(mainText(svg)).toBe('b002 b003 M{b004 b010{b005 b009{b006}}}');
    await release({ x: PRESS.x + 3, y: PRESS.y + 3 });
    await placed();

    expect(mainText(svg)).toBe('b002 b003 C{} b004 b010{b005 b009{b006}}');
    expect(documentText()).toBe(before);
  });
});

/**
 * `text` on a shown editor canvas with the editor's dragger, checker and previewer and the toolbox
 * (its plugin attached to the session's core and store), the toolbox's flyout built.
 */
async function withToolbox(
  text: string,
): Promise<{ svg: Blockly.WorkspaceSvg; flyout: Blockly.IFlyout }> {
  const wasm = present(core, 'core');
  const toolbox = toolboxInjectOptions();
  const svg = renderedWorkspace({
    toolbox: toolbox.toolbox,
    plugins: {
      ...toolbox.plugins,
      blockDragger: B2cBlockDragger,
      connectionChecker: B2C_CHECKER_NAME,
      connectionPreviewer: B2C_PREVIEWER_NAME,
    },
    move: { scrollbars: true, drag: true },
  });
  setViewSize(svg, 1600, 900);
  Blockly.svgResize(svg);
  const session = startSession(wasm, loadText(wasm, text), { workspace: svg });
  const detach = createToolboxPlugin().attach({
    workspace: svg,
    store: useAppStore,
    core: () => wasm,
    selectBlock: () => undefined,
    activeModuleId: () => useAppStore.getState().project?.activeModuleId ?? '',
  });
  running = {
    ...session,
    dispose() {
      detach();
      session.dispose();
    },
  };
  await session.session.flush();
  await Blockly.renderManagement.finishQueuedRenders();
  // The toolbox rebuilds its flyout after the analysis.
  await new Promise((resolve) => setTimeout(resolve, 200));
  await placed();
  await eventsDelivered();
  svg.clearUndo();
  return { svg, flyout: present(svg.getFlyout(), 'flyout') };
}

/**
 * Drags the toolbox's counted loop out of the flyout with the pointer and drops it with its top 4
 * units right of and below connection `target`. Returns the loop created on the canvas.
 */
async function dropToolboxLoop(
  svg: Blockly.WorkspaceSvg,
  flyout: Blockly.IFlyout,
  target: Blockly.Connection,
): Promise<Blockly.BlockSvg> {
  const original = present(
    flyout
      .getWorkspace()
      .getTopBlocks(true)
      .find((block) => block.type === 'control.for_range'),
    'the counted loop in the flyout',
  );
  const before = new Set(svg.getAllBlocks(false).map((block) => block.id));
  original.pathObject.svgPath.dispatchEvent(
    new PointerEvent('pointerdown', { ...POINTER, clientX: PRESS.x, clientY: PRESS.y }),
  );
  // Past the drag radius: Blockly creates the loop on the canvas and drags it.
  const started = { x: PRESS.x + 40, y: PRESS.y };
  await moveTo(started);
  const created = present(
    svg
      .getAllBlocks(false)
      .find((block) => !before.has(block.id) && block.type === 'control.for_range'),
    'the loop taken from the toolbox',
  );
  const own = present(created.previousConnection, 'previous');
  const goal = {
    x: started.x + (target.x - own.x + 4) * svg.scale,
    y: started.y + (target.y - own.y + 4) * svg.scale,
  };
  await moveTo(goal);
  expect(previewShown(svg)).toBe(true);
  await release(goal);
  await placed();
  await present(running, 'session').session.flush();
  return created;
}

/** The diagnostic codes of the latest analysis, with the block each is on. */
function diagnosticsNow(): string[] {
  return (useAppStore.getState().analysis.preview?.diagnostics ?? []).map(
    (diagnostic) => `${diagnostic.code} ${diagnostic.primary.block ?? ''}`,
  );
}

/** Sum to Ten (`main` b005: `create total` b001, the loop `for i` b003, `print total` b004). */
function sumToTen(): string {
  return present(EXAMPLE_PROJECTS['sum_to_ten.b2c'], 'sum to ten');
}

/** Sum to Ten with `main`'s body replaced by `create int i = 0` (b_i) and `print i` (b_p). */
function createAndPrintI(): string {
  const doc = JSON.parse(sumToTen()) as BdmDocument;
  const main = mainNode(doc) as { statements?: Record<string, unknown[]> };
  main.statements = {
    BODY: [
      {
        id: 'b_i',
        type: 'var.declare',
        v: 1,
        fields: { TYPE: 'int', NAME: { sym: 's_ix', name: 'i' }, CONST: false },
        inputs: { VALUE: { expr: [{ num: '0' }] } },
      },
      {
        id: 'b_p',
        type: 'io.print',
        v: 1,
        extra: { itemCount: 1 },
        fields: { NEWLINE: true, SEP: 'none', STREAM: 'out' },
        inputs: { ITEM0: { expr: [{ ref: 's_ix' }] } },
      },
    ],
  };
  return JSON.stringify(doc);
}

describe.skipIf(core === null)(
  'a counted loop taken from the toolbox that wraps statements',
  () => {
    afterEach(() => {
      const workspace = running?.workspace;
      if (workspace instanceof Blockly.WorkspaceSvg) {
        workspace.cancelCurrentGesture();
      }
    });

    it('counts with j around `create i`, which shares its body’s scope, in one Undo step', async () => {
      const { svg, flyout } = await withToolbox(createAndPrintI());
      expect(diagnosticsNow()).toEqual([]);
      const main = present(svg.getBlockById('b005'), 'main');
      const loop = await dropToolboxLoop(
        svg,
        flyout,
        present(main.getInput('BODY')?.connection, 'main’s body'),
      );

      expect(listText(main.getInputTargetBlock('BODY'))).toBe(`${loop.id}{b_i b_p}`);
      // Not i: `create i` in its body would be B2C-E0210 (a second i in one scope).
      expect(loop.getField('VAR')?.getText()).toBe('j');
      expect(diagnosticsNow()).toEqual([]);

      await eventsDelivered();
      svg.undo(false);
      await eventsDelivered();
      expect(svg.getBlockById(loop.id)).toBeNull();
      expect(listText(main.getInputTargetBlock('BODY'))).toBe('b_i b_p');
    });

    it('counts with j around a `for i` loop, whose counter would otherwise hide its own', async () => {
      const { svg, flyout } = await withToolbox(sumToTen());
      expect(diagnosticsNow()).toEqual([]);
      const main = present(svg.getBlockById('b005'), 'main');
      const loop = await dropToolboxLoop(svg, flyout, below(svg, 'b001'));

      expect(listText(main.getInputTargetBlock('BODY'))).toBe(`b001 ${loop.id}{b003{b002} b004}`);
      // Not i: the inner `for i` would hide it (B2C-W0501).
      expect(loop.getField('VAR')?.getText()).toBe('j');
      expect(diagnosticsNow()).toEqual([]);

      await eventsDelivered();
      svg.undo(false);
      await eventsDelivered();
      expect(svg.getBlockById(loop.id)).toBeNull();
      expect(listText(main.getInputTargetBlock('BODY'))).toBe('b001 b003{b002} b004');
    });
  },
);
