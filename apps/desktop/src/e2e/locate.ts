/**
 * Finding Blockly's parts on screen for the end-to-end tests. Blockly draws blocks into SVG and
 * keeps its model in JavaScript, so a WebDriver test cannot see which SVG group is which field, or
 * where a connection is. These helpers answer that from Blockly's model, for blocks on the canvas
 * and in the toolbox's flyout, so the tests can then click and drag with real pointer input.
 *
 * Points are client (viewport) coordinates in CSS pixels, which are also WebDriver's pointer
 * coordinates when the window's device pixel ratio is 1.
 */
import * as Blockly from 'blockly/core';

/** A point in client coordinates. */
export interface ClientPoint {
  readonly x: number;
  readonly y: number;
}

/** What `document.elementFromPoint` gives (stubbed in unit tests). */
export type ElementAtPoint = (x: number, y: number) => Element | null;

/** How far apart the points {@link grabPoint} tries are, in CSS pixels. */
const GRAB_STEP = 3;

/** The most points {@link grabPoint} tries. */
const MAX_GRAB_PROBES = 4000;

/** The workspaces a block can be on: the canvas, then the toolbox's flyout. */
function workspacesOf(workspace: Blockly.WorkspaceSvg): Blockly.WorkspaceSvg[] {
  const flyout = workspace.getFlyout()?.getWorkspace() ?? null;
  return flyout === null ? [workspace] : [workspace, flyout];
}

/** A rendered block with this ID on the canvas or in the flyout, or `null`. */
export function findBlock(workspace: Blockly.WorkspaceSvg, id: string): Blockly.BlockSvg | null {
  for (const candidate of workspacesOf(workspace)) {
    const block = candidate.getBlockById(id);
    if (block instanceof Blockly.BlockSvg && !block.isDeadOrDying()) {
      return block;
    }
  }
  return null;
}

/** The SVG group of a block (with everything drawn inside it), or `null`. */
export function blockElement(workspace: Blockly.WorkspaceSvg, id: string): Element | null {
  return findBlock(workspace, id)?.getSvgRoot() ?? null;
}

/**
 * The SVG group of field `field` of a block, or, with `input`, of the block in that input (an
 * expression slot's `VALUE`, for example). `null` when there is no such field or it is hidden.
 */
export function fieldElement(
  workspace: Blockly.WorkspaceSvg,
  blockId: string,
  field: string,
  input?: string,
): Element | null {
  let block: Blockly.Block | null = findBlock(workspace, blockId);
  if (block !== null && input !== undefined) {
    block = block.getInputTargetBlock(input);
  }
  const found = block?.getField(field) ?? null;
  return found?.isVisible() === true ? found.getSvgRoot() : null;
}

/**
 * The ID of the first top-level block of `type` in the toolbox's flyout (in the flyout's order)
 * whose fields have the given values, such as the *repeat until* entry of `control.while` with
 * `{MODE: 'until'}`; `null` when there is none.
 */
export function flyoutBlockId(
  workspace: Blockly.WorkspaceSvg,
  type: string,
  fields: Readonly<Record<string, string>> = {},
): string | null {
  const flyout = workspace.getFlyout()?.getWorkspace() ?? null;
  if (flyout === null) {
    return null;
  }
  const wanted = Object.entries(fields);
  for (const block of flyout.getTopBlocks(true)) {
    if (
      block.type === type &&
      wanted.every(([name, value]) => String(block.getFieldValue(name)) === value)
    ) {
      return block.id;
    }
  }
  return null;
}

/**
 * Where a connection of a block is on screen: `previous`, `next` or `output`, or the name of one of
 * its inputs. `null` when the block or the connection does not exist.
 */
export function connectionPoint(
  workspace: Blockly.WorkspaceSvg,
  blockId: string,
  connection: string,
): ClientPoint | null {
  const block = findBlock(workspace, blockId);
  if (block === null) {
    return null;
  }
  let found: Blockly.Connection | null;
  switch (connection) {
    case 'previous':
      found = block.previousConnection;
      break;
    case 'next':
      found = block.nextConnection;
      break;
    case 'output':
      found = block.outputConnection;
      break;
    default:
      found = block.getInput(connection)?.connection ?? null;
  }
  if (found === null) {
    return null;
  }
  const point = Blockly.utils.svgMath.wsToScreenCoordinates(
    block.workspace,
    new Blockly.utils.Coordinate(found.x, found.y),
  );
  return { x: point.x, y: point.y };
}

/**
 * A point where pressing grabs the block itself: on the block's own outline, not on one of its
 * fields or on a block inside it, and not covered by anything else (the point is checked with
 * `elementFromPoint`). Points are tried row by row from the block's top left corner, so the point
 * is near the block's start. `null` when the block is not on screen or no such point was found.
 */
export function grabPoint(
  workspace: Blockly.WorkspaceSvg,
  blockId: string,
  elementAt: ElementAtPoint,
): ClientPoint | null {
  const block = findBlock(workspace, blockId);
  if (block === null) {
    return null;
  }
  const outline = block.pathObject.svgPath;
  const box = outline.getBoundingClientRect();
  // Whole pixels: WebDriver's pointer moves to integer coordinates.
  const left = Math.ceil(box.left) + GRAB_STEP;
  const top = Math.ceil(box.top) + GRAB_STEP;
  let probes = 0;
  for (let y = top; y < box.bottom; y += GRAB_STEP) {
    for (let x = left; x < box.right; x += GRAB_STEP) {
      probes += 1;
      if (probes > MAX_GRAB_PROBES) {
        return null;
      }
      if (elementAt(x, y) === outline) {
        return { x, y };
      }
    }
  }
  return null;
}
