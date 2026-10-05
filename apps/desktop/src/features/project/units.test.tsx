/**
 * The project feature's small parts: display text, error messages, the saved document's layout,
 * the recent-list filter and the start page section registry.
 */
import type { BdmDocument } from '@blocks2cpp/b2c-core-wasm';
import { act, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { documentFixture } from '../../app/testing/fixtures';
import { withGenerator, withSavedLayout } from './document';
import {
  describeOpenError,
  describeQuitError,
  describeSaveError,
  errorCode,
  loadProblems,
  MAX_SHOWN_PROBLEMS,
  newerFormatMessage,
} from './errors';
import { MAX_RECENT_ENTRIES, shownRecentEntries } from './model';
import { createStartPageSections, MAX_START_PAGE_SECTIONS, useStartPageSections } from './sections';
import { ipcFailure, loaderDiagnostic, recentEntry } from './testing';
import { localDateTime, MAX_SHOWN_NAME_CHARS, shownName, shownText } from './text';

describe('display text', () => {
  it('shows hidden characters and cuts long text without splitting a surrogate pair', () => {
    expect(shownText('a‮b', 100)).toBe('a⟨U+202E⟩b');
    expect(shownText('abcdef', 4)).toBe('abc…');
    // The cut would fall between the halves of 😀: the whole pair goes.
    expect(shownText('ab😀cd', 4)).toBe('ab…');
    expect(shownName('   ')).toBe('Untitled project');
    expect(shownName('x'.repeat(500))).toHaveLength(MAX_SHOWN_NAME_CHARS);
  });

  it('formats timestamps, refusing ones that do not parse', () => {
    expect(localDateTime('2026-10-05T10:42:00Z')).toMatch(/2026/);
    expect(localDateTime('yesterday')).toBeNull();
  });
});

describe('error messages', () => {
  it('words E0108 with and without the version needed', () => {
    expect(newerFormatMessage('0.3.0')).toContain('(needs ≥ 0.3.0)');
    expect(newerFormatMessage(null)).not.toContain('needs');
    expect(newerFormatMessage('  ')).not.toContain('needs');
    expect(newerFormatMessage('1‮0')).toContain('1⟨U+202E⟩0');
  });

  it('lists at most 20 loader problems and never none', () => {
    const many = Array.from({ length: 30 }, () => loaderDiagnostic('B2C-E0110', 'x'));
    expect(loadProblems(many).problems).toHaveLength(MAX_SHOWN_PROBLEMS);
    expect(loadProblems(many).omitted).toBe(10);
    expect(loadProblems([]).problems).toHaveLength(1);
  });

  it('maps every open failure to something the user can act on', () => {
    expect(describeOpenError(ipcFailure({ code: 'notFound' }))).toEqual({ kind: 'notFound' });
    expect(describeOpenError(ipcFailure({ code: 'io', kind: 'notFound' }))).toEqual({
      kind: 'notFound',
    });
    expect(describeOpenError(ipcFailure({ code: 'unknownRecent' }))).toEqual({
      kind: 'unknownRecent',
    });
    expect(describeOpenError(ipcFailure({ code: 'payloadTooLarge', limit: 33554432 }))).toEqual({
      kind: 'load',
      problems: [
        {
          code: 'B2C-E0101',
          message:
            'The project file is larger than 32 MiB, far more than any real project, so it was not opened.',
        },
      ],
      omitted: 0,
    });
    const message = (error: unknown) => {
      const failure = describeOpenError(error);
      return failure.kind === 'message' ? failure.message : failure.kind;
    };
    expect(message(ipcFailure({ code: 'io', kind: 'permissionDenied' }))).toContain(
      'not allowed to read',
    );
    expect(message(ipcFailure({ code: 'io', kind: 'other' }))).toContain('could not be read');
    expect(message(ipcFailure({ code: 'internal' }))).toContain('(Error: internal)');
    expect(message(new Error('bug'))).toContain('(Error: bug)');
  });

  it('maps every save failure to a sentence', () => {
    expect(describeSaveError(ipcFailure({ code: 'io', kind: 'permissionDenied' }))).toContain(
      'not allowed to write',
    );
    expect(describeSaveError(ipcFailure({ code: 'io', kind: 'other' }))).toContain(
      'disk is not full',
    );
    expect(describeSaveError(ipcFailure({ code: 'invalidDocument', diagnostics: [] }))).toContain(
      'This is a bug',
    );
    expect(describeSaveError(ipcFailure({ code: 'busy' }))).toContain('Another dialog');
    expect(describeSaveError(ipcFailure({ code: 'unknownHandle' }))).toContain('no longer open');
    expect(describeSaveError(ipcFailure({ code: 'rateLimited' }))).toContain(
      '(Error: rateLimited)',
    );
    expect(describeSaveError('nonsense')).toContain('(Error: bug)');
    expect(describeQuitError(ipcFailure({ code: 'internal' }))).toContain('(Error: internal)');
    expect(errorCode(new Error('x'))).toBe('bug');
  });
});

describe('the saved document', () => {
  function withViewports(): BdmDocument {
    const doc = documentFixture();
    doc.modules = [
      {
        id: 'mod_main',
        name: 'main',
        workspace: { blocks: [], viewport: { x: 1, y: 2, scale: 1 } },
      },
      { id: 'mod_util', name: 'util', workspace: { blocks: [] } },
    ];
    return doc;
  }

  it('sets the generator without touching the original', () => {
    const doc = documentFixture();
    const saved = withGenerator(doc, { app: '9.9.9', catalog: '2.0.0' });
    expect(saved.generator).toEqual({ app: '9.9.9', catalog: '2.0.0' });
    expect(doc.generator.app).toBe('0.1.0');
  });

  it("takes over the saved layout but keeps the current document's content", () => {
    const current = withViewports();
    current.project.name = 'Newer';
    current.modules.push({ id: 'mod_new', name: 'extra', workspace: { blocks: [] } });
    const saved = withGenerator(withViewports(), { app: '9.9.9', catalog: '1.0.0' });
    const mainSaved = saved.modules[0];
    const utilSaved = saved.modules[1];
    if (mainSaved === undefined || utilSaved === undefined) {
      throw new Error('missing modules');
    }
    delete mainSaved.workspace.viewport;
    utilSaved.workspace.viewport = { x: 5, y: 6, scale: 2 };

    const merged = withSavedLayout(current, saved);
    expect(merged.project.name).toBe('Newer');
    expect(merged.generator.app).toBe('9.9.9');
    expect(merged.modules[0]?.workspace.viewport).toBeUndefined();
    expect(merged.modules[1]?.workspace.viewport).toEqual({ x: 5, y: 6, scale: 2 });
    expect(merged.modules[2]).toBe(current.modules[2]);
    expect(current.modules[0]?.workspace.viewport).toEqual({ x: 1, y: 2, scale: 1 });
  });
});

describe('the recent list', () => {
  it('keeps well-formed entries, once each, at most ten', () => {
    const entries: unknown[] = [
      recentEntry(1),
      recentEntry(1),
      null,
      { ...recentEntry(2), projectName: 7 },
      { ...recentEntry(3), recentId: 'rc_NOT-HEX' },
      ...Array.from({ length: 20 }, (_, index) => recentEntry(index + 10)),
    ];
    const shown = shownRecentEntries(entries);
    expect(shown).toHaveLength(MAX_RECENT_ENTRIES);
    expect(shown[0]).toEqual(recentEntry(1));
    expect(new Set(shown.map((entry) => entry.recentId)).size).toBe(MAX_RECENT_ENTRIES);
  });
});

describe('start page sections', () => {
  it('orders sections, lets the newest registration of an ID win, and removes them', () => {
    const registry = createStartPageSections();
    const A = () => null;
    const B = () => null;
    const C = () => null;
    const removeB = registry.register('b', B, { order: 1 });
    registry.register('a', A, { order: 1 });
    const removeC = registry.register('c', C, { order: 0 });
    expect(registry.list().map((section) => section.id)).toEqual(['c', 'a', 'b']);

    const Newer = () => null;
    const removeNewer = registry.register('c', Newer);
    expect(registry.list()[0]?.component).toBe(Newer);
    removeNewer();
    removeNewer();
    expect(registry.list()[0]?.component).toBe(C);
    removeB();
    removeC();
    expect(registry.list().map((section) => section.id)).toEqual(['a']);
  });

  it('refuses bad IDs and orders, and more than the limit', () => {
    const registry = createStartPageSections();
    const component = () => null;
    expect(() => registry.register('', component)).toThrow();
    expect(() => registry.register('has space', component)).toThrow();
    expect(() => registry.register('ok', component, { order: Number.NaN })).toThrow();
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    for (let index = 0; index < MAX_START_PAGE_SECTIONS; index += 1) {
      registry.register(`s${String(index)}`, component);
    }
    const remove = registry.register('one-too-many', component);
    expect(registry.list()).toHaveLength(MAX_START_PAGE_SECTIONS);
    expect(error).toHaveBeenCalled();
    remove();
    // Another registration of a known ID is still allowed.
    registry.register('s0', component);
  });

  it('re-renders when sections change', () => {
    const registry = createStartPageSections();
    function Hello() {
      return <p>hello section</p>;
    }
    function Page() {
      const sections = useStartPageSections(registry);
      return (
        <div>
          {sections.map((section) => (
            <section.component key={section.id} />
          ))}
        </div>
      );
    }
    render(<Page />);
    expect(screen.queryByText('hello section')).toBeNull();
    let remove: () => void = () => undefined;
    act(() => {
      remove = registry.register('hello', Hello);
    });
    expect(screen.getByText('hello section')).toBeTruthy();
    act(() => {
      remove();
    });
    expect(screen.queryByText('hello section')).toBeNull();
  });
});
