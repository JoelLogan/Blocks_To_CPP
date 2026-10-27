/** The user-facing text of failed build and run commands, and where the build's document comes from. */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { IPC_ERROR_CODES, IpcCallError } from '@blocks2cpp/ipc-types';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { EditorHandle } from '../../app/editor-types';
import { resetAppStore, useAppStore } from '../../app/store';
import { documentFixture, projectFixture } from '../../app/testing/fixtures';
import { documentToBuild } from './document';
import { failureCode, failureMessage, unreadableCanvasMessage } from './messages';

describe('failureCode', () => {
  it('is the IPC error code, or transport for anything else', () => {
    expect(failureCode(new IpcCallError('run_start', { code: 'staleBuild' }))).toBe('staleBuild');
    expect(failureCode(new IpcCallError('run_start', { code: 'transport', message: 'gone' }))).toBe(
      'transport',
    );
    expect(failureCode(new Error('boom'))).toBe('transport');
    expect(failureCode('boom')).toBe('transport');
  });
});

describe('failureMessage', () => {
  it('has a title and a message for every code, for both actions, and never echoes the code', () => {
    for (const action of ['build', 'run'] as const) {
      for (const code of [...IPC_ERROR_CODES, 'transport'] as const) {
        const { title, message } = failureMessage(code, action);
        expect(title.length).toBeGreaterThan(0);
        expect(message.length).toBeGreaterThan(0);
        expect(message).not.toContain(code);
      }
    }
  });

  it('explains the common cases in plain words', () => {
    expect(failureMessage('restricted', 'build')).toEqual({
      title: 'Restricted Mode',
      message: 'This project is in Restricted Mode. Trust it to build it.',
    });
    expect(failureMessage('restricted', 'run').message).toBe(
      'This project is in Restricted Mode. Trust it to run it.',
    );
    expect(failureMessage('io', 'build').message).toBe(
      'A file the build needs could not be read or written.',
    );
    expect(failureMessage('io', 'run').title).toBe('The program could not start');
    expect(failureMessage('staleBuild', 'run').message).toBe(
      'The program is out of date. Build the project, then try again.',
    );
    expect(failureMessage('internal', 'run').message).toBe(
      'Something went wrong. The details are in the log file.',
    );
    // A lost input file says what to do about the file, not to build again.
    const input = failureMessage('inputFileUnreadable', 'run').message;
    expect(input).toContain('input file can no longer be read');
    expect(input).toContain('Project settings');
    expect(input).not.toContain('Build');
  });
});

describe('documentToBuild', () => {
  beforeEach(() => {
    resetAppStore();
  });

  const editor = { currentDocument: () => documentFixture() } as unknown as EditorHandle;

  it('is null without a project', () => {
    expect(
      documentToBuild({ store: useAppStore, editor: () => editor, core: () => null }),
    ).toBeNull();
  });

  it('sends the store’s document as its JSON text when no core can write it', () => {
    useAppStore.getState().actions.setProject(projectFixture());
    const stored = {
      kind: 'document',
      document: { text: JSON.stringify(projectFixture().document), hash: 'a'.repeat(64) },
    };
    expect(documentToBuild({ store: useAppStore, editor: () => null, core: () => null })).toEqual(
      stored,
    );
    expect(documentToBuild({ store: useAppStore, editor: () => editor, core: () => null })).toEqual(
      stored,
    );
  });

  it('writes the store’s document as canonical text through the core without the editor', () => {
    const named = documentFixture('Stored');
    useAppStore.getState().actions.setProject(projectFixture({ document: named }));
    const canonical = vi.fn(() => ({
      ok: true,
      text: '{"canonical":true}',
      hash: 'c'.repeat(64),
      textHash: 'd'.repeat(64),
      diagnostics: [],
    }));
    const core = { canonical } as unknown as CoreWasm;
    expect(documentToBuild({ store: useAppStore, editor: () => null, core: () => core })).toEqual({
      kind: 'document',
      document: { text: '{"canonical":true}', hash: 'c'.repeat(64) },
    });
    expect(canonical).toHaveBeenCalledWith(JSON.stringify(named));
  });

  it('reads the canvas now, through the core', () => {
    useAppStore.getState().actions.setProject(projectFixture());
    const core = {
      canonical: () => ({ ok: true, text: '{"x":3}', hash: 'c'.repeat(64), document: {} }),
    } as unknown as CoreWasm;
    expect(documentToBuild({ store: useAppStore, editor: () => editor, core: () => core })).toEqual(
      {
        kind: 'document',
        document: { text: '{"x":3}', hash: 'c'.repeat(64) },
      },
    );
  });

  it('does not fall back to the stored document when the loader refuses the canvas', () => {
    vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    useAppStore.getState().actions.setProject(projectFixture());
    const diagnostics = [
      {
        code: 'B2C-E0104',
        severity: 'error' as const,
        message: 'Lists and objects in the project file are nested more than 128 levels deep.',
        primary: { part: { kind: 'whole' as const } },
        source: 'loader' as const,
      },
    ];
    const core = { canonical: () => ({ ok: false, diagnostics }) } as unknown as CoreWasm;
    expect(documentToBuild({ store: useAppStore, editor: () => editor, core: () => core })).toEqual(
      {
        kind: 'unreadable',
        diagnostics,
      },
    );
    const { title, message } = unreadableCanvasMessage(diagnostics);
    expect(title).toBe('The build could not start');
    expect(message).toContain(
      'B2C-E0104: Lists and objects in the project file are nested more than 128 levels deep.',
    );
    expect(message).toContain('nothing was built');
  });

  it('falls back to the store when reading the canvas throws, logging no project text', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    useAppStore.getState().actions.setProject(projectFixture());
    const core = {
      canonical: () => {
        throw new TypeError('trap with "secret" text');
      },
    } as unknown as CoreWasm;
    expect(documentToBuild({ store: useAppStore, editor: () => editor, core: () => core })).toEqual(
      {
        kind: 'document',
        document: { text: JSON.stringify(projectFixture().document), hash: 'a'.repeat(64) },
      },
    );
    expect(warn).toHaveBeenCalledWith(expect.any(String), { name: 'TypeError' });
    expect(JSON.stringify(warn.mock.calls)).not.toContain('secret');
  });
});
