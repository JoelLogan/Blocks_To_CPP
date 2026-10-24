/**
 * Whether Run and Build are available, and why not (docs/spec/04-user-interface.md §4.1, §4.4,
 * §4.6; 07 §7.2, §7.6.1). The backend checks the same preconditions again and refuses on its own;
 * the gate only keeps the toolbar honest and explains itself.
 */
import type { OnErrors } from '@blocks2cpp/ipc-types';

import { hasUsableToolchain, liveErrors } from './store/selectors';
import type { AppData } from './store/state';

/**
 * Why Run and Build are held back, or wait: `noToolchain` once the compiler search has ended
 * without a usable g++, and `discovering` while the first search runs and no usable g++ is known
 * yet. Run and Build stay enabled then: the build waits for the search (at most 30 s, 07 §7.2).
 */
export type RunGateReason = 'noProject' | 'restricted' | 'noToolchain' | 'errors' | 'discovering';

/** The result of {@link runGate}. */
export interface RunGate {
  /** Whether the toolbar button is enabled. */
  enabled: boolean;
  /** The first reason that applies, in the order of {@link RunGateReason}, or `null`. */
  reason: RunGateReason | null;
  /** How many errors the live analysis reports. */
  errorCount: number;
  /**
   * The *Run on errors* setting: with `showProblems`, Run stays enabled while there are errors
   * (and shows the first one instead of building).
   */
  onErrorsMode: OnErrors;
}

/** The setting's value before the settings were read (its default, 05 §5.9). */
const DEFAULT_ON_ERRORS: OnErrors = 'disableRun';

/** Whether Run and Build are available for `state`. */
export function runGate(
  state: Pick<AppData, 'project' | 'toolchains' | 'analysis' | 'settings'>,
): RunGate {
  const errorCount = liveErrors(state).length;
  const onErrorsMode = state.settings.value?.run.onErrors ?? DEFAULT_ON_ERRORS;
  const gate = (enabled: boolean, reason: RunGateReason | null): RunGate => ({
    enabled,
    reason,
    errorCount,
    onErrorsMode,
  });

  if (state.project === null) {
    return gate(false, 'noProject');
  }
  if (state.project.trust.state === 'restricted') {
    return gate(false, 'restricted');
  }
  const usable = hasUsableToolchain(state);
  if (!usable && !state.toolchains.discovering) {
    return gate(false, 'noToolchain');
  }
  if (errorCount > 0) {
    return gate(onErrorsMode === 'showProblems', 'errors');
  }
  if (!usable) {
    return gate(true, 'discovering');
  }
  return gate(true, null);
}

/** Which toolbar button a message is for. */
export type GatedAction = 'run' | 'build';

/** `1 error` or `2 errors`. */
export function countErrors(count: number): string {
  return count === 1 ? '1 error' : `${String(count)} errors`;
}

/**
 * The tooltip of the Run or Build button: what it does, or why it is held back and what clicking
 * it does instead.
 */
export function runGateMessage(gate: RunGate, action: GatedAction): string {
  switch (gate.reason) {
    case 'noProject':
      return 'Open or create a project first';
    case 'restricted':
      return action === 'run'
        ? 'Restricted Mode: trust this project to run it'
        : 'Restricted Mode: trust this project to build it';
    case 'noToolchain':
      return 'No g++ found – click to set one up';
    case 'errors':
      return `${countErrors(gate.errorCount)} – click to see the first`;
    case 'discovering':
      return action === 'run'
        ? 'Looking for a C++ compiler (g++): Run starts once one is found (F5)'
        : 'Looking for a C++ compiler (g++): the build starts once one is found (Ctrl+B)';
    case null:
      return action === 'run' ? 'Build if needed, then run (F5)' : 'Build (Ctrl+B)';
  }
}
