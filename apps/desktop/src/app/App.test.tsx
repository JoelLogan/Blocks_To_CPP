import { act, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { expectNoAxeViolations } from '../test/axe';
import { App } from './App';

/**
 * Blockly is replaced by a stand-in that records what the workspace component asks of it, so the
 * tests can check the wiring (options, theme, resizing, clean-up). src/test/blockly-environment
 * checks the same component against the real Blockly.
 */
const blockly = vi.hoisted(() => {
  const workspace = { setTheme: vi.fn(), dispose: vi.fn() };
  return {
    workspace,
    inject: vi.fn(() => workspace),
    svgResize: vi.fn(),
    setLocale: vi.fn(),
    Theme: { defineTheme: vi.fn((name: string) => ({ name })) },
    Themes: { Zelos: { name: 'zelos' } },
  };
});
vi.mock('blockly/core', () => blockly);
vi.mock('blockly/msg/en', () => ({}));

/** The colour-scheme media query, which the workspace module reads when it is first imported. */
const colourScheme = vi.hoisted(() => {
  const listeners = new Set<() => void>();
  const query = {
    matches: false,
    addEventListener: (_type: string, listener: () => void) => listeners.add(listener),
    removeEventListener: (_type: string, listener: () => void) => listeners.delete(listener),
  };
  window.matchMedia = () => query as unknown as MediaQueryList;
  return {
    listeners,
    /** Switches the system colour scheme, as the user would in the OS settings. */
    set(dark: boolean) {
      query.matches = dark;
      for (const listener of listeners) {
        listener();
      }
    },
  };
});

const ipc = vi.hoisted(() => ({ appVersion: vi.fn<() => Promise<string>>() }));
vi.mock('../lib/ipc', () => ipc);

/** A ResizeObserver whose callback the test runs by hand (happy-dom computes no layout). */
class FakeResizeObserver {
  static instances: FakeResizeObserver[] = [];
  readonly callback: () => void;
  readonly observed: Element[] = [];
  disconnected = false;

  constructor(callback: () => void) {
    this.callback = callback;
    FakeResizeObserver.instances.push(this);
  }

  observe(element: Element): void {
    this.observed.push(element);
  }

  disconnect(): void {
    this.disconnected = true;
  }
}

/** A promise the test settles when it wants to. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  FakeResizeObserver.instances = [];
  vi.stubGlobal('ResizeObserver', FakeResizeObserver);
  ipc.appVersion.mockReturnValue(new Promise<string>(() => undefined));
});

afterEach(() => {
  colourScheme.set(false);
});

describe('App shell', () => {
  it('shows the regions of the main window, each with a name', () => {
    render(<App />);

    expect(screen.getByRole('banner').textContent).toBe('Blocks2Cpp');
    expect(screen.getByRole('navigation', { name: 'Block categories' })).toBeTruthy();
    expect(screen.getByRole('main', { name: 'Block workspace' })).toBeTruthy();
    expect(screen.getByRole('complementary', { name: 'Generated C++' })).toBeTruthy();
    expect(screen.getByRole('contentinfo')).toBeTruthy();

    const dock = screen.getByRole('region', { name: 'Console, problems and build output' });
    expect(within(dock).getByText('Console')).toBeTruthy();
    expect(within(dock).getByText('Problems')).toBeTruthy();
    expect(within(dock).getByText('Build output')).toBeTruthy();
  });

  it('has no accessibility problems', async () => {
    const { container } = render(<App />);
    await expectNoAxeViolations(container);
  });

  it('shows the version the backend reports in the status bar', async () => {
    ipc.appVersion.mockResolvedValue('1.2.3');
    render(<App />);

    expect(await screen.findByText('Blocks2Cpp 1.2.3')).toBeTruthy();
    expect(ipc.appVersion).toHaveBeenCalledTimes(1);
  });

  it('shows no version outside the desktop app, where the backend cannot be reached', async () => {
    const version = deferred<string>();
    ipc.appVersion.mockReturnValue(version.promise);
    render(<App />);

    await act(async () => {
      version.reject(new Error('not running inside Tauri'));
      await version.promise.catch(() => undefined);
    });
    expect(within(screen.getByRole('contentinfo')).getByText('Blocks2Cpp')).toBeTruthy();
  });

  it('ignores a version that arrives after the window is gone', async () => {
    const version = deferred<string>();
    ipc.appVersion.mockReturnValue(version.promise);
    const { unmount } = render(<App />);
    unmount();

    // Nothing may update the unmounted component (React would report it).
    const consoleError = vi.spyOn(console, 'error');
    await act(async () => {
      version.resolve('1.2.3');
      await version.promise;
    });
    expect(consoleError).not.toHaveBeenCalled();
  });
});

describe('Block workspace', () => {
  it('injects Blockly with the Zelos renderer, bundled media and no sounds', () => {
    render(<App />);

    expect(blockly.inject).toHaveBeenCalledTimes(1);
    const [host, options] = blockly.inject.mock.calls[0] as unknown as [
      HTMLElement,
      Record<string, unknown>,
    ];
    expect(host.className).toBe('blockly-host');
    expect(screen.getByRole('main').contains(host)).toBe(true);
    expect(options).toMatchObject({
      renderer: 'zelos',
      sounds: false,
      theme: { name: 'b2c-light' },
      // Tests run like the dev server, which serves Blockly's media from the package.
      media: '/node_modules/blockly/media/',
    });
  });

  it('starts in the dark theme when the system uses a dark colour scheme', () => {
    colourScheme.set(true);
    render(<App />);

    expect(blockly.inject.mock.calls[0]).toMatchObject([
      expect.anything(),
      { theme: { name: 'b2c-dark' } },
    ]);
  });

  it('follows changes of the system colour scheme', () => {
    render(<App />);

    colourScheme.set(true);
    expect(blockly.workspace.setTheme).toHaveBeenLastCalledWith({ name: 'b2c-dark' });
    colourScheme.set(false);
    expect(blockly.workspace.setTheme).toHaveBeenLastCalledWith({ name: 'b2c-light' });
  });

  it('resizes the workspace when its panel changes size', () => {
    render(<App />);

    const [observer] = FakeResizeObserver.instances;
    expect(observer?.observed).toEqual([screen.getByRole('main').firstElementChild]);
    observer?.callback();
    expect(blockly.svgResize).toHaveBeenCalledWith(blockly.workspace);
  });

  it('releases the workspace and its listeners when the window closes', () => {
    const { unmount } = render(<App />);
    expect(colourScheme.listeners.size).toBe(1);

    unmount();

    expect(blockly.workspace.dispose).toHaveBeenCalledTimes(1);
    expect(FakeResizeObserver.instances[0]?.disconnected).toBe(true);
    expect(colourScheme.listeners.size).toBe(0);
  });
});
