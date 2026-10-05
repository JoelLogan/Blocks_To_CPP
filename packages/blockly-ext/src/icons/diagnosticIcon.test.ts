/**
 * The diagnostics badge on real catalog blocks, rendered with Zelos in happy-dom: where it goes,
 * what it shows, the outline and part marks, and that it never edits the project (no events, not
 * saved, no layout change).
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';

import { expectNoAxeViolations } from '../../test/axe';
import { headlessWorkspace, renderedWorkspace } from '../../test/helpers';
import { registerB2cBlocks } from '../blocks/register';
import { installIdGenerator } from '../ids';
import { registerB2cMutators } from '../mutators/register';
import { exprShadowState, getTokenHighlight } from '../shadows/state';
import { DIAGNOSTIC_CSS_CLASS } from './css';
import {
  BADGE_SIZE,
  DIAGNOSTIC_ICON_TYPE,
  getBlockDiagnostics,
  registerDiagnosticIcon,
  setBlockDiagnostics,
} from './diagnosticIcon';
import { partMarkKeys } from './parts';
import type { BlockDiagnosticItem } from './summary';

beforeAll(() => {
  installIdGenerator();
  registerB2cMutators();
  registerB2cBlocks();
  registerDiagnosticIcon();
});

afterEach(() => {
  vi.useRealTimers();
});

const ERROR: BlockDiagnosticItem = {
  code: 'B2C-E0201',
  severity: 'error',
  message: 'This uses a variable that does not exist here.',
  primary: { part: { kind: 'whole' } },
};

const WARNING: BlockDiagnosticItem = {
  code: 'B2C-W0501',
  severity: 'warning',
  message: 'This value is never used.',
  primary: { part: { kind: 'whole' } },
};

function rendered(workspace: Blockly.WorkspaceSvg, type: string): Blockly.BlockSvg {
  const block = workspace.newBlock(type);
  block.initSvg();
  block.render();
  return block;
}

/** Renders everything Blockly has queued (badges added or removed queue a render). */
function flushRenders(): void {
  Blockly.renderManagement.triggerQueuedRenders();
}

function badgeRoot(block: Blockly.BlockSvg): SVGGElement {
  const icon = block.getIcon(DIAGNOSTIC_ICON_TYPE);
  const root = icon?.getFocusableElement();
  if (!(root instanceof SVGGElement)) {
    throw new Error('the block has no badge');
  }
  return root;
}

/** A `control.while` whose COND holds a read-only expression shadow of `tokens`. */
function whileWithTokens(workspace: Blockly.Workspace): Blockly.Block {
  const loop = workspace.newBlock('control.while');
  loop
    .getInput('COND')
    ?.connection?.setShadowState(
      exprShadowState(
        [{ ref: 's_guess' }, { op: '==' }, { ref: 's_secret' }],
        false,
        'bool',
        false,
      ),
    );
  return loop;
}

describe('the badge on a rendered block', () => {
  it('shows the most serious severity with its own shape, an accessible name and a tooltip', () => {
    const workspace = renderedWorkspace();
    const block = rendered(workspace, 'var.set');
    setBlockDiagnostics(block, [WARNING, ERROR]);
    flushRenders();

    expect(getBlockDiagnostics(block)?.severity).toBe('error');
    const root = badgeRoot(block);
    expect(root.classList.contains(DIAGNOSTIC_CSS_CLASS.badge)).toBe(true);
    expect(root.classList.contains(DIAGNOSTIC_CSS_CLASS.badgeSeverity.error)).toBe(true);
    expect(root.classList.contains(DIAGNOSTIC_CSS_CLASS.badgeSeverity.warning)).toBe(false);
    expect(root.getAttribute('role')).toBe('img');
    expect(root.getAttribute('aria-label')).toBe(
      'Error: This uses a variable that does not exist here., and 1 more problem',
    );
    expect(root.querySelector('circle')).not.toBeNull();
    expect(block.getIcon(DIAGNOSTIC_ICON_TYPE)?.getTooltip()).toBe(
      '✖ Error: This uses a variable that does not exist here. (B2C-E0201)\n' +
        '⚠ Warning: This value is never used. (B2C-W0501)',
    );

    setBlockDiagnostics(block, [WARNING]);
    expect(root.classList.contains(DIAGNOSTIC_CSS_CLASS.badgeSeverity.warning)).toBe(true);
    expect(root.classList.contains(DIAGNOSTIC_CSS_CLASS.badgeSeverity.error)).toBe(false);
    // The warning is a triangle, not a circle: the shape differs as well as the colour.
    expect(root.querySelector('circle')).toBeNull();
    expect(root.querySelector('path')).not.toBeNull();
  });

  it('draws only SVG, never HTML, whatever the message says', () => {
    const workspace = renderedWorkspace();
    const block = rendered(workspace, 'var.set');
    setBlockDiagnostics(block, [{ ...ERROR, message: '<img src=x onerror=alert(1)>' }]);
    const root = badgeRoot(block);
    const html = [...root.querySelectorAll('*')].filter(
      (element) => element.namespaceURI !== 'http://www.w3.org/2000/svg',
    );
    expect(html).toEqual([]);
    expect(root.querySelector('img')).toBeNull();
    expect(root.getAttribute('aria-label')).toBe('Error: <img src=x onerror=alert(1)>');
  });

  it('sits at the top-right corner without moving the block content', () => {
    const workspace = renderedWorkspace();
    const block = rendered(workspace, 'control.while');
    const fieldPositions = () =>
      block.inputList
        .flatMap((input) => input.fieldRow)
        .map((field) => field.getSvgRoot()?.getAttribute('transform'));
    const before = fieldPositions();
    const width = block.width;

    setBlockDiagnostics(block, [ERROR]);
    flushRenders();

    // Every field, including the first label ("repeat"), stays where it was.
    expect(before.length).toBeGreaterThan(1);
    expect(fieldPositions()).toEqual(before);
    expect(block.width).toBe(width);
    const transform = badgeRoot(block).getAttribute('transform') ?? '';
    const match = /translate\(([-\d.]+), ([-\d.]+)\)/.exec(transform);
    const x = Number(match?.[1]);
    const y = Number(match?.[2]);
    // The badge covers the corner: its right edge is just past the block's right edge.
    expect(x + BADGE_SIZE).toBeGreaterThan(block.width);
    expect(x).toBeLessThan(block.width);
    expect(y).toBeLessThan(0);
  });

  it('tints the outline by severity, dims stale-only diagnostics, and removes everything', () => {
    const workspace = renderedWorkspace();
    const block = rendered(workspace, 'var.set');
    const group = block.getSvgRoot();

    setBlockDiagnostics(block, [{ ...ERROR, stale: true }]);
    expect(group.classList.contains(DIAGNOSTIC_CSS_CLASS.block.error)).toBe(true);
    expect(group.classList.contains(DIAGNOSTIC_CSS_CLASS.blockStale)).toBe(true);
    expect(badgeRoot(block).classList.contains(DIAGNOSTIC_CSS_CLASS.badgeStale)).toBe(true);

    setBlockDiagnostics(block, [{ ...ERROR, stale: true }, WARNING]);
    expect(group.classList.contains(DIAGNOSTIC_CSS_CLASS.blockStale)).toBe(false);
    expect(badgeRoot(block).classList.contains(DIAGNOSTIC_CSS_CLASS.badgeStale)).toBe(false);

    setBlockDiagnostics(block, []);
    flushRenders();
    expect(block.getIcon(DIAGNOSTIC_ICON_TYPE)).toBeUndefined();
    expect(getBlockDiagnostics(block)).toBeNull();
    for (const className of Object.values(DIAGNOSTIC_CSS_CLASS.block)) {
      expect(group.classList.contains(className)).toBe(false);
    }
    expect(group.querySelector(`.${DIAGNOSTIC_CSS_CLASS.badge}`)).toBeNull();
  });

  it('stays visible when the block is collapsed', () => {
    const workspace = renderedWorkspace();
    const block = rendered(workspace, 'control.while');
    setBlockDiagnostics(block, [ERROR]);
    block.setCollapsed(true);
    flushRenders();
    expect(badgeRoot(block).style.display).not.toBe('none');
  });

  it('marks the field a diagnostic points at, and unmarks it when it goes', () => {
    const workspace = renderedWorkspace();
    const block = rendered(workspace, 'control.while');
    const fieldRoot = block.getField('MODE')?.getSvgRoot();

    setBlockDiagnostics(block, [
      { ...WARNING, primary: { part: { kind: 'field', name: 'MODE' } } },
    ]);
    expect(fieldRoot?.classList.contains(DIAGNOSTIC_CSS_CLASS.field.warning)).toBe(true);
    expect(partMarkKeys(block)).toEqual(['field:MODE:warning']);

    // A more serious diagnostic on the same field wins.
    setBlockDiagnostics(block, [
      { ...WARNING, primary: { part: { kind: 'field', name: 'MODE' } } },
      { ...ERROR, primary: { part: { kind: 'field', name: 'MODE' } } },
    ]);
    expect(fieldRoot?.classList.contains(DIAGNOSTIC_CSS_CLASS.field.error)).toBe(true);
    expect(fieldRoot?.classList.contains(DIAGNOSTIC_CSS_CLASS.field.warning)).toBe(false);

    setBlockDiagnostics(block, [ERROR]);
    expect(fieldRoot?.classList.contains(DIAGNOSTIC_CSS_CLASS.field.error)).toBe(false);
    expect(partMarkKeys(block)).toEqual([]);
  });

  it('marks the block in an input, and ignores parts the block does not have', () => {
    const workspace = renderedWorkspace();
    const loop = whileWithTokens(workspace) as Blockly.BlockSvg;
    loop.initSvg();
    loop.render();
    const shadow = loop.getInputTargetBlock('COND') as Blockly.BlockSvg;

    setBlockDiagnostics(loop, [
      { ...ERROR, primary: { part: { kind: 'input', name: 'COND' } } },
      { ...ERROR, primary: { part: { kind: 'input', name: 'NOPE' } } },
      { ...ERROR, primary: { part: { kind: 'field', name: 'NOPE' } } },
    ]);
    expect(shadow.getSvgRoot().classList.contains(DIAGNOSTIC_CSS_CLASS.input.error)).toBe(true);
    expect(partMarkKeys(loop)).toEqual(['input:COND:error']);

    setBlockDiagnostics(loop, []);
    expect(shadow.getSvgRoot().classList.contains(DIAGNOSTIC_CSS_CLASS.input.error)).toBe(false);
  });

  it('underlines a token range in the read-only expression shadow', () => {
    const workspace = renderedWorkspace();
    const loop = whileWithTokens(workspace) as Blockly.BlockSvg;
    loop.initSvg();
    loop.render();
    const shadow = loop.getInputTargetBlock('COND') as Blockly.BlockSvg;

    setBlockDiagnostics(loop, [
      { ...ERROR, primary: { part: { kind: 'tokens', input: 'COND', start: 2, end: 3 } } },
    ]);
    expect(getTokenHighlight(shadow)).toEqual({ start: 2, end: 3 });
    expect(shadow.getSvgRoot().classList.contains(DIAGNOSTIC_CSS_CLASS.input.error)).toBe(true);

    setBlockDiagnostics(loop, [WARNING]);
    expect(getTokenHighlight(shadow)).toBeNull();
    expect(shadow.getSvgRoot().classList.contains(DIAGNOSTIC_CSS_CLASS.input.error)).toBe(false);
  });

  it('marks nothing for diagnostics of blocks inside (a collapsed block stands for them)', () => {
    const workspace = renderedWorkspace();
    const block = rendered(workspace, 'control.while');
    setBlockDiagnostics(block, [
      { ...ERROR, nested: true, primary: { part: { kind: 'field', name: 'MODE' } } },
    ]);
    expect(getBlockDiagnostics(block)?.severity).toBe('error');
    expect(partMarkKeys(block)).toEqual([]);
  });

  it('fires no Blockly events and is never saved', () => {
    const workspace = renderedWorkspace();
    const loop = whileWithTokens(workspace) as Blockly.BlockSvg;
    loop.initSvg();
    loop.render();
    const events: string[] = [];
    workspace.addChangeListener((event) => {
      events.push(event.type);
    });
    const saved = JSON.stringify(Blockly.serialization.blocks.save(loop));

    setBlockDiagnostics(loop, [
      ERROR,
      { ...ERROR, primary: { part: { kind: 'tokens', input: 'COND', start: 0, end: 1 } } },
    ]);
    flushRenders();
    setBlockDiagnostics(loop, []);
    flushRenders();

    expect(events).toEqual([]);
    expect(Blockly.Events.isEnabled()).toBe(true);
    expect(workspace.getUndoStack()).toEqual([]);
    setBlockDiagnostics(loop, [ERROR]);
    expect(JSON.stringify(Blockly.serialization.blocks.save(loop))).toBe(saved);
  });

  it('shows its messages in Blockly tooltip as plain text, and passes axe', async () => {
    vi.useFakeTimers();
    const workspace = renderedWorkspace();
    const block = rendered(workspace, 'var.set');
    setBlockDiagnostics(block, [{ ...ERROR, message: 'Uses <b>score</b>\u202e here.' }]);
    flushRenders();
    const root = badgeRoot(block);

    root.dispatchEvent(new PointerEvent('pointerover', { bubbles: true }));
    root.dispatchEvent(new PointerEvent('pointermove', { bubbles: true, clientX: 5, clientY: 5 }));
    vi.advanceTimersByTime(2000);

    const tooltip = Blockly.Tooltip.getDiv();
    expect(tooltip?.textContent).toBe('✖ Error: Uses <b>score</b>⟨U+202E⟩ here. (B2C-E0201)');
    expect(tooltip?.querySelector('b')).toBeNull();
    vi.useRealTimers();
    if (tooltip !== null) {
      await expectNoAxeViolations(tooltip);
    }
    await expectNoAxeViolations(root);
    Blockly.Tooltip.hide();
  });

  it('ignores disposed blocks and unknown severities', () => {
    const workspace = renderedWorkspace();
    const block = rendered(workspace, 'var.set');
    setBlockDiagnostics(block, [{ ...ERROR, severity: 'fatal' } as unknown as BlockDiagnosticItem]);
    expect(block.getIcon(DIAGNOSTIC_ICON_TYPE)).toBeUndefined();
    block.dispose();
    expect(() => {
      setBlockDiagnostics(block, [ERROR]);
    }).not.toThrow();
  });
});

describe('the badge on a headless workspace', () => {
  it('keeps the summary and the token marks without any SVG', () => {
    const workspace = headlessWorkspace();
    const loop = whileWithTokens(workspace);
    const shadow = loop.getInputTargetBlock('COND');
    setBlockDiagnostics(loop, [
      WARNING,
      { ...ERROR, primary: { part: { kind: 'tokens', input: 'COND', start: 0, end: 1 } } },
    ]);
    expect(getBlockDiagnostics(loop)?.severity).toBe('error');
    expect(shadow === null ? null : getTokenHighlight(shadow)).toEqual({ start: 0, end: 1 });

    setBlockDiagnostics(loop, []);
    expect(getBlockDiagnostics(loop)).toBeNull();
    expect(shadow === null ? null : getTokenHighlight(shadow)).toBeNull();
  });
});
