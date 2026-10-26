/**
 * The block editor component against the real Blockly: injection options, theme, resizing, the
 * editor handle, the plugins, the module switcher and clean-up.
 */
import { act, fireEvent, render, screen } from '@testing-library/react';
import * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { getCore, setCore } from '../app/core';
import { type EditorContext, type EditorPlugin, getEditorHandle } from '../app/editor-types';
import { EDITOR_PLUGINS } from '../app/editorPlugins';
import { resetAppStore, useAppStore } from '../app/store';
import { documentFixture, projectFixture } from '../app/testing/fixtures';
import { pressTab } from '../app/layout/sequentialFocus';
import { expectNoAxeViolations } from '../test/axe';
import { attachEditor, EditorWorkspace, editorInjectOptions } from './EditorWorkspace';
import { createKeyboardPlugin } from './keyboard';
import { createCoreHost } from './preview/coreHost';
import { registerEditorBlocks } from './services';
import { B2cBlockDragger } from './sync/drag';
import { present, setViewSize } from './sync/testing';
import { CONTROLS_COMPONENT_ID } from './view/placement';
import { FLYOUT_TEST_TIMEOUT_MS } from './toolbox/testing';

// Each test here injects the editor with the continuous toolbox, whose flyout draws every catalog
// block (toolbox/testing.ts): the slowest test took 2.6 s under coverage with two workers here and
// 3.3 s in CI after W2's blocks. Package T's flyout reuse is to make this limit unneeded.
vi.setConfig({ testTimeout: FLYOUT_TEST_TIMEOUT_MS });

/** The colour-scheme media query the component reads. */
const scheme = vi.hoisted(() => {
  const listeners = new Set<() => void>();
  const query = {
    matches: false,
    addEventListener: (_type: string, listener: () => void) => listeners.add(listener),
    removeEventListener: (_type: string, listener: () => void) => listeners.delete(listener),
  };
  return {
    listeners,
    query,
    set(dark: boolean) {
      query.matches = dark;
      for (const listener of listeners) {
        listener();
      }
    },
  };
});

class FakeResizeObserver {
  static instances: FakeResizeObserver[] = [];
  readonly callback: () => void;
  disconnected = false;
  constructor(callback: () => void) {
    this.callback = callback;
    FakeResizeObserver.instances.push(this);
  }
  observe(): void {
    // The test calls the callback by hand.
  }
  unobserve(): void {
    // Not needed.
  }
  disconnect(): void {
    this.disconnected = true;
  }
}

/** The main workspaces Blockly holds (not flyouts). */
function mainWorkspaces(): Blockly.WorkspaceSvg[] {
  return Blockly.Workspace.getAll().filter(
    (workspace): workspace is Blockly.WorkspaceSvg =>
      workspace instanceof Blockly.WorkspaceSvg && !workspace.isFlyout && !workspace.isMutator,
  );
}

function twoModuleProject(): void {
  const doc = documentFixture();
  doc.modules = [
    {
      id: 'mod_main',
      name: 'main',
      workspace: { blocks: [{ id: 'a', type: 'control.forever', v: 1, x: 0, y: 0 }] },
    },
    {
      id: 'mod_util',
      name: 'util',
      workspace: { blocks: [{ id: 'b', type: 'control.forever', v: 1, x: 0, y: 0 }] },
    },
  ];
  useAppStore
    .getState()
    .actions.setProject(projectFixture({ document: doc, activeModuleId: 'mod_main' }));
}

beforeEach(() => {
  resetAppStore();
  scheme.set(false);
  window.matchMedia = () => scheme.query as unknown as MediaQueryList;
  FakeResizeObserver.instances = [];
  vi.stubGlobal('ResizeObserver', FakeResizeObserver);
  // A core that never starts: these tests are about the component, not the preview.
  setCore(null);
  // The app's own plugins are tested in their packages; these tests add their own.
  EDITOR_PLUGINS.length = 0;
});

afterEach(() => {
  EDITOR_PLUGINS.length = 0;
  setCore(null);
});

describe('the block editor', () => {
  it('injects Blockly with Zelos, the Blocks2Cpp theme and checker, bundled media, no sounds', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { container, unmount } = render(<EditorWorkspace />);
    const host = container.querySelector('.blockly-host');
    expect(host?.querySelector('.injectionDiv')).not.toBeNull();
    const [workspace] = mainWorkspaces();
    expect(workspace?.options.renderer).toBe('zelos');
    expect(workspace?.options.hasSounds).toBe(false);
    expect(workspace?.options.pathToMedia).toBe('/node_modules/blockly/media/');
    expect(workspace?.getTheme().name).toBe('b2c-light');
    expect(workspace?.connectionChecker.constructor.name).toBe('B2cConnectionChecker');
    // Blockly creates a dragger per drag from the options; ours reports drags to the session.
    expect(
      workspace &&
        Blockly.registry.getClassFromOptions(
          Blockly.registry.Type.BLOCK_DRAGGER,
          workspace.options,
        ),
    ).toBe(B2cBlockDragger);
    expect(workspace?.getToolbox()).not.toBeNull();
    unmount();
    expect(mainWorkspaces()).toHaveLength(0);
  });

  it('follows the system colour scheme', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    scheme.set(true);
    const { unmount } = render(<EditorWorkspace />);
    const [workspace] = mainWorkspaces();
    expect(workspace?.getTheme().name).toBe('b2c-dark');
    act(() => {
      scheme.set(false);
    });
    expect(workspace?.getTheme().name).toBe('b2c-light');
    expect(scheme.listeners.size).toBe(1);
    unmount();
    expect(scheme.listeners.size).toBe(0);
  });

  it('resizes the workspace with its panel and stops observing when it closes', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const { unmount } = render(<EditorWorkspace />);
    const [workspace] = mainWorkspaces();
    if (workspace === undefined) {
      throw new Error('no workspace');
    }
    // Blockly.svgResize ends by resizing the workspace.
    const resize = vi.spyOn(workspace, 'resize');
    const observer = FakeResizeObserver.instances[0];
    observer?.callback();
    expect(resize).toHaveBeenCalled();
    unmount();
    expect(observer?.disconnected).toBe(true);
  });

  it('publishes the editor handle and shows the open project', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    twoModuleProject();
    const { unmount } = render(<EditorWorkspace />);
    const handle = getEditorHandle();
    expect(handle?.workspace.getBlockById('a')).not.toBeNull();
    expect(handle?.currentDocument().modules).toHaveLength(2);
    unmount();
    expect(getEditorHandle()).toBeNull();
  });

  it('attaches the editor plugins and detaches them in reverse order', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const order: string[] = [];
    let context: EditorContext | null = null;
    const plugin = (name: string): EditorPlugin => ({
      name,
      attach: (ctx) => {
        context = ctx;
        order.push(`attach ${name}`);
        return () => order.push(`detach ${name}`);
      },
    });
    EDITOR_PLUGINS.push(
      plugin('toolbox'),
      {
        name: 'broken',
        attach: () => {
          throw new Error('broken plugin');
        },
      },
      plugin('diagnostics'),
    );
    twoModuleProject();
    const { unmount } = render(<EditorWorkspace />);
    expect(order).toEqual(['attach toolbox', 'attach diagnostics']);
    expect((context as EditorContext | null)?.activeModuleId()).toBe('mod_main');
    // The core has not started in this test, and the context reads it on every call.
    expect((context as EditorContext | null)?.core()).toBeNull();
    unmount();
    expect(order).toEqual([
      'attach toolbox',
      'attach diagnostics',
      'detach diagnostics',
      'detach toolbox',
    ]);
  });

  it('shows a module switcher for projects with several modules', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    twoModuleProject();
    const { unmount } = render(<EditorWorkspace />);
    const buttons = screen.getAllByTestId('module-switch');
    expect(buttons.map((button) => button.textContent)).toEqual(['main.cpp', 'util.cpp']);
    expect(buttons[0]?.getAttribute('aria-pressed')).toBe('true');
    // Blockly's own canvas is checked by the accessibility pass (milestone M2, wave 5).
    await expectNoAxeViolations(screen.getByRole('navigation', { name: 'Modules' }));

    act(() => {
      fireEvent.click(present(buttons[1], 'the second module button'));
    });
    expect(useAppStore.getState().project?.activeModuleId).toBe('mod_util');
    expect(getEditorHandle()?.workspace.getBlockById('b')).not.toBeNull();
    expect(getEditorHandle()?.workspace.getBlockById('a')).toBeNull();
    expect(screen.getAllByTestId('module-switch')[1]?.getAttribute('aria-pressed')).toBe('true');
    unmount();
  });

  it('puts the canvas controls over the canvas, next in the Tab order, and takes them away with it', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    useAppStore.getState().actions.setProject(projectFixture());
    // As in the app: the keyboard plugin puts the toolbox's blocks before the canvas.
    EDITOR_PLUGINS.push(createKeyboardPlugin({ matchMedia: null }));
    const { container, unmount } = render(<EditorWorkspace />);
    const [workspace] = mainWorkspaces();
    if (workspace === undefined) {
      throw new Error('no workspace');
    }
    // The window lays the editor out: the resize observer resizes the workspace.
    setViewSize(workspace, 1000, 700);
    act(() => {
      FakeResizeObserver.instances[0]?.callback();
    });

    // In the workspace's container, right after Blockly's injection `div`.
    const group = screen.getByRole('group', { name: 'Canvas zoom' });
    expect(group.parentElement).toBe(container.querySelector('.blockly-host'));
    expect(group.previousElementSibling).toBe(workspace.getInjectionDiv());
    expect(group.style.display).toBe('');
    // Blockly's own SVG zoom controls are gone.
    expect(container.querySelector('.blocklyZoom')).toBeNull();

    // Tab goes from the canvas to the controls.
    const canvas = workspace.getSvgGroup();
    if (!(canvas instanceof SVGElement)) {
      throw new Error('the canvas is not an SVG element');
    }
    canvas.focus();
    const reached: (string | null)[] = [];
    for (let step = 0; step < 4; step++) {
      act(() => {
        reached.push(pressTab()?.getAttribute('aria-label') ?? null);
      });
    }
    expect(reached).toEqual(['Zoom out', 'Zoom in', 'Reset zoom (now 90%)', 'Show all blocks']);

    unmount();
    expect(group.isConnected).toBe(false);
    expect(workspace.getComponentManager().getComponent(CONTROLS_COMPONENT_ID)).toBeUndefined();
  });

  it('has no module switcher for a single module', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    useAppStore.getState().actions.setProject(projectFixture());
    const { unmount } = render(<EditorWorkspace />);
    expect(screen.queryByTestId('module-switch')).toBeNull();
    unmount();
  });
});

describe('the initial toolbox and the inject options', () => {
  it('start with every catalog category and the continuous toolbox', () => {
    const options = editorInjectOptions(Blockly.Themes.Zelos);
    const contents = (options.toolbox as { contents: { name: string }[] }).contents;
    expect(contents.map((category) => category.name)).toContain('Program');
    // The wheel scrolls, Ctrl + wheel and a pinch zoom; the editor draws its own zoom controls.
    expect(options.zoom).toMatchObject({
      controls: false,
      wheel: true,
      pinch: true,
      minScale: 0.1,
      maxScale: 4,
    });
    expect(options.move).toMatchObject({ wheel: true });
    expect(options.plugins).toEqual({
      connectionChecker: 'b2c_checker',
      connectionPreviewer: 'b2c_previewer',
      blockDragger: B2cBlockDragger,
      toolbox: 'b2c_continuous_toolbox',
      flyoutsVerticalToolbox: 'b2c_continuous_flyout',
      metricsManager: 'b2c_continuous_metrics',
    });
  });

  it('make the trash can a place to drop blocks that keeps nothing', async () => {
    registerEditorBlocks();
    const host = document.createElement('div');
    document.body.append(host);
    const workspace = Blockly.inject(host, editorInjectOptions(Blockly.Themes.Zelos));
    const trashcan = present(workspace.trashcan, 'the trash can');
    // Still a delete area: a block dropped on it is deleted.
    expect(
      workspace
        .getComponentManager()
        .hasCapability(trashcan.id, Blockly.ComponentManager.Capability.DELETE_AREA),
    ).toBe(true);
    expect(host.querySelector('.blocklyTrashcanFlyout')).toBeNull();

    const block = workspace.newBlock('control.forever');
    block.initSvg();
    block.render();
    block.dispose(false);
    // Blockly delivers events after a frame; the trash can would store the block then.
    await new Promise((resolve) => requestAnimationFrame(resolve));
    await new Promise((resolve) => setTimeout(resolve, 0));
    // A click opens the stored blocks, 10 ms later.
    trashcan.click();
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(trashcan.contentsIsOpen()).toBe(false);

    workspace.dispose();
    host.remove();
  });

  it('attaches to an injected workspace and starts the core early', async () => {
    registerEditorBlocks();
    const host = document.createElement('div');
    document.body.append(host);
    const workspace = Blockly.inject(host, editorInjectOptions(Blockly.Themes.Zelos));
    const initCore = vi.fn(() => Promise.reject(new Error('not built')));
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const editor = attachEditor(workspace, {
      store: useAppStore,
      host: createCoreHost({ initCore, resetCore: vi.fn(), getCore, setCore }),
      dialogs: {} as never,
      plugins: [],
    });
    await Promise.resolve();
    expect(initCore).toHaveBeenCalled();
    await vi.waitFor(() => {
      expect(error).toHaveBeenCalledWith(
        'The compiler core could not be started',
        expect.any(Error),
      );
    });
    editor.detach();
    editor.detach();
    expect(getEditorHandle()).toBeNull();
    workspace.dispose();
    host.remove();
  });
});
