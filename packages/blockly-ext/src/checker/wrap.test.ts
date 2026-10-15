/**
 * Wrapping statements in an empty C-block (03 §3.3): the rule (`wrapPlace`, `completeWrap`), the
 * connection checker's drag checks for it, and its drag preview (`B2cConnectionPreviewer`), on a
 * rendered Zelos workspace. A pointer drag is stood in for by `setDragging(true)`, which is how
 * Blockly's block drag marks the dragged blocks' connections; the real drags are the desktop
 * app's dragWrap.test.ts and its end-to-end tests.
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';

import { registerB2cMutators } from '../mutators/register';
import { defineTestBlocks } from '../mutators/test-fixtures';
import { B2C_CHECKER_NAME } from './checker';
import {
  B2C_PREVIEWER_NAME,
  B2cConnectionPreviewer,
  previewedWrap,
  registerB2cConnectionPreviewer,
} from './previewer';
import { completeWrap, firstMouth, isEmptyStatementCBlock, wrapPlace } from './wrap';

const FAR = 1e9;

/** Blockly's own checker: what Blockly would decide without the wrap rule. */
const PLAIN = new Blockly.ConnectionChecker();

let host: HTMLDivElement;
let workspace: Blockly.WorkspaceSvg;

beforeAll(() => {
  registerB2cMutators();
  defineTestBlocks();
});

beforeEach(() => {
  host = document.createElement('div');
  document.body.append(host);
  workspace = Blockly.inject(host, {
    renderer: 'zelos',
    sounds: false,
    plugins: { connectionChecker: B2C_CHECKER_NAME, connectionPreviewer: B2C_PREVIEWER_NAME },
  });
});

afterEach(() => {
  Blockly.common.draggingConnections.length = 0;
  workspace.dispose();
  document.body.replaceChildren();
  defineTestBlocks();
});

function rendered(type: string, id?: string): Blockly.BlockSvg {
  const block = workspace.newBlock(type, id);
  block.initSvg();
  block.render();
  return block;
}

function previous(block: Blockly.Block): Blockly.RenderedConnection {
  const connection = block.previousConnection;
  if (!(connection instanceof Blockly.RenderedConnection)) {
    throw new Error(`${block.type} has no previous connection`);
  }
  return connection;
}

function next(block: Blockly.Block): Blockly.RenderedConnection {
  const connection = block.nextConnection;
  if (!(connection instanceof Blockly.RenderedConnection)) {
    throw new Error(`${block.type} has no next connection`);
  }
  return connection;
}

function statements(block: Blockly.Block, name: string): Blockly.RenderedConnection {
  const connection = block.getInput(name)?.connection;
  if (!(connection instanceof Blockly.RenderedConnection)) {
    throw new Error(`${block.type} has no statement input ${name}`);
  }
  return connection;
}

/**
 * `program.main` holding the prints a, b and c, as the canvas has them, and an empty `if` that is
 * not connected (the block a drag would move).
 */
function list(): {
  main: Blockly.BlockSvg;
  a: Blockly.BlockSvg;
  b: Blockly.BlockSvg;
  c: Blockly.BlockSvg;
  cBlock: Blockly.BlockSvg;
} {
  const main = rendered('program.main', 'main');
  const a = rendered('io.print', 'a');
  const b = rendered('io.print', 'b');
  const c = rendered('io.print', 'c');
  statements(main, 'BODY').connect(previous(a));
  next(a).connect(previous(b));
  next(b).connect(previous(c));
  const cBlock = rendered('control.if', 'C');
  return { main, a, b, c, cBlock };
}

/** `main`'s body as text: IDs, a C-block's first mouth in braces, a marker as `M`. */
function body(main: Blockly.Block): string {
  const text = (first: Blockly.Block | null): string => {
    const parts: string[] = [];
    for (let block = first; block !== null; block = block.getNextBlock()) {
      const name = block.isInsertionMarker() ? 'M' : block.id;
      const mouth = firstMouth(block);
      parts.push(mouth === null ? name : `${name}{${text(mouth.targetBlock())}}`);
    }
    return parts.join(' ');
  };
  return text(main.getInputTargetBlock('BODY'));
}

/** Marks `block` as dragged with the pointer, as Blockly's block drag does. */
function drag(block: Blockly.BlockSvg): void {
  block.setDragging(true);
}

describe('empty statement C-blocks', () => {
  it('are statements of the catalog with a first mouth that holds nothing', () => {
    const cBlock = rendered('control.if');
    expect(firstMouth(cBlock)).toBe(cBlock.getInput('DO0')?.connection);
    expect(isEmptyStatementCBlock(cBlock)).toBe(true);
    expect(isEmptyStatementCBlock(rendered('control.forever'))).toBe(true);
    statements(cBlock, 'DO0').connect(previous(rendered('io.print')));
    expect(isEmptyStatementCBlock(cBlock)).toBe(false);
    // No mouth, and a definition (it has no previous connection and never wraps).
    expect(firstMouth(rendered('io.print'))).toBeNull();
    expect(isEmptyStatementCBlock(rendered('io.print'))).toBe(false);
    expect(isEmptyStatementCBlock(rendered('program.main'))).toBe(false);
  });
});

describe('wrapPlace', () => {
  it('is the statement held where the dragged C-block’s top would go', () => {
    const { main, a, b, cBlock } = list();
    drag(cBlock);
    expect(wrapPlace(previous(cBlock), next(a))).toEqual({
      block: cBlock,
      mouth: firstMouth(cBlock),
      place: next(a),
      wrapped: b,
    });
    // The first statement of a list.
    expect(wrapPlace(previous(cBlock), statements(main, 'BODY'))?.wrapped).toBe(a);
  });

  it('is nothing where nothing would be wrapped, or for a block not dragged with the pointer', () => {
    const { a, c, cBlock } = list();
    // Not dragged: a keyboard move inserts the C-block as before.
    expect(wrapPlace(previous(cBlock), next(a))).toBeNull();
    drag(cBlock);
    // The end of the list: an insertion.
    expect(wrapPlace(previous(cBlock), next(c))).toBeNull();
    // Not the C-block's previous connection.
    expect(wrapPlace(next(cBlock), next(a))).toBeNull();
  });

  it('is nothing for a C-block that is not empty, or a block without a mouth', () => {
    const { a } = list();
    const full = rendered('control.forever');
    statements(full, 'BODY').connect(previous(rendered('io.print')));
    drag(full);
    expect(wrapPlace(previous(full), next(a))).toBeNull();
    const print = rendered('io.print');
    drag(print);
    expect(wrapPlace(previous(print), next(a))).toBeNull();
  });

  it('is nothing for a statement that cannot move, or one the drag moves', () => {
    const { a, b, cBlock } = list();
    drag(cBlock);
    b.setMovable(false);
    expect(wrapPlace(previous(cBlock), next(a))).toBeNull();
    b.setMovable(true);
    a.setDragging(true);
    expect(wrapPlace(previous(cBlock), next(a))).toBeNull();
  });

  it('looks through its own preview, which stands in the statement’s place', () => {
    const { main, a, b, cBlock } = list();
    drag(cBlock);
    const previewer = new B2cConnectionPreviewer(cBlock);
    previewer.previewConnection(previous(cBlock), next(a));
    expect(body(main)).toBe('a M{b c}');
    expect(wrapPlace(previous(cBlock), next(a))?.wrapped).toBe(b);
    previewer.hidePreview();
  });
});

describe('the connection checker', () => {
  /** Redefines `control.forever` without a next connection, as a C-block can be (a cap). */
  function capBlock(): Blockly.BlockSvg {
    Blockly.Blocks['control.forever'] = {
      init(this: Blockly.Block): void {
        this.appendStatementInput('BODY');
        this.setPreviousStatement(true);
      },
    };
    return rendered('control.forever');
  }

  it('lets a dragged empty C-block take a statement’s place where Blockly would refuse', () => {
    const { a } = list();
    const cap = capBlock();
    // Blockly refuses to insert a block without a next connection between two statements.
    expect(PLAIN.canConnect(previous(cap), next(a), true, FAR)).toBe(false);
    expect(workspace.connectionChecker.canConnect(previous(cap), next(a), true, FAR)).toBe(false);
    drag(cap);
    expect(workspace.connectionChecker.canConnect(previous(cap), next(a), true, FAR)).toBe(true);
  });

  it('keeps the distance of a drag', () => {
    const { a } = list();
    const cap = capBlock();
    drag(cap);
    const distance = previous(cap).distanceFrom(next(a));
    expect(workspace.connectionChecker.canConnect(previous(cap), next(a), true, distance)).toBe(
      true,
    );
    expect(workspace.connectionChecker.canConnect(previous(cap), next(a), true, distance - 1)).toBe(
      false,
    );
  });

  it('refuses a wrap whose statements may not go into the mouth', () => {
    const { a, cBlock } = list();
    drag(cBlock);
    expect(workspace.connectionChecker.canConnect(previous(cBlock), next(a), true, FAR)).toBe(true);
    statements(cBlock, 'DO0').setCheck('nothing');
    previous(present(a.getNextBlock())).setCheck('statement');
    // Blockly would insert the C-block, with the statements below it.
    expect(PLAIN.canConnect(previous(cBlock), next(a), true, FAR)).toBe(true);
    expect(workspace.connectionChecker.canConnect(previous(cBlock), next(a), true, FAR)).toBe(
      false,
    );
  });
});

describe('the wrap preview', () => {
  it('is registered as b2c_previewer', () => {
    registerB2cConnectionPreviewer();
    expect(
      Blockly.registry.getClass(Blockly.registry.Type.CONNECTION_PREVIEWER, B2C_PREVIEWER_NAME),
    ).toBe(B2cConnectionPreviewer);
  });

  it('puts a marker in the statements’ place and them in its mouth, and puts them back', () => {
    const { main, a, b, c, cBlock } = list();
    const before = Blockly.serialization.workspaces.save(workspace);
    drag(cBlock);
    const fired: Blockly.Events.Abstract[] = [];
    workspace.addChangeListener((event) => fired.push(event));
    const previewer = new B2cConnectionPreviewer(cBlock);

    previewer.previewConnection(previous(cBlock), next(a));
    expect(body(main)).toBe('a M{b c}');
    expect(previewer.shownWrap()?.wrapped).toBe(b);
    expect(previewedWrap(cBlock)?.place).toBe(next(a));
    // The same place again changes nothing.
    const marker = b.getParent();
    previewer.previewConnection(previous(cBlock), next(a));
    expect(b.getParent()).toBe(marker);

    previewer.hidePreview();
    expect(body(main)).toBe('a b c');
    expect(marker?.isDisposed()).toBe(true);
    expect(previewer.shownWrap()).toBeNull();
    for (const block of [a, b, c]) {
      expect(block.isDisposed()).toBe(false);
    }
    cBlock.setDragging(false);
    expect(Blockly.serialization.workspaces.save(workspace)).toEqual(before);
    // Previews are not changes to the program.
    expect(fired.filter((event) => !event.isUiEvent)).toEqual([]);
  });

  it('moves from one wrap to another and to Blockly’s own preview', () => {
    const { main, a, b, c, cBlock } = list();
    drag(cBlock);
    const previewer = new B2cConnectionPreviewer(cBlock);
    previewer.previewConnection(previous(cBlock), statements(main, 'BODY'));
    expect(body(main)).toBe('M{a b c}');
    previewer.previewConnection(previous(cBlock), next(b));
    expect(body(main)).toBe('a b M{c}');
    // The end of the list: Blockly's insertion preview.
    previewer.previewConnection(previous(cBlock), next(c));
    expect(body(main)).toBe('a b c M{}');
    previewer.previewConnection(previous(cBlock), next(a));
    expect(body(main)).toBe('a M{b c}');
    previewer.dispose();
    expect(body(main)).toBe('a b c');
    expect(workspace.getAllBlocks(false).filter((block) => block.isInsertionMarker())).toEqual([]);
  });

  it('draws its marker once more after unplugging it, before it disposes it', () => {
    // Blockly 12.5.1's render queue keeps for good a block it was to draw and never did; an
    // unplugged marker is queued, so a marker disposed undrawn would never be freed.
    const { a, b, cBlock } = list();
    drag(cBlock);
    const previewer = new B2cConnectionPreviewer(cBlock);
    previewer.previewConnection(previous(cBlock), next(a));
    const marker = b.getParent();
    if (!(marker instanceof Blockly.BlockSvg) || !marker.isInsertionMarker()) {
      throw new Error('the preview has no marker');
    }
    const drawn = vi.spyOn(marker, 'renderEfficiently');
    const disposed = vi.spyOn(marker, 'dispose');

    previewer.hidePreview();

    expect(disposed).toHaveBeenCalledOnce();
    const lastDrawn = drawn.mock.invocationCallOrder.at(-1);
    expect(lastDrawn).toBeDefined();
    expect(lastDrawn).toBeLessThan(disposed.mock.invocationCallOrder[0] ?? 0);
  });

  it('sets loose, never disposes, a block left attached to its marker', () => {
    const { main, a, cBlock } = list();
    drag(cBlock);
    const previewer = new B2cConnectionPreviewer(cBlock);
    previewer.previewConnection(previous(cBlock), next(a));
    const marker = present(a.getNextBlock());
    // A block that should not be there, below the marker.
    const stray = rendered('io.print', 'stray');
    Blockly.Events.disable();
    next(marker).connect(previous(stray));
    Blockly.Events.enable();
    const errors: unknown[] = [];
    const log = console.error;
    console.error = (...args: unknown[]) => errors.push(args);
    try {
      previewer.hidePreview();
    } finally {
      console.error = log;
    }
    expect(body(main)).toBe('a b c');
    expect(stray.isDisposed()).toBe(false);
    expect(stray.getParent()).toBeNull();
    expect(errors).toHaveLength(1);
  });

  it('is shown for a pair the checker allows as a wrap only', () => {
    const { main, a, cBlock } = list();
    // Not dragged with the pointer: Blockly's insertion preview.
    const previewer = new B2cConnectionPreviewer(cBlock);
    previewer.previewConnection(previous(cBlock), next(a));
    expect(body(main)).toBe('a M{} b c');
    expect(previewer.shownWrap()).toBeNull();
    previewer.hidePreview();
    expect(body(main)).toBe('a b c');
  });
});

describe('completeWrap', () => {
  it('moves the statements Blockly hung below the C-block into its mouth', () => {
    const { main, a, b, cBlock } = list();
    drag(cBlock);
    const wrap = present(wrapPlace(previous(cBlock), next(a)));
    cBlock.setDragging(false);
    // Blockly's drop: the C-block at the place, the statements that were there below it.
    previous(cBlock).connect(next(a));
    expect(body(main)).toBe('a C{} b c');
    expect(completeWrap(wrap)).toBe(true);
    expect(body(main)).toBe('a C{b c}');
    expect(b.getParent()).toBe(cBlock);
  });

  it('moves loose statements into the mouth (a C-block without a next connection)', () => {
    const { main, a, b, cBlock } = list();
    drag(cBlock);
    const wrap = present(wrapPlace(previous(cBlock), next(a)));
    cBlock.setDragging(false);
    previous(b).disconnect();
    previous(cBlock).connect(next(a));
    expect(completeWrap(wrap)).toBe(true);
    expect(body(main)).toBe('a C{b c}');
  });

  it('changes nothing unless the drop put the C-block in the place with the statements below', () => {
    const { main, a, b, cBlock } = list();
    drag(cBlock);
    const wrap = present(wrapPlace(previous(cBlock), next(a)));
    cBlock.setDragging(false);
    // Dropped loose (or deleted): nothing to finish.
    expect(completeWrap(wrap)).toBe(false);
    expect(body(main)).toBe('a b c');
    // Somewhere else.
    next(present(b.getNextBlock())).connect(previous(cBlock));
    expect(completeWrap(wrap)).toBe(false);
    expect(body(main)).toBe('a b c C{}');
    // In the place, but the statements are elsewhere (b moved into another list).
    previous(cBlock).disconnect();
    previous(cBlock).connect(next(a));
    const other = rendered('control.forever');
    statements(other, 'BODY').connect(previous(b));
    expect(completeWrap(wrap)).toBe(false);
    expect(body(main)).toBe('a C{}');
    // Gone.
    b.dispose(false);
    expect(completeWrap(wrap)).toBe(false);
  });

  it('changes nothing when the C-block is no longer empty', () => {
    const { main, a, cBlock } = list();
    drag(cBlock);
    const wrap = present(wrapPlace(previous(cBlock), next(a)));
    cBlock.setDragging(false);
    previous(cBlock).connect(next(a));
    statements(cBlock, 'DO0').connect(previous(rendered('io.print', 'inner')));
    expect(completeWrap(wrap)).toBe(false);
    expect(body(main)).toBe('a C{inner} b c');
  });
});

function present<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) {
    throw new Error('missing');
  }
  return value;
}
