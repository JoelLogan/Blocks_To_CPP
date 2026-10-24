/**
 * Closing the app's window the way a person does, with the window's close button: the system
 * sends the window a close request, which the app handles (docs/spec/02-architecture.md §2.5:
 * with unsaved changes it asks first; otherwise the window closes and the app exits, killing
 * every build and program it runs, 07 §7.6.2).
 *
 * WebDriver's "close window" cannot be used: for a Tauri app it only destroys the web view
 * (WebKitGTK) or its container (WebView2), and the app keeps running without a page.
 *
 * - **Linux:** ./close-window.py sends the ICCCM `WM_DELETE_WINDOW` message to the app's windows,
 *   as a window manager does (the virtual display has none).
 * - **Windows:** `Process.CloseMainWindow()` posts `WM_CLOSE` to the app's main window, as the
 *   title bar's close button does.
 */
import { execFile } from 'node:child_process';
import path from 'node:path';

import { powershell } from './processes';

/** The X11 helper, next to this file. */
export const CLOSE_WINDOW_SCRIPT = path.join(import.meta.dirname, 'close-window.py');

/** How long asking for the close may take (not the app's reaction to it). */
const REQUEST_TIMEOUT_MS = 30_000;

/** The window close could not be requested. */
export class WindowCloseError extends Error {
  override readonly name = 'WindowCloseError';
}

/** Runs the X11 helper for `pid`. */
function closeX11Windows(pid: number): Promise<void> {
  return new Promise((resolve, reject) => {
    // `-I`: isolated mode, so nothing next to the script or in the environment changes Python.
    execFile(
      'python3',
      ['-I', CLOSE_WINDOW_SCRIPT, String(pid)],
      { timeout: REQUEST_TIMEOUT_MS, encoding: 'utf8' },
      (error, _stdout, stderr) => {
        if (error === null) {
          resolve();
        } else {
          reject(
            new WindowCloseError(
              `The app's window could not be closed: ${stderr.trim() || error.message}`,
              { cause: error },
            ),
          );
        }
      },
    );
  });
}

/** Posts `WM_CLOSE` to the main window of process `pid`. */
async function closeMainWindow(pid: number): Promise<void> {
  const result = (
    await powershell(
      `$p = Get-Process -Id ${String(pid)} -ErrorAction Stop; ` +
        "if ($p.MainWindowHandle -eq 0) { 'nowindow' } " +
        "elseif ($p.CloseMainWindow()) { 'asked' } else { 'refused' }",
    )
  ).trim();
  if (result !== 'asked') {
    throw new WindowCloseError(`The app's main window could not be closed (${result})`);
  }
}

/**
 * Asks the windows of process `pid` to close, as their close button does. Returns once the request
 * is sent; what the app does then (a question, or exiting) is up to the test to wait for.
 *
 * @throws WindowCloseError when the process has no window to close or the request failed.
 */
export async function requestWindowClose(pid: number): Promise<void> {
  if (!Number.isSafeInteger(pid) || pid <= 0) {
    throw new WindowCloseError(`Not a process ID: ${String(pid)}`);
  }
  if (process.platform === 'win32') {
    await closeMainWindow(pid);
  } else {
    await closeX11Windows(pid);
  }
}
