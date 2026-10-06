import path from 'node:path';

import { describe, expect, it } from 'vitest';

import { appEnvironment, harnessSettings, HarnessSettingsError, REPOSITORY_ROOT } from './env';

describe('harnessSettings', () => {
  it('defaults to the debug build, tauri-driver on PATH and /usr/bin on Linux', () => {
    const settings = harnessSettings({}, 'linux');
    expect(settings.app).toBe(path.join(REPOSITORY_ROOT, 'target', 'debug', 'blocks2cpp-desktop'));
    expect(settings.tauriDriver).toBe('tauri-driver');
    expect(settings.nativeDriver).toBeNull();
    expect(settings.toolchainDirs).toBe('/usr/bin');
    expect(path.isAbsolute(settings.artifacts)).toBe(true);
  });

  it('follows CARGO_TARGET_DIR and the variables', () => {
    const root = path.resolve('/opt/e2e');
    const settings = harnessSettings(
      {
        CARGO_TARGET_DIR: path.join(root, 'target'),
        B2C_E2E_TAURI_DRIVER: 'my-driver',
        B2C_E2E_NATIVE_DRIVER: path.join(root, 'msedgedriver.exe'),
        B2C_E2E_TOOLCHAIN_DIRS: path.join(root, 'ucrt64', 'bin'),
        B2C_E2E_ARTIFACTS: path.join(root, 'artifacts'),
      },
      'win32',
    );
    expect(settings.app).toBe(path.join(root, 'target', 'debug', 'blocks2cpp-desktop.exe'));
    expect(settings.tauriDriver).toBe('my-driver');
    expect(settings.nativeDriver).toBe(path.join(root, 'msedgedriver.exe'));
    expect(settings.toolchainDirs).toBe(path.join(root, 'ucrt64', 'bin'));
    expect(settings.artifacts).toBe(path.join(root, 'artifacts'));
    expect(harnessSettings({ B2C_E2E_APP: path.join(root, 'app') }, 'linux').app).toBe(
      path.join(root, 'app'),
    );
  });

  it('needs the toolchain folders outside Linux and absolute paths', () => {
    expect(() => harnessSettings({}, 'win32')).toThrow(HarnessSettingsError);
    expect(() => harnessSettings({ B2C_E2E_APP: 'relative/app' }, 'linux')).toThrow(
      'B2C_E2E_APP must be an absolute path',
    );
    expect(() => harnessSettings({ B2C_E2E_ARTIFACTS: 'out' }, 'linux')).toThrow('absolute');
    // An empty variable counts as unset.
    expect(harnessSettings({ B2C_E2E_APP: '' }, 'linux').app).toContain('blocks2cpp-desktop');
  });
});

describe('appEnvironment', () => {
  it('gives the app a fresh profile and leaves out the harness variables', () => {
    const settings = harnessSettings({}, 'linux');
    const env = appEnvironment(
      settings,
      { root: '/tmp/x/profile', dialogs: '/tmp/x/dialogs.json' },
      { PATH: '/usr/bin', B2C_E2E_APP: '/a', B2C_E2E_ROOT: '/old', B2C_E2E_ARTIFACTS: '/b' },
      'linux',
    );
    expect(env).toEqual({
      PATH: '/usr/bin',
      B2C_E2E_ROOT: '/tmp/x/profile',
      B2C_E2E_DIALOGS: '/tmp/x/dialogs.json',
      B2C_E2E_TOOLCHAIN_DIRS: '/usr/bin',
      WEBKIT_DISABLE_DMABUF_RENDERER: '1',
    });
    const windows = appEnvironment(
      { ...settings, toolchainDirs: 'C:\\msys64\\ucrt64\\bin' },
      { root: 'C:\\t\\profile', dialogs: 'C:\\t\\d.json' },
      {},
      'win32',
    );
    expect(windows['WEBKIT_DISABLE_DMABUF_RENDERER']).toBeUndefined();
    expect(windows['B2C_E2E_TOOLCHAIN_DIRS']).toBe('C:\\msys64\\ucrt64\\bin');
  });
});
