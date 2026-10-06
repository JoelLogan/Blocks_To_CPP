/**
 * The entry point (src/main.tsx) installs the end-to-end hook only in the frontend built for the
 * E2E tests (`vite build --mode e2e`), and starts the Trusted Types collector in every build.
 */
import { act } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { AppRuntime } from '../app/bootstrap';
import { E2E_HOOK_NAME } from './contract';

const boot = vi.hoisted(() => ({
  start: vi.fn(() => Promise.resolve({ kind: 'ready' as const })),
  getPhase: vi.fn(() => ({ kind: 'ready' as const })),
}));

vi.mock('../app/bootstrap', () => ({
  createAppRuntime: () => ({ start: boot.start, getPhase: boot.getPhase }) as unknown as AppRuntime,
}));
vi.mock('../app/Root', () => ({
  Root: () => <p>the app</p>,
}));

beforeEach(() => {
  vi.resetModules();
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
  const root = document.createElement('div');
  root.id = 'root';
  document.body.append(root);
});

afterEach(() => {
  document.getElementById('root')?.remove();
  Reflect.deleteProperty(window, E2E_HOOK_NAME);
});

/** The hook on `window`, once the entry point's dynamic import has run. */
function installedHook(): { ready(): boolean } | undefined {
  return (window as unknown as Record<string, { ready(): boolean } | undefined>)[E2E_HOOK_NAME];
}

describe('main in e2e mode', () => {
  it('installs window.__B2C_E2E__, which reads the runtime', async () => {
    vi.stubEnv('MODE', 'e2e');
    await act(async () => {
      await import('../main');
    });
    await vi.waitFor(() => {
      expect(installedHook()).toBeDefined();
    });
    expect(installedHook()?.ready()).toBe(true);
    expect(boot.getPhase).toHaveBeenCalled();
  });

  it('leaves the hook out in every other mode', async () => {
    vi.stubEnv('MODE', 'production');
    await act(async () => {
      await import('../main');
    });
    // Give a dynamic import the chance it would have had.
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(installedHook()).toBeUndefined();
  });
});
