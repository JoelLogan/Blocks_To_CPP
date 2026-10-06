/** The end-to-end test hook: what it reads, what it checks, and how it is installed. */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import type * as Blockly from 'blockly/core';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { BootPhase } from '../app/bootstrap';
import type { EditorHandle } from '../app/editor-types';
import { resetAppStore, useAppStore } from '../app/store';
import { projectFixture } from '../app/testing/fixtures';
import { ConsoleBridge } from '../features/build-run';
import type { TrustedTypesReport } from '../lib/trustedTypes';
import { createE2eHook, E2E_HOOK_NAME, type E2eHookDeps, installE2eHook } from './index';
import { ConsoleTranscript } from './transcript';

/** A canonical result the fake core gives. */
const CANONICAL = { ok: true, text: '{"canonical":true}\n', hash: 'b'.repeat(64), diagnostics: [] };

function fakeCore(result: unknown = CANONICAL): CoreWasm {
  return { canonical: vi.fn(() => result) } as unknown as CoreWasm;
}

/** A fake editor, and its `selectBlock` mock. */
function fakeEditor(): EditorHandle & { readonly selectBlockMock: ReturnType<typeof vi.fn> } {
  const selectBlockMock = vi.fn();
  return {
    loadDocument: vi.fn(),
    currentDocument: () => {
      const project = useAppStore.getState().project;
      if (project === null) {
        throw new Error('no project');
      }
      return project.document;
    },
    selectBlock: selectBlockMock,
    workspace: {} as Blockly.WorkspaceSvg,
    selectBlockMock,
  };
}

function deps(overrides: Partial<E2eHookDeps> = {}): E2eHookDeps {
  const report: TrustedTypesReport = { count: 2, directives: ['require-trusted-types-for'] };
  return {
    target: {},
    phase: (): BootPhase => ({ kind: 'ready' }),
    store: useAppStore,
    editor: () => null,
    core: () => null,
    console: new ConsoleBridge(),
    trustedTypes: () => report,
    elementAt: () => null,
    parseHtml: vi.fn(),
    ...overrides,
  };
}

beforeEach(() => {
  resetAppStore();
});

afterEach(() => {
  resetAppStore();
});

describe('createE2eHook', () => {
  it('reports readiness from the start-up phase', () => {
    let phase: BootPhase = { kind: 'starting' };
    const hook = createE2eHook(deps({ phase: () => phase }), new ConsoleTranscript());
    expect(hook.ready()).toBe(false);
    phase = { kind: 'ready' };
    expect(hook.ready()).toBe(true);
    expect(Object.isFrozen(hook)).toBe(true);
  });

  it('gives the canonical document of the canvas, or says why it cannot', () => {
    const editor = fakeEditor();
    const core = fakeCore();
    const hook = createE2eHook(
      deps({ editor: () => editor, core: () => core }),
      new ConsoleTranscript(),
    );
    expect(() => hook.document()).toThrow('No project is open');
    useAppStore.getState().actions.setProject(projectFixture());
    expect(hook.document()).toBe(CANONICAL.text);

    const refusing = fakeCore({ ok: false, diagnostics: [{ code: 'B2C-E0104' }] });
    const unreadable = createE2eHook(
      deps({ editor: () => editor, core: () => refusing }),
      new ConsoleTranscript(),
    );
    expect(() => unreadable.document()).toThrow('(B2C-E0104)');
  });

  it('reads the transcript, the Trusted Types counts and the C++ the code panel shows', () => {
    const transcript = new ConsoleTranscript();
    transcript.appendText('Correct!\r\n');
    const hook = createE2eHook(deps(), transcript);
    expect(hook.consoleText()).toBe('Correct!\n');
    const report = hook.trustedTypes();
    expect(report).toEqual({ count: 2, directives: ['require-trusted-types-for'] });

    expect(hook.code()).toBe('');
    useAppStore.getState().actions.setAnalysis({
      preview: {
        files: [
          { path: 'main.cpp', kind: 'source', contents: 'int main() {}\n' },
          { path: 'ide/b2c_ide_init.cpp', kind: 'source', contents: 'hidden\n' },
          { path: 'util.h', kind: 'header', contents: '#pragma once\n' },
        ],
      } as never,
    });
    expect(hook.code()).toBe('int main() {}\n\n#pragma once\n');
  });

  it('selects and centres a block, and needs the editor for the canvas helpers', () => {
    const editor = fakeEditor();
    const hook = createE2eHook(deps({ editor: () => editor }), new ConsoleTranscript());
    hook.selectBlock('b005');
    expect(editor.selectBlockMock).toHaveBeenCalledWith('b005', { center: true });
    expect(() => {
      hook.selectBlock('');
    }).toThrow(TypeError);

    const closed = createE2eHook(deps(), new ConsoleTranscript());
    expect(() => {
      closed.selectBlock('b005');
    }).toThrow('The editor is not open');
    expect(() => closed.blockElement('b005')).toThrow('The editor is not open');
    expect(() => {
      closed.insertBlocks('b011', 'BODY', []);
    }).toThrow('The editor is not open');
  });

  it('checks the arguments of the canvas helpers', () => {
    const editor = fakeEditor();
    const hook = createE2eHook(deps({ editor: () => editor }), new ConsoleTranscript());
    expect(() => hook.blockElement(7 as unknown as string)).toThrow(TypeError);
    expect(() => hook.fieldElement('b', 'x'.repeat(257))).toThrow(TypeError);
    expect(() => hook.connectionPoint('b', '')).toThrow(TypeError);
    expect(() => hook.grabPoint('')).toThrow(TypeError);
    expect(() => hook.flyoutBlockId('io.print', [] as unknown as Record<string, string>)).toThrow(
      'fields must be an object',
    );
    expect(() =>
      hook.flyoutBlockId('io.print', { MODE: 1 } as unknown as Record<string, string>),
    ).toThrow('string values');
  });

  it('probes the Trusted Types policy with a constant string', () => {
    const parseHtml = vi.fn();
    const hook = createE2eHook(deps({ parseHtml }), new ConsoleTranscript());
    hook.probeTrustedTypes();
    expect(parseHtml).toHaveBeenCalledWith('<p>Blocks2Cpp Trusted Types probe</p>');
    // The default parser accepts it without running anything.
    const withoutParser = { ...deps() };
    Reflect.deleteProperty(withoutParser, 'parseHtml');
    expect(() => {
      createE2eHook(withoutParser, new ConsoleTranscript()).probeTrustedTypes();
    }).not.toThrow();
  });
});

describe('installE2eHook', () => {
  it('installs a read-only hook with a console transcript, and removes both again', async () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const target: Record<string, unknown> = {};
    const bridge = new ConsoleBridge();
    const uninstall = installE2eHook(deps({ target, console: bridge }));

    const hook = target[E2E_HOOK_NAME] as { consoleText(): string };
    expect(hook).toBeDefined();
    expect(Object.keys(target)).toEqual([]);
    expect(() => {
      target[E2E_HOOK_NAME] = 'replaced';
    }).toThrow(TypeError);

    await bridge.write(new TextEncoder().encode('Too low!\r\n'));
    expect(hook.consoleText()).toBe('Too low!\n');

    uninstall();
    expect(E2E_HOOK_NAME in target).toBe(false);
    expect(Object.hasOwn(bridge, 'write')).toBe(false);
  });
});
