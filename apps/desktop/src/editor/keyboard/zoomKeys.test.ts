/**
 * The zoom keys of the block editor (docs/spec/04-user-interface.md §4.7, decision D10 of the M2
 * polish plan): `Ctrl+=` (and `Ctrl++`, also on the keypad) and `Ctrl+-` zoom the canvas one step,
 * `Ctrl+0` resets it to 100 %, and each announces the new zoom; the plain keys and a held block
 * leave the zoom alone.
 */
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { FLYOUT_TEST_TIMEOUT_MS } from '../toolbox/testing';
import { KEY_MAP } from './help';
import { type KeyboardEditor, keyboardEditor, press } from './testing';

// Each test here opens an editor whose toolbox shows the flyout of every catalog block: with W2's
// control and file blocks, the slowest took 4.4 s in the suite with two workers and coverage on a
// busy machine, close to Vitest's default of 5 s (3.1 s alone).
vi.setConfig({ testTimeout: FLYOUT_TEST_TIMEOUT_MS });

const editors: KeyboardEditor[] = [];

afterEach(() => {
  for (const editor of editors.splice(0)) {
    editor.dispose();
  }
  Blockly.keyboardNavigationController.setIsActive(false);
});

/** The guessing game in the editor, with the keyboard focus on its `print` block. */
function openOnBlock(): KeyboardEditor {
  const editor = keyboardEditor();
  editors.push(editor);
  Blockly.getFocusManager().focusNode(editor.block('print'));
  return editor;
}

/** The `keyCode`s of the zoom keys, as WebKitGTK and WebView2 report them. */
const KEYS = {
  equals: { key: '=', keyCode: 187 },
  plus: { key: '+', keyCode: 187 },
  numpadPlus: { key: '+', keyCode: 107 },
  minus: { key: '-', keyCode: 189 },
  numpadMinus: { key: '-', keyCode: 109 },
  zero: { key: '0', keyCode: 48 },
  numpadZero: { key: '0', keyCode: 96 },
} as const;

/** Presses a zoom key on the focused element, with Ctrl (and Shift) as given. */
function pressZoomKey(
  which: keyof typeof KEYS,
  modifiers: { ctrlKey?: boolean; shiftKey?: boolean } = { ctrlKey: true },
): KeyboardEvent {
  const event = new KeyboardEvent('keydown', {
    ...KEYS[which],
    ...modifiers,
    bubbles: true,
    cancelable: true,
  });
  (document.activeElement ?? document.body).dispatchEvent(event);
  return event;
}

describe('the zoom keys', () => {
  it('zoom in by one step with Ctrl+=, Ctrl++ and the keypad’s Ctrl++, saying the new zoom', () => {
    const { workspace, keyboard } = openOnBlock();
    expect(workspace.scale).toBe(0.9);

    const event = pressZoomKey('equals');
    expect(workspace.scale).toBeCloseTo(1.08, 10);
    expect(keyboard.announcer.last).toBe('Zoom 108%');
    // The webview zooms nothing else.
    expect(event.defaultPrevented).toBe(true);

    pressZoomKey('plus', { ctrlKey: true, shiftKey: true });
    expect(workspace.scale).toBeCloseTo(0.9 * 1.44, 10);
    pressZoomKey('numpadPlus');
    expect(workspace.scale).toBeCloseTo(0.9 * 1.728, 10);
    expect(keyboard.announcer.last).toBe('Zoom 156%');
  });

  it('zoom out by one step with Ctrl+- and the keypad’s Ctrl+-', () => {
    const { workspace, keyboard } = openOnBlock();
    pressZoomKey('minus');
    expect(workspace.scale).toBeCloseTo(0.75, 10);
    expect(keyboard.announcer.last).toBe('Zoom 75%');
    pressZoomKey('numpadMinus');
    expect(workspace.scale).toBeCloseTo(0.625, 10);
  });

  it('reset the zoom to 100 % with Ctrl+0, also on the keypad', () => {
    const { workspace, keyboard } = openOnBlock();
    pressZoomKey('minus');
    pressZoomKey('zero');
    expect(workspace.scale).toBe(1);
    expect(keyboard.announcer.last).toBe('Zoom 100%');
    workspace.setScale(2);
    pressZoomKey('numpadZero');
    expect(workspace.scale).toBe(1);
  });

  it('work with the focus on the canvas itself', () => {
    const { workspace } = openOnBlock();
    const canvas = workspace.getSvgGroup();
    if (!(canvas instanceof SVGElement)) {
      throw new Error('the canvas is not an SVG element');
    }
    canvas.focus();
    pressZoomKey('equals');
    expect(workspace.scale).toBeCloseTo(1.08, 10);
  });

  it('leave the zoom alone without Ctrl and while a block is held', () => {
    const { workspace, keyboard } = openOnBlock();
    const plain = pressZoomKey('equals', {});
    expect(workspace.scale).toBe(0.9);
    expect(plain.defaultPrevented).toBe(false);

    // A held block takes every key (moving it, or the move hint).
    press('m');
    expect(keyboard.mover.active).toBe(true);
    pressZoomKey('equals');
    expect(workspace.scale).toBe(0.9);
    keyboard.mover.cancel();
  });

  it('are in the key map the canvas describes and the checklist lists', () => {
    expect(KEY_MAP).toEqual(
      expect.arrayContaining([
        { scope: 'canvas', keys: 'Ctrl+= / Ctrl+-', action: 'Zoom in or out' },
        { scope: 'canvas', keys: 'Ctrl+0', action: 'Reset the zoom to 100%' },
      ]),
    );
  });
});
