/**
 * What the user is told when a clipboard action does nothing: the loader's problems with a
 * refused payload, the limits of the project with the blocks inserted, and an unavailable core;
 * and the dialog notifier, which shows one notice at a time.
 */
import { describe, expect, it, vi } from 'vitest';

import { diagnosticFixture } from '../../app/testing/fixtures';
import { describeNotice, dialogNotifier, MAX_LISTED_PROBLEMS, MAX_PROBLEM_CHARS } from './notices';

const DUPLICATE_KEY = diagnosticFixture({
  code: 'B2C-E0105',
  message: 'The key "catalog" appears twice in one object of the pasted data.',
  source: 'loader',
});

/** Waits until every pending promise callback has run. */
async function afterMicrotasks(): Promise<void> {
  await new Promise((resolve) => setTimeout(resolve, 0));
}

/** `count` loader problems with distinct codes. */
function problems(count: number) {
  return Array.from({ length: count }, (_, index) =>
    diagnosticFixture({
      code: `B2C-E01${String(10 + index)}`,
      message: `problem ${String(index)}`,
    }),
  );
}

describe('describeNotice', () => {
  it('lists the loader codes of a refused paste', () => {
    const { title, message } = describeNotice({
      kind: 'refused',
      action: 'paste',
      diagnostics: [DUPLICATE_KEY],
    });
    expect(title).toBe('The blocks could not be pasted');
    expect(message).toBe(
      'The clipboard does not hold blocks that Blocks2Cpp can use, so nothing was pasted.\n\n' +
        'B2C-E0105: The key "catalog" appears twice in one object of the pasted data.',
    );
  });

  it('names the action in the title', () => {
    const titles = (['copy', 'cut', 'paste', 'duplicate'] as const).map(
      (action) => describeNotice({ kind: 'refused', action, diagnostics: [] }).title,
    );
    expect(titles).toEqual([
      'The blocks could not be copied',
      'The blocks could not be cut',
      'The blocks could not be pasted',
      'The block could not be duplicated',
    ]);
  });

  it('says that the project would break its limits, with the problems', () => {
    const deep = diagnosticFixture({ code: 'B2C-E0104', message: 'The nesting is too deep.' });
    const paste = describeNotice({ kind: 'limits', action: 'paste', diagnostics: [deep] });
    expect(paste.message).toBe(
      'Pasting these blocks here would take the project past the limits of a project file, so nothing was pasted.\n\n' +
        'B2C-E0104: The nesting is too deep.',
    );
    const duplicate = describeNotice({ kind: 'limits', action: 'duplicate', diagnostics: [] });
    expect(duplicate.message).toBe(
      'Duplicating this block would take the project past the limits of a project file, so nothing was changed.',
    );
  });

  it('lists at most a few problems and counts the rest', () => {
    const many = describeNotice({
      kind: 'refused',
      action: 'paste',
      diagnostics: problems(MAX_LISTED_PROBLEMS + 3),
    });
    const lines = many.message.split('\n').slice(2);
    expect(lines).toHaveLength(MAX_LISTED_PROBLEMS + 1);
    expect(lines.at(-1)).toBe('and 3 more problems');
    const oneMore = describeNotice({
      kind: 'refused',
      action: 'paste',
      diagnostics: problems(MAX_LISTED_PROBLEMS + 1),
    });
    expect(oneMore.message.split('\n').at(-1)).toBe('and 1 more problem');
  });

  it('cuts long messages at whole characters', () => {
    const long = diagnosticFixture({
      code: 'B2C-E0110',
      message: '😀'.repeat(MAX_PROBLEM_CHARS * 2),
    });
    const { message } = describeNotice({ kind: 'refused', action: 'paste', diagnostics: [long] });
    const line = message.split('\n').at(-1) ?? '';
    const shown = Array.from(line.slice('B2C-E0110: '.length));
    expect(shown).toHaveLength(MAX_PROBLEM_CHARS);
    expect(shown.at(-1)).toBe('…');
    expect(shown.slice(0, -1).every((char) => char === '😀')).toBe(true);
  });

  it('explains why an action could not run', () => {
    expect(
      describeNotice({ kind: 'unavailable', action: 'copy', reason: 'coreNotStarted' }).message,
    ).toBe('The compiler core is still starting. Try again in a moment.');
    expect(
      describeNotice({ kind: 'unavailable', action: 'paste', reason: 'coreStopped' }).message,
    ).toContain('is being restarted');
    expect(describeNotice({ kind: 'unavailable', action: 'cut', reason: 'internal' }).message).toBe(
      'Something went wrong inside Blocks2Cpp. Nothing was changed.',
    );
  });
});

describe('dialogNotifier', () => {
  it('shows one notice at a time and logs the ones that come meanwhile', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    let close: () => void = () => undefined;
    const alert = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          close = resolve;
        }),
    );
    const notify = dialogNotifier({ alert });
    notify({ kind: 'refused', action: 'paste', diagnostics: [DUPLICATE_KEY] });
    notify({ kind: 'unavailable', action: 'paste', reason: 'internal' });
    expect(alert).toHaveBeenCalledOnce();
    expect(alert).toHaveBeenCalledWith({
      title: 'The blocks could not be pasted',
      message: expect.stringContaining('B2C-E0105') as unknown,
    });
    expect(warn).toHaveBeenCalledOnce();

    close();
    await afterMicrotasks();
    notify({ kind: 'unavailable', action: 'copy', reason: 'coreNotStarted' });
    expect(alert).toHaveBeenCalledTimes(2);
  });

  it('shows the next notice after one that could not be shown', async () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const alert = vi.fn(() => Promise.reject(new Error('no dialog host')));
    const notify = dialogNotifier({ alert });
    notify({ kind: 'unavailable', action: 'copy', reason: 'internal' });
    await afterMicrotasks();
    expect(error).toHaveBeenCalledOnce();
    notify({ kind: 'unavailable', action: 'copy', reason: 'internal' });
    expect(alert).toHaveBeenCalledTimes(2);
  });
});
