/** The entry point (src/main.tsx) starts the runtime once and mounts the root into index.html. */
import { act, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { AppRuntime, AppRuntimeOptions } from '../app/bootstrap';

const boot = vi.hoisted(() => ({
  options: [] as AppRuntimeOptions[],
  start: vi.fn(() => Promise.resolve({ kind: 'ready' as const })),
}));

vi.mock('../app/bootstrap', () => ({
  createAppRuntime: (options: AppRuntimeOptions) => {
    boot.options.push(options);
    return { start: boot.start } as unknown as AppRuntime;
  },
}));
vi.mock('../app/Root', () => ({
  Root: () => <p>the app</p>,
}));

beforeEach(() => {
  // Each test imports the entry point afresh, as a page load would.
  vi.resetModules();
  boot.options = [];
});

afterEach(() => {
  document.getElementById('root')?.remove();
});

describe('main', () => {
  it('renders the root into the element with the id "root" and starts the runtime once', async () => {
    const root = document.createElement('div');
    root.id = 'root';
    document.body.append(root);

    await act(async () => {
      await import('../main');
    });

    expect(root.contains(screen.getByText('the app'))).toBe(true);
    expect(boot.start).toHaveBeenCalledTimes(1);
    expect(boot.options).toHaveLength(1);
    // Outside Tauri, in development, the window runs without a backend.
    expect(boot.options[0]?.withoutBackend).toBe(true);
    expect(boot.options[0]?.window).toBe(window);
  });

  it('stops with a clear error when index.html has no root element', async () => {
    await expect(import('../main')).rejects.toThrow('index.html has no element with the id "root"');
    expect(boot.start).not.toHaveBeenCalled();
  });
});
