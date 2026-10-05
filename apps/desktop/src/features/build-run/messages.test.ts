/** The user-facing text of failed build and run commands, and where the build's document comes from. */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { IPC_ERROR_CODES, IpcCallError } from '@blocks2cpp/ipc-types';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import type { EditorHandle } from '../../app/editor-types';
import { resetAppStore, useAppStore } from '../../app/store';
import { documentFixture, projectFixture } from '../../app/testing/fixtures';
import { documentToBuild } from './document';
import { failureCode, failureMessage } from './messages';

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

  it('uses the store without the editor or the core', () => {
    useAppStore.getState().actions.setProject(projectFixture({ canonicalText: '{"x":1}' }));
    expect(documentToBuild({ store: useAppStore, editor: () => null, core: () => null })).toEqual({
      text: '{"x":1}',
      hash: 'a'.repeat(64),
    });
    expect(documentToBuild({ store: useAppStore, editor: () => editor, core: () => null })).toEqual(
      {
        text: '{"x":1}',
        hash: 'a'.repeat(64),
      },
    );
  });

  it('falls back to the store when reading the canvas throws, logging no project text', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    useAppStore.getState().actions.setProject(projectFixture({ canonicalText: '{"x":2}' }));
    const core = {
      canonical: () => {
        throw new TypeError('trap with "secret" text');
      },
    } as unknown as CoreWasm;
    expect(
      documentToBuild({ store: useAppStore, editor: () => editor, core: () => core })?.text,
    ).toBe('{"x":2}');
    expect(warn).toHaveBeenCalledWith(expect.any(String), { name: 'TypeError' });
  });
});
