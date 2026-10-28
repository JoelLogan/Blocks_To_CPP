import type { Theme } from '@blocks2cpp/ipc-types';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { resetAppStore, useAppStore } from '../store';
import { settingsFixture } from '../testing/fixtures';
import {
  applyTheme,
  DARK_SCHEME_QUERY,
  documentTheme,
  onThemeChange,
  resolveTheme,
  type SchemeQuery,
  startAppearance,
  systemSchemeQuery,
  THEME_ATTRIBUTE,
} from './appearance';

/** A system scheme the test changes. */
function fakeScheme(dark: boolean): SchemeQuery & { set(dark: boolean): void; listeners: number } {
  const listeners = new Set<() => void>();
  let matches = dark;
  return {
    get matches() {
      return matches;
    },
    get listeners() {
      return listeners.size;
    },
    addEventListener: (_type, listener) => listeners.add(listener),
    removeEventListener: (_type, listener) => listeners.delete(listener),
    set(next: boolean) {
      matches = next;
      for (const listener of [...listeners]) {
        listener();
      }
    },
  };
}

/** Waits for the attribute's mutation records (delivered in a microtask). */
async function mutations(): Promise<void> {
  await Promise.resolve();
}

afterEach(() => {
  document.documentElement.removeAttribute(THEME_ATTRIBUTE);
  resetAppStore();
});

describe('resolveTheme', () => {
  it.each([
    ['light', false, 'light'],
    ['light', true, 'light'],
    ['dark', false, 'dark'],
    ['dark', true, 'dark'],
    ['system', false, 'light'],
    ['system', true, 'dark'],
    [null, false, 'light'],
    [null, true, 'dark'],
  ] as const)('%s with the system dark: %s gives %s', (choice, systemDark, expected) => {
    expect(resolveTheme(choice, systemDark)).toBe(expected);
  });
});

describe('the attribute', () => {
  it('is the theme in use: dark only when it says dark', () => {
    const root = document.createElement('div');
    expect(documentTheme(root)).toBe('light');
    root.setAttribute(THEME_ATTRIBUTE, 'sepia');
    expect(documentTheme(root)).toBe('light');
    applyTheme(root, 'dark');
    expect(root.getAttribute(THEME_ATTRIBUTE)).toBe('dark');
    expect(documentTheme(root)).toBe('dark');
  });

  it('reports a change once, and never a value that stayed', async () => {
    const root = document.createElement('div');
    applyTheme(root, 'light');
    const listener = vi.fn();
    const stop = onThemeChange(listener, root);
    applyTheme(root, 'light');
    await mutations();
    expect(listener).not.toHaveBeenCalled();
    applyTheme(root, 'dark');
    await mutations();
    expect(listener).toHaveBeenCalledExactlyOnceWith('dark');
    // Rewritten with the same value by something else: still no report.
    root.setAttribute(THEME_ATTRIBUTE, 'dark');
    await mutations();
    expect(listener).toHaveBeenCalledOnce();
    stop();
    applyTheme(root, 'light');
    await mutations();
    expect(listener).toHaveBeenCalledOnce();
  });
});

describe('startAppearance', () => {
  function start(choice: () => Theme | null, scheme: SchemeQuery | null) {
    const root = document.createElement('div');
    const listeners = new Set<() => void>();
    const stop = startAppearance({
      root,
      choice,
      subscribe: (listener) => {
        listeners.add(listener);
        return () => listeners.delete(listener);
      },
      scheme,
    });
    const changed = () => {
      for (const listener of [...listeners]) {
        listener();
      }
    };
    return { root, stop, changed, listeners };
  }

  it("shows the system's theme before the settings are read, and follows the system", () => {
    const scheme = fakeScheme(true);
    const { root, stop } = start(() => null, scheme);
    expect(documentTheme(root)).toBe('dark');
    scheme.set(false);
    expect(documentTheme(root)).toBe('light');
    stop();
    expect(scheme.listeners).toBe(0);
  });

  it('applies a chosen theme at once and then ignores the system', () => {
    const scheme = fakeScheme(false);
    let choice: Theme = 'system';
    const { root, changed } = start(() => choice, scheme);
    expect(documentTheme(root)).toBe('light');
    choice = 'dark';
    changed();
    expect(documentTheme(root)).toBe('dark');
    scheme.set(false);
    expect(documentTheme(root)).toBe('dark');
    choice = 'light';
    changed();
    scheme.set(true);
    expect(documentTheme(root)).toBe('light');
    choice = 'system';
    changed();
    expect(documentTheme(root)).toBe('dark');
  });

  it('works without a system query (light, or what was chosen)', () => {
    let choice: Theme = 'system';
    const { root, changed, stop, listeners } = start(() => choice, null);
    expect(documentTheme(root)).toBe('light');
    choice = 'dark';
    changed();
    expect(documentTheme(root)).toBe('dark');
    stop();
    expect(listeners.size).toBe(0);
  });

  it('follows the setting in the app store', () => {
    const scheme = fakeScheme(false);
    const root = document.documentElement;
    const stop = startAppearance({
      root,
      choice: () => useAppStore.getState().settings.value?.appearance.theme ?? null,
      subscribe: (listener) => useAppStore.subscribe(listener),
      scheme,
    });
    expect(documentTheme()).toBe('light');
    useAppStore
      .getState()
      .actions.setSettings({ value: settingsFixture({ appearance: { theme: 'dark' } }) });
    expect(documentTheme()).toBe('dark');
    stop();
    useAppStore
      .getState()
      .actions.setSettings({ value: settingsFixture({ appearance: { theme: 'light' } }) });
    expect(documentTheme()).toBe('dark');
  });
});

describe('systemSchemeQuery', () => {
  it("asks the window for the system's dark scheme", () => {
    const query = fakeScheme(true);
    const matchMedia = vi.fn(() => query as unknown as MediaQueryList);
    expect(systemSchemeQuery({ matchMedia })).toBe(query);
    expect(matchMedia).toHaveBeenCalledWith(DARK_SCHEME_QUERY);
    expect(systemSchemeQuery({} as Pick<Window, 'matchMedia'>)).toBeNull();
  });
});
