/**
 * The status bar's toolchain item (docs/spec/04-user-interface.md §4.1, 08 §8.5): it names the
 * compiler a build will use, with the compiler's full path in its tooltip and accessible name, so
 * a planted g++ cannot pass unseen; hidden and reordering characters in the path are shown.
 */
import { act, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { expectNoAxeViolations } from '../../test/axe';
import { screens } from '../screens';
import { resetAppStore, useAppStore } from '../store';
import { toolchainFixture } from '../testing/fixtures';
import { MAX_TOOLCHAIN_PATH_CHARS, StatusBar, toolchainPathText } from './StatusBar';

const PATH = 'C:\\msys64\\ucrt64\\bin\\g++.exe';
const cleanups: (() => void)[] = [];

function showToolchain(displayPath = PATH): void {
  act(() => {
    useAppStore.getState().actions.setToolchains({
      list: [toolchainFixture({ displayPath })],
      discovering: false,
    });
  });
}

beforeEach(() => {
  resetAppStore();
});

afterEach(() => {
  for (const cleanup of cleanups.splice(0)) {
    cleanup();
  }
});

describe('the status bar’s toolchain', () => {
  it('has the full path in its tooltip and accessible name', async () => {
    const { container } = render(<StatusBar />);
    showToolchain();
    const item = screen.getByTestId('status-toolchain');
    expect(item.title).toBe(PATH);
    expect(item.textContent).toBe(`√g++ 15.2.0 (MSYS2 UCRT64), ${PATH}`);
    await expectNoAxeViolations(container);
  });

  it('names the path in the link to the toolchain page too', () => {
    cleanups.push(screens.registerScreen('toolchainSetup', () => null));
    render(<StatusBar />);
    showToolchain();
    // (The test DOM spaces the hidden part off as a block; a browser joins it to the label.)
    const link = screen.getByRole('button', {
      name: (name) =>
        name.replace(/\s+,/, ',') ===
        `g++ 15.2.0 (MSYS2 UCRT64), ${PATH} (open the toolchain page)`,
    });
    expect(link.title).toBe(PATH);
  });

  it('shows characters that hide or reorder text in the path', () => {
    render(<StatusBar />);
    showToolchain('/home/ada/bin/\u202Eexe.++g');
    const item = screen.getByTestId('status-toolchain');
    expect(item.title).toBe('/home/ada/bin/⟨U+202E⟩exe.++g');
    expect(item.textContent).toContain('/home/ada/bin/⟨U+202E⟩exe.++g');
  });

  it('cuts a very long path', () => {
    const long = `/${'a'.repeat(2 * MAX_TOOLCHAIN_PATH_CHARS)}/g++`;
    const shown = toolchainPathText(toolchainFixture({ displayPath: long }));
    expect(shown).toHaveLength(MAX_TOOLCHAIN_PATH_CHARS);
    expect(shown.endsWith('…')).toBe(true);
    expect(toolchainPathText(toolchainFixture({ displayPath: '/usr/bin/g++' }))).toBe(
      '/usr/bin/g++',
    );
  });

  it('has no path and no tooltip without a usable g++', () => {
    render(<StatusBar />);
    const item = screen.getByTestId('status-toolchain');
    expect(item.textContent).toBe('✖No g++ found');
    expect(item.hasAttribute('title')).toBe(false);
  });
});
