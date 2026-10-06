/**
 * Closing a window as its close button does: argument checks, and the X11 helper's answers for a
 * process without windows (with a display, as under `xvfb-run`).
 */
import { spawnSync } from 'node:child_process';

import { describe, expect, it } from 'vitest';

import { CLOSE_WINDOW_SCRIPT, requestWindowClose, WindowCloseError } from './window';

/** Runs the X11 helper with `args`; its exit status and error output. */
function runScript(args: readonly string[], env: NodeJS.ProcessEnv = process.env) {
  const result = spawnSync('python3', ['-I', CLOSE_WINDOW_SCRIPT, ...args], {
    encoding: 'utf8',
    env,
    timeout: 30_000,
  });
  return { status: result.status, stderr: result.stderr };
}

describe('requestWindowClose', () => {
  it('refuses what is not a process ID', async () => {
    await expect(requestWindowClose(0)).rejects.toThrow(WindowCloseError);
    await expect(requestWindowClose(-3)).rejects.toThrow(WindowCloseError);
    await expect(requestWindowClose(1.5)).rejects.toThrow(WindowCloseError);
  });
});

describe.skipIf(process.platform !== 'linux')('close-window.py', () => {
  it('refuses a missing or malformed process ID', () => {
    for (const args of [[], ['abc'], ['0'], ['12', '13']]) {
      const { status, stderr } = runScript(args);
      expect(status, args.join(' ')).toBe(1);
      expect(stderr).toContain('usage');
    }
  });

  it('says so when there is no display', () => {
    const env = { ...process.env };
    delete env['DISPLAY'];
    const { status, stderr } = runScript([String(process.pid)], env);
    expect(status).toBe(1);
    expect(stderr).toMatch(/cannot open the X display|libX11 cannot be loaded/);
  });

  it.skipIf((process.env['DISPLAY'] ?? '') === '')(
    'finds no window to close for a process without one',
    () => {
      const { status, stderr } = runScript([String(process.pid)]);
      expect(status).toBe(2);
      expect(stderr).toContain('has no window to close');
    },
  );
});
