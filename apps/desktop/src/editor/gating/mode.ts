/**
 * The editor's mode from the app's state (./gate.ts): the expert-mode settings, the open project's
 * C++ standard and *Show advanced blocks* option, and the probed capabilities of the toolchain a
 * build would use.
 */
import type { AppData } from '../../app/store';
import { selectedToolchain } from '../../app/store/selectors';
import type { EditorMode } from './gate';

/** The standard while no project is open, when the settings are not read yet either (05 §5.9). */
const DEFAULT_STANDARD = 'c++20';

/**
 * The editor's mode. *Show advanced blocks* is on when the machine setting is (M3-D4) or the open
 * project asks for it (`project.options.showAdvanced`, decision W4A-2 of the W4 contract).
 */
export function editorMode(
  state: Pick<AppData, 'settings' | 'project' | 'toolchains'>,
): EditorMode {
  const settings = state.settings.value;
  const project = state.project?.document.project;
  return {
    labels: settings?.editor.blockLabels ?? 'friendly',
    showAdvanced:
      (settings?.editor.showAdvanced ?? false) || project?.options.showAdvanced === true,
    standard: project?.language.standard ?? settings?.newProject.standard ?? DEFAULT_STANDARD,
    toolchain: selectedToolchain(state)?.capabilities ?? null,
  };
}

/**
 * A short key that changes exactly when what the mode offers or how blocks read can change, for a
 * rebuild check (the toolbox's, W2's plugin): `friendly|basic|c++20|fmt` and the like.
 */
export function modeKey(mode: EditorMode): string {
  const toolchain =
    mode.toolchain === null ? 'unknown' : mode.toolchain.stdFormat ? 'fmt' : 'nofmt';
  return [mode.labels, mode.showAdvanced ? 'advanced' : 'basic', mode.standard, toolchain].join(
    '|',
  );
}
