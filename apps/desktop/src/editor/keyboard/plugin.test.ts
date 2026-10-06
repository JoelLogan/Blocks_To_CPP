/**
 * The keyboard editor plugin's attachment against the real Blockly: names and descriptions of the
 * canvas and the toolbox's blocks, the Tab order, the shortcut registry, keyboard mode, the end of
 * a move when another project opens, reduced motion and clean-up.
 */
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { useAppStore } from '../../app/store';
import { projectFixture } from '../../app/testing/fixtures';
import { expectNoAxeViolations } from '../../test/axe';
import { ANNOUNCER_CLASS, Announcer, MAX_ANNOUNCEMENT_CHARS } from './announcer';
import {
  CANVAS_DESCRIPTION,
  CANVAS_LABEL,
  FLYOUT_DESCRIPTION,
  FLYOUT_LABEL,
  KEY_MAP,
} from './help';
import { attachReducedMotion, prefersReducedMotion, REDUCED_MOTION_QUERY } from './motion';
import { keyboardPlugin } from './plugin';
import { KEYBOARD_SHORTCUT_NAMES } from './shortcuts';
import { type KeyboardEditor, keyboardEditor, press } from './testing';

let editors: KeyboardEditor[] = [];

function open(options: Parameters<typeof keyboardEditor>[0] = {}): KeyboardEditor {
  const editor = keyboardEditor(options);
  editors.push(editor);
  return editor;
}

afterEach(() => {
  for (const editor of editors.splice(0)) {
    editor.dispose();
  }
  Blockly.keyboardNavigationController.setIsActive(false);
});

/** Focuses the canvas's own element, as Tab does. */
function focusCanvas(workspace: Blockly.WorkspaceSvg): void {
  const canvas = workspace.getSvgGroup();
  if (!(canvas instanceof SVGElement)) {
    throw new Error('the canvas is not an SVG element');
  }
  canvas.focus();
}

function registered(): string[] {
  const known = Blockly.ShortcutRegistry.registry.getRegistry();
  return Object.values(KEYBOARD_SHORTCUT_NAMES).filter((name) => known[name] !== undefined);
}

describe('the keyboard plugin', () => {
  it('is the editor plugin named keyboard', () => {
    expect(keyboardPlugin.name).toBe('keyboard');
  });

  it('names the canvas and the toolbox’s blocks and describes their keys', () => {
    const { workspace, keyboard } = open();
    const canvas = workspace.getSvgGroup();
    expect(canvas.getAttribute('aria-label')).toBe(CANVAS_LABEL);
    const described = document.getElementById(canvas.getAttribute('aria-describedby') ?? '');
    expect(described?.textContent).toBe(CANVAS_DESCRIPTION);

    const flyout = keyboard.flyout()?.getWorkspace().getSvgGroup();
    expect(flyout?.getAttribute('aria-label')).toBe(FLYOUT_LABEL);
    const flyoutDescribed = document.getElementById(flyout?.getAttribute('aria-describedby') ?? '');
    expect(flyoutDescribed?.textContent).toBe(FLYOUT_DESCRIPTION);
  });

  it('puts the toolbox’s blocks between the toolbox and the canvas in the Tab order', () => {
    const { workspace, keyboard } = open();
    const injection = workspace.getInjectionDiv();
    const stops: Element[] = [...injection.querySelectorAll('[tabindex="0"]')];
    const toolbox = keyboard.toolbox()?.HtmlDiv;
    const flyout = keyboard.flyout()?.getWorkspace().getSvgGroup();
    const canvas = workspace.getSvgGroup();
    if (toolbox === null || toolbox === undefined || flyout === undefined) {
      throw new Error('no toolbox');
    }
    expect(stops.indexOf(toolbox)).toBeGreaterThanOrEqual(0);
    expect(stops.indexOf(toolbox)).toBeLessThan(stops.indexOf(flyout));
    expect(stops.indexOf(flyout)).toBeLessThan(stops.indexOf(canvas));
  });

  it('registers its keys once for all editors and removes them with the last', () => {
    expect(registered()).toEqual([]);
    const first = open();
    const second = open();
    expect(registered()).toHaveLength(Object.keys(KEYBOARD_SHORTCUT_NAMES).length);
    first.dispose();
    expect(registered()).toHaveLength(Object.keys(KEYBOARD_SHORTCUT_NAMES).length);
    second.dispose();
    expect(registered()).toEqual([]);
    // Blockly's own Escape is still there.
    expect(Blockly.ShortcutRegistry.registry.getRegistry()['escape']).toBeDefined();
  });

  it('sends a key to the editor of the canvas it was pressed on', () => {
    const one = open();
    const two = open();
    Blockly.getFocusManager().focusNode(two.block('decl'));
    press('ArrowDown');
    expect(Blockly.getFocusManager().getFocusedNode()).toBe(two.block('print'));
    expect(two.keyboard.announcer.last).toBe('block “print”');
    expect(one.keyboard.announcer.last).toBe('');
  });

  it('undoes what it changed when detached', () => {
    // A second plugin on an editor's canvas, detached again, leaves the first one's state.
    const { workspace } = open();
    const canvas = workspace.getSvgGroup();
    const injection = workspace.getInjectionDiv();
    const flyoutSvg = workspace.getFlyout()?.getWorkspace().getParentSvg();
    const describedBy = canvas.getAttribute('aria-describedby');
    const detach = keyboardPlugin.attach({
      workspace,
      store: useAppStore,
      core: () => null,
      selectBlock: () => undefined,
      activeModuleId: () => '',
    });
    expect(injection.querySelectorAll(`.${ANNOUNCER_CLASS}`)).toHaveLength(2);
    expect(canvas.getAttribute('aria-describedby')).not.toBe(describedBy);

    detach();
    expect(injection.querySelectorAll(`.${ANNOUNCER_CLASS}`)).toHaveLength(1);
    expect(canvas.getAttribute('aria-describedby')).toBe(describedBy);
    expect(canvas.getAttribute('aria-label')).toBe(CANVAS_LABEL);
    expect(flyoutSvg?.nextSibling).toBe(workspace.getParentSvg());
  });

  it('gives back Blockly’s own names and order when the last plugin detaches', () => {
    const { workspace, dispose } = open();
    editors = editors.filter((editor) => editor.workspace !== workspace);
    const canvas = workspace.getSvgGroup();
    const flyoutSvg = workspace.getFlyout()?.getWorkspace().getParentSvg();
    const blocklyLabel = 'Blockly Workspace';
    // The editor's dispose detaches the plugin first, then disposes the workspace: look in between.
    const remove = vi.spyOn(workspace, 'dispose').mockImplementation(() => {
      expect(canvas.getAttribute('aria-label')).toBe(blocklyLabel);
      expect(canvas.hasAttribute('aria-describedby')).toBe(false);
      expect(workspace.getParentSvg().nextSibling).not.toBe(null);
      expect(flyoutSvg?.previousSibling).not.toBe(null);
      expect(workspace.getInjectionDiv().querySelector(`.${ANNOUNCER_CLASS}`)).toBeNull();
      expect(registered()).toEqual([]);
    });
    dispose();
    expect(remove).toHaveBeenCalledTimes(1);
    remove.mockRestore();
    workspace.dispose();
  });

  it('turns on Blockly’s keyboard look when the canvas is reached with Tab', () => {
    const { workspace, keyboard } = open();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
    focusCanvas(workspace);
    expect(Blockly.keyboardNavigationController.getIsActive()).toBe(true);
    expect(keyboard.announcer.last).toBe('block “main”');
  });

  it('does not after a pointer press', () => {
    const { workspace } = open();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true }));
    document.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true }));
    focusCanvas(workspace);
    expect(Blockly.keyboardNavigationController.getIsActive()).toBe(false);
  });

  it('ends a move when another project opens', () => {
    const { block, keyboard } = open();
    Blockly.getFocusManager().focusNode(block('print'));
    press('m');
    expect(keyboard.mover.active).toBe(true);
    useAppStore
      .getState()
      .actions.setProject(projectFixture({ handle: 'ph_ffffffffffffffffffffffffffffffff' }));
    expect(keyboard.mover.active).toBe(false);
  });

  it('moves the focus off a connection before the workspace goes', () => {
    const { block, workspace, dispose } = open();
    const input = block('print').nextConnection;
    if (!(input instanceof Blockly.RenderedConnection)) {
      throw new Error('no next connection');
    }
    Blockly.getFocusManager().focusNode(input);
    editors = editors.filter((editor) => editor.workspace !== workspace);
    expect(() => {
      dispose();
    }).not.toThrow();
  });

  it('has no accessibility problems in the editor it attaches to', async () => {
    const { workspace } = open();
    await expectNoAxeViolations(workspace.getInjectionDiv());
  });
});

describe('the announcer', () => {
  it('announces plain text politely and repeats a repeated message', () => {
    const parent = document.createElement('div');
    document.body.append(parent);
    const announcer = new Announcer(parent, { canvas: 'keys', flyout: 'more keys' });
    const region = parent.querySelector('[role="status"]');
    expect(region?.getAttribute('aria-live')).toBe('polite');

    announcer.announce('<b>hello</b>');
    // Plain text: the markup is shown, never parsed.
    expect(region?.children).toHaveLength(0);
    expect(region?.textContent).toContain('<b>hello</b>');
    const first = region?.textContent;
    announcer.announce('<b>hello</b>');
    expect(region?.textContent).not.toBe(first);
    expect(announcer.last).toBe('<b>hello</b>');

    announcer.announce('x'.repeat(MAX_ANNOUNCEMENT_CHARS + 50));
    expect(announcer.last.length).toBe(MAX_ANNOUNCEMENT_CHARS);
    expect(announcer.last.endsWith('…')).toBe(true);

    announcer.announce('   ');
    expect(announcer.last).toBe('');
    announcer.dispose();
    expect(parent.children).toHaveLength(0);
    parent.remove();
  });
});

describe('the key map', () => {
  it('lists every key the plugin binds', () => {
    const keys = KEY_MAP.map((row) => row.keys).join(' ');
    for (const key of ['↓', '↑', '→', '←', 'Enter', 'Space', 'M', 'T', 'Escape', 'Delete']) {
      expect(keys).toContain(key);
    }
    for (const scope of ['canvas', 'move', 'toolbox', 'flyout']) {
      expect(KEY_MAP.some((row) => row.scope === scope)).toBe(true);
    }
  });
});

describe('reduced motion', () => {
  function media(matches: boolean) {
    return vi.fn((query: string) => (query === REDUCED_MOTION_QUERY ? { matches } : null));
  }

  it('reads the system setting', () => {
    expect(prefersReducedMotion(media(true))).toBe(true);
    expect(prefersReducedMotion(media(false))).toBe(false);
    expect(prefersReducedMotion(null)).toBe(false);
    expect(
      prefersReducedMotion(() => {
        throw new Error('no media queries');
      }),
    ).toBe(false);
  });

  it('stops the wiggle of a dragged block when the system asks for it', () => {
    const { workspace, block } = open();
    const stop = vi.spyOn(Blockly.blockAnimations, 'disconnectUiStop');
    const reduced = media(true);
    const detach = attachReducedMotion(workspace, reduced);
    const drag = new Blockly.Events.BlockDrag(block('print'), true, []);
    workspace.fireChangeListener(drag);
    expect(stop).toHaveBeenCalledTimes(1);

    const end = new Blockly.Events.BlockDrag(block('print'), false, []);
    workspace.fireChangeListener(end);
    expect(stop).toHaveBeenCalledTimes(1);
    detach();
    workspace.fireChangeListener(drag);
    expect(stop).toHaveBeenCalledTimes(1);
  });

  it('leaves the wiggle alone otherwise', () => {
    const { workspace, block } = open();
    const stop = vi.spyOn(Blockly.blockAnimations, 'disconnectUiStop');
    const detach = attachReducedMotion(workspace, media(false));
    workspace.fireChangeListener(new Blockly.Events.BlockDrag(block('print'), true, []));
    expect(stop).not.toHaveBeenCalled();
    detach();
  });
});
