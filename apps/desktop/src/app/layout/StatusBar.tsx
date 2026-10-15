import type { BuildConfig, Toolchain } from '@blocks2cpp/ipc-types';
import { useShallow } from 'zustand/react/shallow';

import { visibleInvisibles } from '../../panels/shared/invisibles';
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

/** The longest toolchain path the status bar shows, in UTF-16 units (as the toolchain page). */
export const MAX_TOOLCHAIN_PATH_CHARS = 1024;

/**
 * The full path of a toolchain as the status bar shows it in its tooltip and accessible name
 * (08 §8.5): characters that do not show or that reorder text become visible placeholders
 * (`⟨U+202E⟩`), so a planted compiler cannot pass for another, and a very long path is cut.
 */
export function toolchainPathText(toolchain: Toolchain): string {
  const visible = visibleInvisibles(toolchain.displayPath);
  return visible.length <= MAX_TOOLCHAIN_PATH_CHARS
    ? visible
    : `${visible.slice(0, MAX_TOOLCHAIN_PATH_CHARS - 1)}…`;
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
 * It selects only what it shows, so an edit that changes none of it does not render it again.
 */
export function StatusBar() {
  const project = useAppStore(
    useShallow((state) =>
      state.project === null
        ? null
        : {
            standard: standardLabel(state.project),
            dirty: state.project.dirty,
            fileName: state.project.fileName,
            savedAt: state.project.savedAt,
            restricted: state.project.trust.state === 'restricted',
          },
    ),
  );
  const config = useAppStore((state) => state.build.config);
  return (
    <footer className="status-bar">
      <ToolchainStatus />
      {project !== null && (
        <span className="status-item" data-testid="status-standard">
          {project.standard}
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
      {project?.restricted === true && (
        <span className="status-item status-restricted" data-testid="status-restricted">
          <LockIcon />
          Restricted Mode
        </span>
      )}
    </footer>
  );
}

/**
 * The selected toolchain, as a link to the toolchain page once a feature provides it. Its full
 * path is in the tooltip and the accessible name (08 §8.5: the person can see which compiler a
 * build will run).
 */
function ToolchainStatus() {
  const { toolchain, discovering } = useAppStore(
    useShallow((state) => ({
      toolchain: selectedToolchain(state),
      discovering: state.toolchains.discovering,
    })),
  );
  const setupPage = useRegisteredScreen('toolchainSetup');

  let content;
  const path = toolchain === null ? null : toolchainPathText(toolchain);
  if (toolchain !== null) {
    content = (
      <>
        <span className="status-icon status-ok" aria-hidden="true">
          √
        </span>
        {toolchainLabel(toolchain)}
        <span className="visually-hidden">, {path}</span>
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

  const tooltip = path === null ? {} : { title: path };
  if (setupPage === null) {
    return (
      <span className="status-item" data-testid="status-toolchain" {...tooltip}>
        {content}
      </span>
    );
  }
  return (
    <button
      type="button"
      className="status-item status-link"
      data-testid="status-toolchain"
      {...tooltip}
      onClick={() => {
        useAppStore.getState().actions.setUi({ screen: 'toolchainSetup' });
      }}
    >
      {content}
      <span className="visually-hidden"> (open the toolchain page)</span>
    </button>
  );
}
