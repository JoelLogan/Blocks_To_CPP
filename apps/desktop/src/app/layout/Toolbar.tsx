import type { BuildConfig } from '@blocks2cpp/ipc-types';
import { useId } from 'react';
import { useShallow } from 'zustand/react/shallow';

import { activateGated, SHELL } from '../actions';
import { triggerCommand } from '../commands';
import { type GatedAction, runGate, runGateMessage } from '../runGate';
import { useRegisteredScreen } from '../screens';
import { SHORTCUTS } from '../shortcuts';
import { useAppStore } from '../store';
import { Hint } from '../ui/Hint';
import { GearIcon } from '../ui/icons';
import { MainMenu } from './MainMenu';

/** The top bar's name when no project is open. */
export const APP_NAME = 'Blocks2Cpp';

const CONFIG_LABELS: Record<BuildConfig, string> = { debug: 'Debug', release: 'Release' };

/**
 * The top bar (docs/spec/04-user-interface.md §4.1): the main menu `≡`, the project name with `•`
 * while there are unsaved changes, the session's Debug/Release choice, ► Run, ■ Stop, Build and
 * Settings.
 */
export function Toolbar() {
  return (
    <header className="toolbar">
      <MainMenu />
      <ProjectTitle />
      <div className="toolbar-group" role="group" aria-label="Build and run">
        <ConfigSelect />
        <GatedButton action="run" icon="►" label="Run" />
        <StopButton />
        <GatedButton action="build" icon={null} label="Build" />
      </div>
      <SettingsButton />
    </header>
  );
}

/** The project's name, with the unsaved-changes marker. */
function ProjectTitle() {
  const { name, dirty } = useAppStore(
    useShallow((state) => ({
      name: state.project?.document.project.name ?? null,
      dirty: state.project?.dirty ?? false,
    })),
  );
  return (
    <div className="toolbar-title">
      <span className="app-mark" aria-hidden="true" />
      {name === null ? (
        <h1 className="project-name">{APP_NAME}</h1>
      ) : (
        <h1 className="project-name" data-testid="project-name">
          <bdi>{name}</bdi>
          {dirty && (
            <>
              <span className="dirty-marker" aria-hidden="true">
                {' •'}
              </span>
              <span className="visually-hidden"> (unsaved changes)</span>
            </>
          )}
        </h1>
      )}
    </div>
  );
}

/** The Debug/Release choice, which lasts for the session and is never saved (§4.1). */
function ConfigSelect() {
  const id = useId();
  const config = useAppStore((state) => state.build.config);
  return (
    <>
      <label className="visually-hidden" htmlFor={id}>
        Build configuration
      </label>
      <select
        id={id}
        className="toolbar-select"
        value={config}
        onChange={(event) => {
          const value = event.target.value;
          if (value === 'debug' || value === 'release') {
            useAppStore.getState().actions.setBuild({ config: value });
          }
        }}
      >
        {(['debug', 'release'] as const).map((value) => (
          <option key={value} value={value}>
            {CONFIG_LABELS[value]}
          </option>
        ))}
      </select>
    </>
  );
}

/**
 * Run or Build. A held-back button stays focusable (`aria-disabled`), so its hint is reachable by
 * keyboard, and pressing it does what the hint says: show the first error, or open the toolchain
 * setup page.
 */
function GatedButton({
  action,
  icon,
  label,
}: {
  action: GatedAction;
  icon: string | null;
  label: string;
}) {
  const gate = useAppStore(useShallow(runGate));
  const discovering = useAppStore((state) => state.toolchains.discovering);
  const message = runGateMessage(gate, action, { discovering });
  const descriptionId = useId();
  return (
    <>
      <Hint text={message}>
        <button
          type="button"
          className="toolbar-button"
          data-testid={`toolbar-${action}`}
          aria-disabled={!gate.enabled}
          aria-describedby={descriptionId}
          aria-keyshortcuts={SHORTCUTS[action].aria}
          onClick={() => {
            activateGated(action, SHELL);
          }}
        >
          {icon !== null && (
            <span className="toolbar-icon" aria-hidden="true">
              {icon}
            </span>
          )}
          {label}
        </button>
      </Hint>
      <span id={descriptionId} className="visually-hidden">
        {message}
      </span>
    </>
  );
}

/** ■ Stop: stops the running program or cancels the running build. */
function StopButton() {
  const { running, building } = useAppStore(
    useShallow((state) => ({
      running: state.run.status === 'starting' || state.run.status === 'running',
      building: state.build.status === 'building',
    })),
  );
  const enabled = running || building;
  const message = running
    ? `Stop the program (${SHORTCUTS.stop.label})`
    : building
      ? `Stop the build (${SHORTCUTS.stop.label})`
      : 'Nothing is running';
  const descriptionId = useId();
  return (
    <>
      <Hint text={message}>
        <button
          type="button"
          className="toolbar-button"
          data-testid="toolbar-stop"
          aria-disabled={!enabled}
          aria-describedby={descriptionId}
          aria-keyshortcuts={SHORTCUTS.stop.aria}
          onClick={() => {
            if (enabled) {
              triggerCommand('run.stop');
            }
          }}
        >
          <span className="toolbar-icon" aria-hidden="true">
            ■
          </span>
          Stop
        </button>
      </Hint>
      <span id={descriptionId} className="visually-hidden">
        {message}
      </span>
    </>
  );
}

/** Settings, shown once a feature provides the Settings page. */
function SettingsButton() {
  const settingsPage = useRegisteredScreen('settings');
  if (settingsPage === null) {
    return null;
  }
  return (
    <button
      type="button"
      className="toolbar-button toolbar-settings"
      onClick={() => {
        triggerCommand('settings.open');
      }}
    >
      <GearIcon />
      Settings
    </button>
  );
}
