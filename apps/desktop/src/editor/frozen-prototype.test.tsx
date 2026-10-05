/**
 * The shipped app runs with Tauri's `freezePrototype: true` (docs/spec/08-security.md §8.8). This
 * file freezes `Object.prototype` and then loads and runs the block editor: blockly-ext's blocks,
 * fields and mutators, the sync and the live preview with the real compiler core (when it is
 * built). A library that assigns to an inherited property breaks here.
 *
 * Blockly itself is imported before the freeze: under Node its package entry sets up jsdom, whose
 * dependencies assign to inherited properties at load time; the webviews load Blockly's browser
 * build instead, which the end-to-end tests run frozen.
 */
import { act, render } from '@testing-library/react';
import { beforeAll, describe, expect, it, vi } from 'vitest';

beforeAll(async () => {
  await import('blockly/core');
  await import('blockly/msg/en');
  Object.freeze(Object.prototype);
  window.matchMedia = () =>
    ({
      matches: false,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
    }) as unknown as MediaQueryList;
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe(): void {
        // happy-dom has no layout to observe.
      }
      unobserve(): void {
        // Nothing observed.
      }
      disconnect(): void {
        // Nothing observed.
      }
    },
  );
});

describe('with a frozen Object.prototype', () => {
  it('the editor loads, edits and reads back a project', async () => {
    expect(Object.isFrozen(Object.prototype)).toBe(true);
    const { EditorWorkspace } = await import('./EditorWorkspace');
    const { getEditorHandle } = await import('../app/editor-types');
    const { useAppStore } = await import('../app/store');
    const { documentFixture, projectFixture } = await import('../app/testing/fixtures');

    const doc = documentFixture();
    doc.modules = [
      {
        id: 'mod_main',
        name: 'main',
        workspace: {
          blocks: [
            {
              id: 'main',
              type: 'program.main',
              v: 1,
              x: 0,
              y: 0,
              statements: {
                BODY: [
                  {
                    id: 'if',
                    type: 'control.if',
                    v: 1,
                    extra: { elseIfCount: 1, hasElse: true },
                    inputs: { COND0: { expr: [{ ref: 's_x' }, { op: '<' }, { num: '3' }] } },
                  },
                  {
                    id: 'decl',
                    type: 'var.declare',
                    v: 1,
                    fields: { CONST: false, NAME: { sym: 's_x', name: 'x' }, TYPE: 'int' },
                  },
                ],
              },
            },
            { id: 'pk', type: 'pack.unknown', v: 1, x: 300, y: 0 },
          ],
        },
      },
    ];
    act(() => {
      useAppStore.getState().actions.setProject(projectFixture({ document: doc }));
    });
    const { unmount } = render(<EditorWorkspace />);
    const handle = getEditorHandle();
    expect(handle).not.toBeNull();
    act(() => {
      handle?.loadDocument(doc, { clearUndo: true });
    });
    const block = handle?.workspace.getBlockById('if');
    expect(block?.getInput('ELSE')).not.toBeNull();
    block?.setCommentText('frozen');
    // The compiler core starts and previews under the frozen prototype too (when it is built).
    const { testCore } = await import('./sync/testing');
    if ((await testCore()) !== null) {
      await vi.waitFor(
        () => {
          expect(useAppStore.getState().analysis.preview?.files.length).toBeGreaterThan(0);
        },
        { timeout: 20_000 },
      );
    }
    const read = handle?.currentDocument();
    expect(read?.modules[0]?.workspace.blocks.find((node) => node.id === 'main')).toMatchObject({
      statements: { BODY: [{ id: 'if', comment: { text: 'frozen' } }, { id: 'decl' }] },
    });
    unmount();
    expect(getEditorHandle()).toBeNull();
  });
});
