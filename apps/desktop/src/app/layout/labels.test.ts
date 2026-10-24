/** The status bar's texts. */
import { describe, expect, it } from 'vitest';

import { documentFixture, projectFixture, toolchainFixture } from '../testing/fixtures';
import { localTime, saveStateLabel, standardLabel, toolchainLabel } from './StatusBar';

describe('status bar labels', () => {
  it('name a toolchain by version and flavour, when known', () => {
    expect(toolchainLabel(toolchainFixture())).toBe('g++ 15.2.0 (MSYS2 UCRT64)');
    expect(toolchainLabel(toolchainFixture({ flavor: null }))).toBe('g++ 15.2.0');
    expect(toolchainLabel(toolchainFixture({ version: null, flavor: null }))).toBe('g++');
  });

  it('name the standard, with GNU extensions when they are on', () => {
    expect(standardLabel(projectFixture())).toBe('C++20');
    const document = documentFixture();
    document.project.language = { standard: 'c++23', gnuExtensions: true };
    expect(standardLabel(projectFixture({ document }))).toBe('GNU++23');
  });

  it('show the save time as HH:MM in local time', () => {
    const savedAt = new Date(2026, 9, 5, 9, 7).toISOString();
    expect(localTime(savedAt)).toBe('09:07');
    expect(saveStateLabel({ dirty: false, fileName: 'game.b2c', savedAt })).toBe('Saved 09:07');
  });

  it('say Unsaved for unsaved changes or a project without a file', () => {
    const fileName = 'game.b2c';
    expect(saveStateLabel({ dirty: true, fileName, savedAt: '2026-10-05T10:42:00Z' })).toBe(
      'Unsaved',
    );
    expect(saveStateLabel({ dirty: true, fileName, savedAt: null })).toBe('Unsaved');
    // A new project, untouched or not, is not in a file yet.
    expect(saveStateLabel({ dirty: false, fileName: null, savedAt: null })).toBe('Unsaved');
  });

  it('say Saved for a file just opened, which has no unsaved changes, and for an odd timestamp', () => {
    expect(saveStateLabel({ dirty: false, fileName: 'game.b2c', savedAt: null })).toBe('Saved');
    expect(localTime('not a time')).toBeNull();
    expect(saveStateLabel({ dirty: false, fileName: 'game.b2c', savedAt: 'not a time' })).toBe(
      'Saved',
    );
  });
});
