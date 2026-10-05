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
import { expectNoAxeViolations } from '../test/axe';
import {
  attachEditor,
  EditorWorkspace,
  editorInjectOptions,
  INITIAL_TOOLBOX,
} from './EditorWorkspace';
import { createCoreHost } from './preview/coreHost';
import { registerEditorBlocks } from './services';
import { present } from './sync/testing';

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
    expect(() => context?.core.version()).toThrow('has not started');
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

  it('has no module switcher for a single module', () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    useAppStore.getState().actions.setProject(projectFixture());
    const { unmount } = render(<EditorWorkspace />);
    expect(screen.queryByTestId('module-switch')).toBeNull();
    unmount();
  });
});

describe('the initial toolbox and the inject options', () => {
  it('lists every catalog category that has blocks, with its style', () => {
    const contents = (
      INITIAL_TOOLBOX as {
        contents: { name: string; categorystyle: string; contents: unknown[] }[];
      }
    ).contents;
    expect(contents.map((category) => category.name)).toContain('Program');
    for (const category of contents) {
      expect(category.categorystyle).toMatch(/^b2c_/);
      expect(category.contents.length).toBeGreaterThan(0);
    }
    const options = editorInjectOptions(Blockly.Themes.Zelos);
    expect(options.zoom).toMatchObject({ minScale: 0.1, maxScale: 4 });
    expect(options.plugins).toEqual({ connectionChecker: 'b2c_checker' });
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
