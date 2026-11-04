/**
 * The module strip (docs/plans/m3-contracts/w3-modules.md §1.2, §2.11, §5.2 E): shown from one
 * module on; the roving tab index; each module's actions and *Module actions*; ⊕ and its limit;
 * *All modules…*; the add, rename and delete dialogs with the real compiler core's name checks;
 * *Undo delete*; the status messages; the refusal while the canvas holds an unchecked change; axe on
 * the strip and the dialogs. The editor's part of each edit is a stand-in that applies the document
 * operations (the session's own are tested in src/editor/sync/modules.test.ts).
 */
import type { BdmBlock, BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createCommandRegistry, type CommandRegistry } from '../../app/commands';
import { createDialogQueue, DialogHost, type DialogQueue } from '../../app/dialogs';
import type { EditorHandle, ModuleEditOutcome } from '../../app/editor-types';
import { createAppEventBus } from '../../app/events';
import type { FeatureContext } from '../../app/features';
import { createScreenRegistry } from '../../app/screens';
import { resetAppStore, useAppStore } from '../../app/store';
import { createFakeIpc, documentFixture, projectFixture } from '../../app/testing/fixtures';
import {
  addModule,
  deleteModule,
  getModuleEdits,
  installModuleEdits,
  type KeptModule,
  MAX_MODULES,
  ModuleEditError,
  moveModule,
  REFUSED_TITLE,
  renameModule,
  restoreModule,
} from '../../features/project/modules';
import { expectNoAxeViolations } from '../../test/axe';
import { present, testCore } from '../sync/testing';
import { openDocumentInEditor } from '../load';
import { ModuleSwitcher } from './ModuleSwitcher';

const core: CoreWasm | null = await testCore();

/** `when program starts` and a define, as top-level blocks. */
const MAIN_BLOCKS: BdmBlock[] = [
  { id: 'blk_main', type: 'program.main', v: 1, x: 0, y: 0 },
  {
    id: 'blk_area',
    type: 'func.define',
    v: 1,
    x: 0,
    y: 200,
    fields: { NAME: { sym: 's_area', name: 'area' }, RETURNS: 'void' },
  },
];

/** Opens a project whose modules have `names` (`main` holding {@link MAIN_BLOCKS}). */
function openWith(names: readonly string[], active = names[0] ?? ''): BdmDocument {
  const doc = documentFixture();
  doc.modules = names.map((name) => ({
    id: `mod_${name}`,
    name,
    workspace: { blocks: name === 'main' ? MAIN_BLOCKS : [] },
  }));
  useAppStore
    .getState()
    .actions.setProject(projectFixture({ document: doc, activeModuleId: `mod_${active}` }));
  return doc;
}

/**
 * An editor that makes module edits as the session does, on the store's document (without a
 * canvas), and counts them.
 */
function fakeEditor(): EditorHandle & { readonly edits: string[] } {
  const edits: string[] = [];
  const apply = (
    what: string,
    change: (doc: BdmDocument) => { doc: BdmDocument; show: string | null },
  ): ModuleEditOutcome => {
    const project = useAppStore.getState().project;
    if (project === null) {
      return 'busy';
    }
    try {
      const next = change(project.document);
      edits.push(what);
      useAppStore.getState().actions.updateProject({
        document: next.doc,
        dirty: true,
        ...(next.show === null ? {} : { activeModuleId: next.show }),
      });
      return 'done';
    } catch (error: unknown) {
      if (error instanceof ModuleEditError) {
        return 'refused';
      }
      throw error;
    }
  };
  let kept: KeptModule | null = null;
  return {
    edits,
    loadDocument: () => undefined,
    currentDocument: () => {
      const project = useAppStore.getState().project;
      if (project === null) {
        throw new Error('no project');
      }
      return project.document;
    },
    selectBlock: () => undefined,
    updateProjectInfo: () => undefined,
    undo: () => undefined,
    redo: () => undefined,
    workspace: undefined as never,
    addModule: (id, name) => apply('add', (doc) => ({ doc: addModule(doc, id, name), show: id })),
    renameModule: (id, name) =>
      apply('rename', (doc) => ({ doc: renameModule(doc, id, name), show: null })),
    deleteModule: (id) => {
      const outcome = apply('delete', (doc) => {
        const deleted = deleteModule(doc, id);
        kept = deleted.kept;
        const shown = useAppStore.getState().project?.activeModuleId;
        return {
          doc: deleted.doc,
          show: shown === id ? (deleted.doc.modules[0]?.id ?? null) : null,
        };
      });
      return { outcome, kept: outcome === 'done' ? kept : null };
    },
    restoreModule: (keptModule) =>
      apply('restore', (doc) => ({
        doc: restoreModule(doc, keptModule),
        show: keptModule.module.id,
      })),
    moveModule: (id, delta) =>
      apply('move', (doc) => ({ doc: moveModule(doc, id, delta), show: null })),
  };
}

interface Harness {
  readonly dialogs: DialogQueue;
  readonly commands: CommandRegistry;
  readonly editor: ReturnType<typeof fakeEditor>;
  readonly ctx: FeatureContext;
  readonly uninstall: () => void;
}

let harness: Harness | null = null;

/** Installs the module edits over a fake editor and renders the strip with the dialog host. */
function renderStrip(): Harness {
  const dialogs = createDialogQueue();
  const commands = createCommandRegistry();
  const editor = fakeEditor();
  let next = 0;
  const ctx: FeatureContext = {
    ipc: createFakeIpc(),
    store: useAppStore,
    commands,
    screens: createScreenRegistry(),
    dialogs,
    events: createAppEventBus(),
    core: () => core,
    editor: () => editor,
  };
  const { uninstall } = installModuleEdits(ctx, {
    newModuleId: () => `mod_new${String((next += 1))}`,
  });
  render(
    <>
      <div className="workspace">
        <ModuleSwitcher />
      </div>
      <DialogHost queue={dialogs} />
    </>,
  );
  harness = { dialogs, commands, editor, ctx, uninstall };
  return harness;
}

/** The module buttons' texts. */
function moduleButtons(): HTMLElement[] {
  return screen.getAllByTestId('module-switch');
}

/** The open dialog. */
function dialog(): HTMLElement {
  return screen.getByRole('dialog');
}

/** Types `text` into the open prompt and gives its problem, or `null` without one. */
function typeName(text: string): string | null {
  const field = screen.getByRole('textbox');
  fireEvent.change(field, { target: { value: text } });
  const alert = screen.queryByRole('alert');
  expect(field.getAttribute('aria-invalid')).toBe(String(alert !== null));
  return alert === null ? null : alert.textContent.replace(/^✖ /u, '');
}

/** The text of the strip's status region. */
function statusText(): string {
  return screen.getByRole('status').textContent;
}

beforeEach(() => {
  resetAppStore();
});

afterEach(() => {
  vi.restoreAllMocks();
  harness?.dialogs.cancelAll();
  harness?.uninstall();
  harness = null;
});

describe('the module strip', () => {
  it('is not shown without a project, and is from one module on, with ⊕ and Module actions', () => {
    const { container, rerender } = render(<ModuleSwitcher />);
    expect(container.childElementCount).toBe(0);
    openWith(['main']);
    rerender(<ModuleSwitcher />);
    expect(screen.getByRole('toolbar', { name: 'Modules' })).not.toBeNull();
    expect(moduleButtons().map((button) => button.textContent)).toEqual(['main.cpp']);
    // Without the module edits feature the strip only switches.
    expect(screen.queryByRole('button', { name: 'Add module' })).toBeNull();
  });

  it('lists the modules as files and switches the active one', async () => {
    openWith(['main', 'shapes', 'io']);
    renderStrip();
    const toolbar = screen.getByRole('toolbar', { name: 'Modules' });
    expect(moduleButtons().map((button) => button.textContent)).toEqual([
      'main.cpp',
      'shapes.cpp',
      'io.cpp',
    ]);
    expect(moduleButtons().map((button) => button.getAttribute('aria-pressed'))).toEqual([
      'true',
      'false',
      'false',
    ]);
    await expectNoAxeViolations(screen.getByTestId('module-strip'));
    expect(toolbar.contains(screen.getByRole('button', { name: 'Add module' }))).toBe(false);

    fireEvent.click(screen.getByRole('button', { name: 'shapes.cpp' }));
    expect(useAppStore.getState().project?.activeModuleId).toBe('mod_shapes');
    expect(screen.getByRole('button', { name: 'shapes.cpp' }).getAttribute('aria-pressed')).toBe(
      'true',
    );
    // Pressing the shown module changes nothing.
    const before = useAppStore.getState().project;
    fireEvent.click(screen.getByRole('button', { name: 'shapes.cpp' }));
    expect(useAppStore.getState().project).toBe(before);
  });

  it('has one Tab stop for the modules, moved by the arrow keys, Home and End', () => {
    openWith(['main', 'shapes', 'io'], 'shapes');
    renderStrip();
    const stops = () => moduleButtons().map((button) => button.tabIndex);
    expect(stops()).toEqual([-1, 0, -1]);
    const buttons = moduleButtons();
    const main = present(buttons[0], 'main');
    const shapes = present(buttons[1], 'shapes');
    const io = present(buttons[2], 'io');
    shapes.focus();
    fireEvent.keyDown(shapes, { key: 'ArrowRight' });
    expect(document.activeElement).toBe(io);
    expect(stops()).toEqual([-1, -1, 0]);
    fireEvent.keyDown(io, { key: 'ArrowRight' });
    expect(document.activeElement).toBe(main);
    fireEvent.keyDown(main, { key: 'ArrowLeft' });
    expect(document.activeElement).toBe(io);
    fireEvent.keyDown(io, { key: 'Home' });
    expect(document.activeElement).toBe(main);
    fireEvent.keyDown(main, { key: 'End' });
    expect(document.activeElement).toBe(io);
    // Moving the focus shows nothing; Enter (a click) does.
    expect(useAppStore.getState().project?.activeModuleId).toBe('mod_shapes');
    fireEvent.click(io);
    expect(useAppStore.getState().project?.activeModuleId).toBe('mod_io');
    expect(stops()).toEqual([-1, -1, 0]);
  });

  it('opens a module’s actions with Shift+F10 or the context-menu key, Escape giving the focus back', async () => {
    openWith(['main', 'shapes']);
    renderStrip();
    const shapes = screen.getByRole('button', { name: 'shapes.cpp' });
    shapes.focus();
    fireEvent.keyDown(shapes, { key: 'F10', shiftKey: true });
    const menu = screen.getByRole('menu', { name: 'Actions of module shapes' });
    const items = screen.getAllByRole('menuitem');
    expect(items.map((item) => item.textContent)).toEqual([
      'Rename…',
      'Delete…',
      'Move left',
      'Move right',
    ]);
    expect(document.activeElement).toBe(items[0]);
    // The last module cannot move right: its reason is its description and tooltip.
    const right = present(items[3], 'Move right');
    expect(right.getAttribute('aria-disabled')).toBe('true');
    expect(right.title).toBe('This module is already the last.');
    expect(right.getAttribute('aria-describedby')).not.toBeNull();
    fireEvent.keyDown(menu, { key: 'End' });
    expect(document.activeElement).toBe(right);
    fireEvent.click(right);
    expect(screen.queryByRole('menu')).not.toBeNull();
    await expectNoAxeViolations(screen.getByTestId('module-strip'));
    fireEvent.keyDown(menu, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(shapes);

    fireEvent.keyDown(shapes, { key: 'ContextMenu' });
    fireEvent.click(screen.getByRole('menuitem', { name: 'Move left' }));
    expect(useAppStore.getState().project?.document.modules.map((m) => m.name)).toEqual([
      'shapes',
      'main',
    ]);
    expect(statusText()).toBe(
      'Module “shapes” moved left. It is now first, so the program is named after it.',
    );
  });

  it('gives the focus back to the strip after a menu item, its dialog or a deletion', async () => {
    openWith(['main', 'utils', 'extra']);
    renderStrip();
    const button = (name: string): HTMLElement => screen.getByRole('button', { name });
    const choose = async (name: string, item: string): Promise<void> => {
      const target = button(name);
      target.focus();
      fireEvent.keyDown(target, { key: 'F10', shiftKey: true });
      await act(async () => {
        fireEvent.click(screen.getByRole('menuitem', { name: item }));
        await Promise.resolve();
      });
    };
    // Rename… and its dialog, cancelled: back on the module's button.
    await choose('utils.cpp', 'Rename…');
    expect(dialog().textContent).toContain('Rename module “utils”');
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
      await Promise.resolve();
    });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(document.activeElement).toBe(button('utils.cpp'));
    // Move left: the module's button keeps it.
    await choose('utils.cpp', 'Move left');
    expect(useAppStore.getState().project?.document.modules.map((m) => m.name)).toEqual([
      'utils',
      'main',
      'extra',
    ]);
    expect(document.activeElement).toBe(button('utils.cpp'));
    // An empty module, deleted at once: its button is gone, so the shown module's gets it.
    await choose('extra.cpp', 'Delete…');
    await waitFor(() => {
      expect(statusText()).toBe('Module “extra” deleted.');
    });
    expect(document.activeElement).toBe(button('main.cpp'));
    // The shown module, with blocks, after its dialog: the module shown now.
    await choose('main.cpp', 'Delete…');
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Delete module' }));
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(statusText()).toBe('Module “main” deleted.');
    });
    expect(useAppStore.getState().project?.activeModuleId).toBe('mod_utils');
    expect(document.activeElement).toBe(button('utils.cpp'));
  });

  it('acts on the shown module from Module actions; Delete… is disabled on the only module', async () => {
    openWith(['main']);
    renderStrip();
    fireEvent.click(screen.getByRole('button', { name: 'Module actions' }));
    const remove = screen.getByRole('menuitem', { name: 'Delete…' });
    expect(remove.getAttribute('aria-disabled')).toBe('true');
    expect(remove.title).toBe('A project needs at least one module.');
    expect(
      document.getElementById(remove.getAttribute('aria-describedby') ?? '')?.textContent,
    ).toBe('A project needs at least one module.');
    fireEvent.click(remove);
    expect(harness?.editor.edits).toEqual([]);
    await expectNoAxeViolations(screen.getByTestId('module-strip'));
  });

  it('disables ⊕ at 256 modules, with its reason, and still passes axe', async () => {
    const names = ['main', ...Array.from({ length: MAX_MODULES - 1 }, (_, i) => `m${String(i)}`)];
    openWith(names);
    renderStrip();
    expect(moduleButtons()).toHaveLength(MAX_MODULES);
    const add = screen.getByRole('button', { name: 'Add module' });
    expect(add.getAttribute('aria-disabled')).toBe('true');
    expect(add.title).toBe('A project can have at most 256 modules.');
    const description = document.getElementById(add.getAttribute('aria-describedby') ?? '');
    expect(description?.textContent).toBe('A project can have at most 256 modules.');
    fireEvent.click(add);
    expect(screen.queryByRole('dialog')).toBeNull();
    await expectNoAxeViolations(screen.getByTestId('module-strip'));
  });

  it('offers All modules… with type-ahead when the buttons do not fit', () => {
    const scroll = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'scrollWidth');
    const client = Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'clientWidth');
    Object.defineProperty(HTMLElement.prototype, 'scrollWidth', {
      configurable: true,
      get(this: HTMLElement) {
        return this.getAttribute('role') === 'toolbar' ? 900 : 0;
      },
    });
    Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
      configurable: true,
      get: () => 300,
    });
    try {
      openWith(['main', 'geometry', 'graphics', 'report']);
      renderStrip();
      fireEvent.click(screen.getByRole('button', { name: 'All modules…' }));
      const menu = screen.getByRole('menu', { name: 'All modules' });
      const items = screen.getAllByRole('menuitemradio');
      expect(items.map((item) => item.getAttribute('aria-checked'))).toEqual([
        'true',
        'false',
        'false',
        'false',
      ]);
      expect(document.activeElement).toBe(items[0]);
      let now = 1_000;
      vi.spyOn(Date, 'now').mockImplementation(() => now);
      fireEvent.keyDown(menu, { key: 'g' });
      expect(document.activeElement).toBe(items[1]);
      // The same letter again: the next module with it.
      fireEvent.keyDown(menu, { key: 'g' });
      expect(document.activeElement).toBe(items[2]);
      // Letters typed together: the module they start.
      now += 2_000;
      fireEvent.keyDown(menu, { key: 'g' });
      fireEvent.keyDown(menu, { key: 'e' });
      expect(document.activeElement).toBe(items[1]);
      now += 2_000;
      fireEvent.keyDown(menu, { key: 'r' });
      expect(document.activeElement).toBe(items[3]);
      fireEvent.click(present(items[3], 'report'));
      expect(useAppStore.getState().project?.activeModuleId).toBe('mod_report');
      expect(screen.queryByRole('menu')).toBeNull();
    } finally {
      if (scroll !== undefined) {
        Object.defineProperty(HTMLElement.prototype, 'scrollWidth', scroll);
      }
      if (client !== undefined) {
        Object.defineProperty(HTMLElement.prototype, 'clientWidth', client);
      }
    }
  });

  it('shows a module whose name cannot be a file name as module<N>.cpp with an error mark', async () => {
    openWith(['main', 'b2c_tools']);
    renderStrip();
    const button = present(moduleButtons()[1], 'the second module');
    expect(button.textContent).toBe('⚠ module2.cpp');
    expect(button.getAttribute('aria-label')).toBe(
      'module2.cpp: the module “b2c_tools”, whose name cannot be used for its file. Rename it.',
    );
    expect(button.title).toBe(button.getAttribute('aria-label'));
    // The preview's file names win.
    act(() => {
      useAppStore.getState().actions.setAnalysis({
        preview: {
          ...(useAppStore.getState().analysis.preview ?? {
            stage: 'generate',
            diagnostics: [],
            sourceMap: null,
            buildable: true,
            placeholders: 0,
            contentHash: 'a'.repeat(64),
            textHash: 'b'.repeat(64),
            blockTypes: {},
            slotTypes: {},
            symbols: [],
          }),
          files: [
            { path: 'main.cpp', kind: 'source', module: 'mod_main', contents: '' },
            { path: 'module2.cpp', kind: 'source', module: 'mod_b2c_tools', contents: '' },
          ],
        },
      });
    });
    expect(moduleButtons().map((b) => b.textContent)).toEqual(['main.cpp', '⚠ module2.cpp']);
    await expectNoAxeViolations(screen.getByTestId('module-strip'));
  });
});

describe.skipIf(core === null)('the module dialogs (w3-modules.md §2.10, §2.11)', () => {
  it('add a module with a checked name, and say the other module’s code changed', async () => {
    openWith(['start']);
    renderStrip();
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Add module' }));
      await Promise.resolve();
    });
    const field = screen.getByRole<HTMLInputElement>('textbox');
    expect(field.value).toBe('module2');
    expect(dialog().textContent).toContain('Add module');
    expect(typeName('Main')).toBe(
      'Module names use lower-case letters, because they become file names. Use “main” instead.',
    );
    expect(typeName('2d')).toMatch(/cannot be used as a file name.*\(B2C-E0117\)$/u);
    expect(typeName('con')).toMatch(/\(B2C-E0118\)$/u);
    expect(typeName('b2c_x')).toBe(
      'Module names that start with “b2c” are kept for files that Blocks2Cpp writes. Choose another name.',
    );
    expect(typeName('x'.repeat(65))).toMatch(/\(B2C-E01\d\d\)$/u);
    expect(typeName('START')).toMatch(/\(B2C-E01(17|19)\)$/u);
    expect(typeName('start')).toMatch(/\(B2C-E0119\)$/u);
    await expectNoAxeViolations(dialog());
    expect(typeName('utils')).toBeNull();
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Add' }));
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    expect(moduleButtons().map((button) => button.textContent)).toEqual(['start.cpp', 'utils.cpp']);
    expect(useAppStore.getState().project?.activeModuleId).toBe('mod_new1');
    expect(statusText()).toBe(
      'Module “utils” added; the C++ code of the other module changed too, as a project with several modules needs.',
    );
    // A third module is just added.
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Add module' }));
      await Promise.resolve();
    });
    expect(screen.getByRole<HTMLInputElement>('textbox').value).toBe('module2');
    await act(async () => {
      fireEvent.submit(screen.getByRole('textbox'));
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(statusText()).toBe('Module “module2” added.');
    });
  });

  it('rename a module, refusing a name equal ignoring case; Cancel changes nothing', async () => {
    openWith(['main', 'utils'], 'utils');
    const { editor } = renderStrip();
    await act(async () => {
      void harness?.commands.runCommand('module.rename');
      await Promise.resolve();
    });
    expect(dialog().textContent).toContain('Rename module “utils”');
    expect(typeName('Main')).toMatch(/\(B2C-E01(17|19)\)$/u);
    expect(typeName('main')).toMatch(/\(B2C-E0119\)$/u);
    expect(typeName('Tools')).toBe(
      'Module names use lower-case letters, because they become file names. Use “tools” instead.',
    );
    await expectNoAxeViolations(dialog());
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
      await Promise.resolve();
    });
    expect(editor.edits).toEqual([]);
    await act(async () => {
      void harness?.commands.runCommand('module.rename');
      await Promise.resolve();
    });
    typeName('tools');
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(statusText()).toBe('Module “utils” renamed to “tools”.');
    });
    expect(moduleButtons().map((button) => button.textContent)).toEqual(['main.cpp', 'tools.cpp']);
  });

  it('delete an empty module at once, and ask for one with blocks, naming when program starts', async () => {
    openWith(['main', 'utils'], 'utils');
    const { editor } = renderStrip();
    await act(async () => {
      void harness?.commands.runCommand('module.delete');
      await Promise.resolve();
    });
    expect(screen.queryByRole('dialog')).toBeNull();
    expect(editor.edits).toEqual(['delete']);
    expect(statusText()).toBe('Module “utils” deleted.');
    // Undo delete, in the status message and in ≡ (a command while something can be undone).
    expect(harness?.commands.hasCommand('module.undoDelete')).toBe(true);
    fireEvent.click(screen.getByRole('button', { name: 'Undo delete' }));
    expect(useAppStore.getState().project?.document.modules.map((m) => m.id)).toEqual([
      'mod_main',
      'mod_utils',
    ]);
    expect(statusText()).toBe('Module “utils” is back.');
    expect(screen.queryByRole('button', { name: 'Undo delete' })).toBeNull();
    expect(harness?.commands.hasCommand('module.undoDelete')).toBe(false);

    // main holds two blocks, when program starts among them.
    const main = present(moduleButtons()[0], 'main');
    main.focus();
    fireEvent.keyDown(main, { key: 'F10', shiftKey: true });
    await act(async () => {
      fireEvent.click(screen.getByRole('menuitem', { name: 'Delete…' }));
      await Promise.resolve();
    });
    const ask = dialog();
    expect(ask.textContent).toContain('Delete module “main”?');
    expect(ask.textContent).toContain('It holds 2 blocks.');
    expect(ask.textContent).toContain('It holds “when program starts”');
    // Cancel is the default.
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Cancel' }));
    await expectNoAxeViolations(ask);
    await act(async () => {
      fireEvent.click(screen.getByRole('button', { name: 'Delete module' }));
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(statusText()).toBe('Module “main” deleted.');
    });
    expect(useAppStore.getState().project?.document.modules.map((m) => m.id)).toEqual([
      'mod_utils',
    ]);
    await act(async () => {
      await harness?.commands.runCommand('module.undoDelete');
    });
    // Back at its place, with its blocks and IDs.
    expect(useAppStore.getState().project?.document.modules[0]?.workspace.blocks).toEqual(
      MAIN_BLOCKS,
    );
  });

  it('keep a deleted module only until the next module edit or another project', async () => {
    openWith(['main', 'utils', 'extra']);
    renderStrip();
    act(() => {
      useAppStore.getState().actions.updateProject({ activeModuleId: 'mod_extra' });
    });
    await act(async () => {
      await harness?.commands.runCommand('module.delete');
    });
    expect(screen.queryByRole('button', { name: 'Undo delete' })).not.toBeNull();
    act(() => {
      void harness?.commands.runCommand('module.moveRight');
    });
    expect(screen.queryByRole('button', { name: 'Undo delete' })).toBeNull();
    act(() => {
      useAppStore.getState().actions.updateProject({ activeModuleId: 'mod_utils' });
    });
    await act(async () => {
      await harness?.commands.runCommand('module.delete');
    });
    expect(screen.queryByRole('button', { name: 'Undo delete' })).not.toBeNull();
    act(() => {
      openWith(['main']);
      useAppStore.getState().actions.updateProject({ handle: 'ph_another' });
    });
    expect(screen.queryByRole('button', { name: 'Undo delete' })).toBeNull();
    expect(statusText()).toBe('');
  });

  it.skipIf(core === null)(
    'forget a deleted module when the project is loaded again, which may hold it again',
    async () => {
      const doc = openWith(['main', 'utils'], 'utils');
      const { editor, ctx } = renderStrip();
      const handle = present(useAppStore.getState().project, 'the project').handle;
      await act(async () => {
        await harness?.commands.runCommand('module.delete');
      });
      expect(screen.queryByRole('button', { name: 'Undo delete' })).not.toBeNull();
      // *Reload*: the file still holds utils, and the handle stays.
      const text = JSON.stringify(doc);
      let loaded = false;
      await act(async () => {
        loaded = (
          await openDocumentInEditor(ctx, {
            handle,
            documentText: text,
            trust: present(useAppStore.getState().project, 'the project').trust,
            fileName: 'modules.b2c',
            migratedFrom: null,
            savedText: text,
          })
        ).ok;
      });
      expect(loaded).toBe(true);
      expect(useAppStore.getState().project?.handle).toBe(handle);
      expect(screen.queryByRole('button', { name: 'Undo delete' })).toBeNull();
      expect(harness?.commands.hasCommand('module.undoDelete')).toBe(false);
      expect(statusText()).toBe('');
      act(() => {
        getModuleEdits()?.undoDelete();
      });
      expect(editor.edits).toEqual(['delete']);
      expect(useAppStore.getState().project?.document.modules.map((m) => m.id)).toEqual([
        'mod_main',
        'mod_utils',
      ]);
    },
  );

  it('refuse every module edit, with the banner’s text, while the canvas holds an unchecked change', async () => {
    openWith(['main', 'utils', 'extra'], 'extra');
    const { editor } = renderStrip();
    // A module deleted before, which Undo delete would put back.
    await act(async () => {
      await harness?.commands.runCommand('module.delete');
    });
    expect(harness?.commands.hasCommand('module.undoDelete')).toBe(true);
    act(() => {
      useAppStore
        .getState()
        .actions.setAnalysis({ notice: 'syncFailed', syncCodes: ['B2C-E0104'] });
    });
    for (const command of [
      'module.add',
      'module.rename',
      'module.delete',
      'module.moveRight',
      'module.undoDelete',
    ] as const) {
      await act(async () => {
        void harness?.commands.runCommand(command);
        await Promise.resolve();
      });
      const refusal = screen.getByRole('dialog', { name: REFUSED_TITLE });
      expect(refusal.textContent).toContain('Blocks are nested more deeply than a project allows');
      await act(async () => {
        fireEvent.click(screen.getByRole('button', { name: 'OK' }));
        await Promise.resolve();
      });
    }
    expect(editor.edits).toEqual(['delete']);
    expect(screen.queryByRole('button', { name: 'Undo delete' })).not.toBeNull();
  });
});
