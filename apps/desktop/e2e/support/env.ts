/**
 * The harness's settings, from environment variables (e2e/README.md lists them):
 *
 * - `B2C_E2E_APP`: the app under test, built with the `e2e-hooks` feature and the e2e frontend.
 *   Default: `target/debug/blocks2cpp-desktop[.exe]` under `CARGO_TARGET_DIR` or the repository.
 * - `B2C_E2E_TAURI_DRIVER`: `tauri-driver` (default: the one on `PATH`).
 * - `B2C_E2E_NATIVE_DRIVER`: the native WebDriver for `tauri-driver --native-driver` (Windows:
 *   the `msedgedriver.exe` that e2e/scripts/msedgedriver.ps1 fetched). Default: none, so
 *   `tauri-driver` looks on `PATH`.
 * - `B2C_E2E_TOOLCHAIN_DIRS`: where the app looks for g++ (the app's own variable, passed on).
 *   Default on Linux: `/usr/bin`; required elsewhere.
 * - `B2C_E2E_ARTIFACTS`: where screenshots, logs and the Trusted Types report go. Default: a
 *   `blocks2cpp-e2e-artifacts` folder in the system's temporary folder.
 * - `B2C_E2E_NATIVE_DRIVER_LOG`: a log the native WebDriver appends to for the whole run (CI's
 *   verbose msedgedriver). A failed test keeps the part written while it ran. Default: none.
 *
 * `B2C_E2E_ROOT` and `B2C_E2E_DIALOGS` are set by the harness for each test (a fresh profile).
 */
import { tmpdir } from 'node:os';
import path from 'node:path';

/** The repository's root folder. */
export const REPOSITORY_ROOT = path.resolve(import.meta.dirname, '../../../..');

/** The variables only the harness reads; they are not passed on to the app. */
export const HARNESS_VARIABLES = [
  'B2C_E2E_APP',
  'B2C_E2E_TAURI_DRIVER',
  'B2C_E2E_NATIVE_DRIVER',
  'B2C_E2E_ARTIFACTS',
  'B2C_E2E_NATIVE_DRIVER_LOG',
] as const;

/** What the harness runs and where it writes. */
export interface HarnessSettings {
  /** The app's executable (absolute). */
  readonly app: string;
  /** The `tauri-driver` command: a path, or a name looked up on `PATH`. */
  readonly tauriDriver: string;
  /** The native WebDriver's path (absolute), or `null` to let `tauri-driver` find it. */
  readonly nativeDriver: string | null;
  /** The value of the app's `B2C_E2E_TOOLCHAIN_DIRS`. */
  readonly toolchainDirs: string;
  /** Where artifacts go (absolute). */
  readonly artifacts: string;
  /** The log the native driver appends to (absolute), or `null` when there is none. */
  readonly nativeDriverLog: string | null;
}

/** The harness's settings are not usable. */
export class HarnessSettingsError extends Error {
  override readonly name = 'HarnessSettingsError';
}

/** A variable's value, or `undefined` when it is unset or empty. */
function value(env: NodeJS.ProcessEnv, name: string): string | undefined {
  const found = env[name];
  return found === undefined || found === '' ? undefined : found;
}

/** An absolute path from a variable, or the error that it is relative. */
function absolute(env: NodeJS.ProcessEnv, name: string): string | undefined {
  const found = value(env, name);
  if (found !== undefined && !path.isAbsolute(found)) {
    throw new HarnessSettingsError(`${name} must be an absolute path`);
  }
  return found;
}

/** The harness's settings from `env` on `platform`; see the module comment. */
export function harnessSettings(
  env: NodeJS.ProcessEnv = process.env,
  platform: NodeJS.Platform = process.platform,
): HarnessSettings {
  const exe = platform === 'win32' ? '.exe' : '';
  const targetDir = absolute(env, 'CARGO_TARGET_DIR') ?? path.join(REPOSITORY_ROOT, 'target');
  const app =
    absolute(env, 'B2C_E2E_APP') ?? path.join(targetDir, 'debug', `blocks2cpp-desktop${exe}`);
  const toolchainDirs =
    value(env, 'B2C_E2E_TOOLCHAIN_DIRS') ?? (platform === 'linux' ? '/usr/bin' : undefined);
  if (toolchainDirs === undefined) {
    throw new HarnessSettingsError(
      'Set B2C_E2E_TOOLCHAIN_DIRS to the folder that holds g++ (for example MSYS2 ucrt64\\bin)',
    );
  }
  return {
    app,
    tauriDriver: value(env, 'B2C_E2E_TAURI_DRIVER') ?? `tauri-driver${exe}`,
    nativeDriver: absolute(env, 'B2C_E2E_NATIVE_DRIVER') ?? null,
    toolchainDirs,
    artifacts:
      absolute(env, 'B2C_E2E_ARTIFACTS') ?? path.join(tmpdir(), 'blocks2cpp-e2e-artifacts'),
    nativeDriverLog: absolute(env, 'B2C_E2E_NATIVE_DRIVER_LOG') ?? null,
  };
}

/**
 * The environment the app runs in: this process's, without the harness's own variables, with a
 * fresh profile (`root`), the dialog script and the toolchain folders. On Linux, WebKitGTK's
 * DMA-BUF renderer is turned off, as a virtual display has no GPU.
 */
export function appEnvironment(
  settings: HarnessSettings,
  profile: { readonly root: string; readonly dialogs: string },
  env: NodeJS.ProcessEnv = process.env,
  platform: NodeJS.Platform = process.platform,
): NodeJS.ProcessEnv {
  const result: NodeJS.ProcessEnv = { ...env };
  for (const name of HARNESS_VARIABLES) {
    Reflect.deleteProperty(result, name);
  }
  result['B2C_E2E_ROOT'] = profile.root;
  result['B2C_E2E_DIALOGS'] = profile.dialogs;
  result['B2C_E2E_TOOLCHAIN_DIRS'] = settings.toolchainDirs;
  if (platform === 'linux') {
    result['WEBKIT_DISABLE_DMABUF_RENDERER'] ??= '1';
  }
  return result;
}
