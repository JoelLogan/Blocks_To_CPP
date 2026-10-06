import type { BuildConfig, Toolchain } from '@blocks2cpp/ipc-types';
import { useShallow } from 'zustand/react/shallow';

import { useRegisteredScreen } from '../screens';
import { useAppStore } from '../store';
import { selectedToolchain } from '../store/selectors';
import type { ProjectState } from '../store/state';
import { LockIcon } from '../ui/icons';

const CONFIG_LABELS: Record<BuildConfig, string> = { debug: 'Debug', release: 'Release' };

/** How the status bar names a toolchain: `g++ 15.2.0 (MSYS2 UCRT64)`. */
export function toolchainLabel(toolchain: Toolchain): string {
  const name = toolchain.version === null ? 'g++' : `g++ ${toolchain.version}`;
  return toolchain.flavor === null ? name : `${name} (${toolchain.flavor})`;
}

/** How the status bar names the project's C++ standard: `C++20`, or `GNU++20` with extensions. */
export function standardLabel(project: ProjectState): string {
  const { standard, gnuExtensions } = project.document.project.language;
  const version = standard.replace(/^c\+\+/, '');
  return gnuExtensions === true ? `GNU++${version}` : `C++${version}`;
}

/** `HH:MM` in local time (24-hour), or `null` for a timestamp that does not parse. */
export function localTime(timestamp: string): string | null {
  const date = new Date(timestamp);
  if (Number.isNaN(date.getTime())) {
    return null;
  }
  const pad = (value: number) => String(value).padStart(2, '0');
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/**
 * The save state, as the top bar's unsaved-changes marker has it (04 §4.1, §4.10): `Unsaved` while
 * there are unsaved changes or for a project that has no file yet; otherwise `Saved 10:42` after a
 * save in this session, and `Saved` for a file that is as it was opened (the backend sends no
 * modification time).
 */
export function saveStateLabel(
  project: Pick<ProjectState, 'dirty' | 'fileName' | 'savedAt'>,
): string {
  if (project.dirty || project.fileName === null) {
    return 'Unsaved';
  }
  const time = project.savedAt === null ? null : localTime(project.savedAt);
  return time === null ? 'Saved' : `Saved ${time}`;
}

/**
 * The status bar (docs/spec/04-user-interface.md §4.1): the toolchain a build would use, the
 * project's C++ standard, the configuration, the save state and the Restricted Mode indicator.
 */
export function StatusBar() {
  const project = useAppStore((state) => state.project);
  const config = useAppStore((state) => state.build.config);
  return (
    <footer className="status-bar">
      <ToolchainStatus />
      {project !== null && (
        <span className="status-item" data-testid="status-standard">
          {standardLabel(project)}
        </span>
      )}
      <span className="status-item" data-testid="status-config">
        {CONFIG_LABELS[config]}
      </span>
      {project !== null && (
        <span className="status-item" data-testid="status-save">
          {saveStateLabel(project)}
        </span>
      )}
      {project?.trust.state === 'restricted' && (
        <span className="status-item status-restricted" data-testid="status-restricted">
          <LockIcon />
          Restricted Mode
        </span>
      )}
    </footer>
  );
}

/** The selected toolchain, as a link to the toolchain page once a feature provides it. */
function ToolchainStatus() {
  const { toolchain, discovering } = useAppStore(
    useShallow((state) => ({
      toolchain: selectedToolchain(state),
      discovering: state.toolchains.discovering,
    })),
  );
  const setupPage = useRegisteredScreen('toolchainSetup');

  let content;
  if (toolchain !== null) {
    content = (
      <>
        <span className="status-icon status-ok" aria-hidden="true">
          √
        </span>
        {toolchainLabel(toolchain)}
      </>
    );
  } else if (discovering) {
    content = 'Looking for g++…';
  } else {
    content = (
      <>
        <span className="status-icon status-error" aria-hidden="true">
          ✖
        </span>
        No g++ found
      </>
    );
  }

  if (setupPage === null) {
    return (
      <span className="status-item" data-testid="status-toolchain">
        {content}
      </span>
    );
  }
  return (
    <button
      type="button"
      className="status-item status-link"
      data-testid="status-toolchain"
      onClick={() => {
        useAppStore.getState().actions.setUi({ screen: 'toolchainSetup' });
      }}
    >
      {content}
      <span className="visually-hidden"> (open the toolchain page)</span>
    </button>
  );
}
