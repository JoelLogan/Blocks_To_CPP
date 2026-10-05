import { beforeEach, describe, expect, it, vi } from 'vitest';

import { appVersion } from './ipc';

/** Tauri's `invoke`, which outside the desktop app has no backend to talk to. */
const tauri = vi.hoisted(() => ({ invoke: vi.fn<(command: string) => Promise<unknown>>() }));
vi.mock('@tauri-apps/api/core', () => tauri);

beforeEach(() => {
  tauri.invoke.mockResolvedValue(undefined);
});

describe('appVersion', () => {
  it('asks the backend with the app_version command and no arguments', async () => {
    tauri.invoke.mockResolvedValue('0.1.0');

    await expect(appVersion()).resolves.toBe('0.1.0');
    expect(tauri.invoke).toHaveBeenCalledTimes(1);
    expect(tauri.invoke).toHaveBeenCalledWith('app_version');
  });

  it('passes a failed call on to the caller', async () => {
    const failure = new Error('the isolation hook dropped the message');
    tauri.invoke.mockRejectedValue(failure);

    await expect(appVersion()).rejects.toBe(failure);
  });
});
