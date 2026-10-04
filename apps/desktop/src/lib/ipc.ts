/**
 * The typed client for the backend's IPC commands (docs/spec/02-architecture.md §2.5).
 * Every command the editor calls goes through this module. The backend permits each one
 * in src-tauri/capabilities/, and the isolation hook in src-tauri/isolation/ checks its
 * arguments. Keep all three in step.
 */
import { invoke } from '@tauri-apps/api/core';

/** The app's version, as the backend reports it. */
export function appVersion(): Promise<string> {
  return invoke<string>('app_version');
}
