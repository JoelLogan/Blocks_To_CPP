import { describe, expect, it } from 'vitest';

import { countErrors, runGate, runGateMessage } from './runGate';
import { initialAppData } from './store/state';
import {
  diagnosticFixture,
  previewFixture,
  projectFixture,
  settingsFixture,
  toolchainFixture,
} from './testing/fixtures';

/** A state in which Run is available: a trusted project, a usable compiler, no errors. */
function readyState() {
  const state = initialAppData();
  state.project = projectFixture();
  state.toolchains.list = [toolchainFixture()];
  state.settings.value = settingsFixture();
  state.analysis.preview = previewFixture([]);
  return state;
}

const restricted = {
  state: 'restricted',
  source: null,
  restrictedReason: 'noRecord',
  markOfTheWeb: false,
} as const;

describe('runGate', () => {
  it('is open for a trusted project with a compiler and no errors', () => {
    expect(runGate(readyState())).toEqual({
      enabled: true,
      reason: null,
      errorCount: 0,
      onErrorsMode: 'disableRun',
    });
  });

  it('is closed without a project', () => {
    const state = readyState();
    state.project = null;
    expect(runGate(state)).toMatchObject({ enabled: false, reason: 'noProject' });
  });

  it('is closed in Restricted Mode', () => {
    const state = readyState();
    state.project = projectFixture({ trust: restricted });
    expect(runGate(state)).toMatchObject({ enabled: false, reason: 'restricted' });
  });

  it('is closed without a usable compiler', () => {
    const state = readyState();
    state.toolchains.list = [toolchainFixture({ usable: false })];
    expect(runGate(state)).toMatchObject({ enabled: false, reason: 'noToolchain' });

    state.toolchains.list = [];
    expect(runGate(state)).toMatchObject({ enabled: false, reason: 'noToolchain' });
  });

  it('is closed while there are errors, with "Disable Run" (the default)', () => {
    const state = readyState();
    state.settings.value = null;
    state.analysis.preview = previewFixture([
      diagnosticFixture(),
      diagnosticFixture({ severity: 'warning', code: 'B2C-W0501' }),
      diagnosticFixture({ code: 'B2C-E0301' }),
    ]);
    expect(runGate(state)).toEqual({
      enabled: false,
      reason: 'errors',
      errorCount: 2,
      onErrorsMode: 'disableRun',
    });
  });

  it('stays open while there are errors, with "Show problems"', () => {
    const state = readyState();
    state.settings.value = settingsFixture({ run: { onErrors: 'showProblems' } });
    state.analysis.preview = previewFixture([diagnosticFixture()]);
    expect(runGate(state)).toEqual({
      enabled: true,
      reason: 'errors',
      errorCount: 1,
      onErrorsMode: 'showProblems',
    });
  });

  it('ignores warnings and information', () => {
    const state = readyState();
    state.analysis.preview = previewFixture([
      diagnosticFixture({ severity: 'warning' }),
      diagnosticFixture({ severity: 'info' }),
    ]);
    expect(runGate(state)).toMatchObject({ enabled: true, reason: null, errorCount: 0 });
  });

  it('names the first reason that applies', () => {
    const state = readyState();
    state.project = projectFixture({ trust: restricted });
    state.toolchains.list = [];
    state.analysis.preview = previewFixture([diagnosticFixture()]);
    expect(runGate(state).reason).toBe('restricted');
  });
});

describe('runGateMessage', () => {
  const gate = runGate(readyState());

  it('says what Run and Build do when they are available', () => {
    expect(runGateMessage(gate, 'run', { discovering: false })).toBe(
      'Build if needed, then run (F5)',
    );
    expect(runGateMessage(gate, 'build', { discovering: false })).toBe('Build (Ctrl+B)');
  });

  it('counts the errors', () => {
    const errors = { ...gate, enabled: false, reason: 'errors' as const, errorCount: 2 };
    expect(runGateMessage(errors, 'run', { discovering: false })).toBe(
      '2 errors – click to see the first',
    );
    expect(runGateMessage({ ...errors, errorCount: 1 }, 'build', { discovering: false })).toBe(
      '1 error – click to see the first',
    );
  });

  it('explains every other reason', () => {
    const closed = (reason: 'noProject' | 'restricted' | 'noToolchain') => ({
      ...gate,
      enabled: false,
      reason,
    });
    expect(runGateMessage(closed('noProject'), 'run', { discovering: false })).toBe(
      'Open or create a project first',
    );
    expect(runGateMessage(closed('restricted'), 'run', { discovering: false })).toBe(
      'Restricted Mode: trust this project to run it',
    );
    expect(runGateMessage(closed('restricted'), 'build', { discovering: false })).toBe(
      'Restricted Mode: trust this project to build it',
    );
    expect(runGateMessage(closed('noToolchain'), 'run', { discovering: false })).toBe(
      'No g++ found – click to set one up',
    );
    expect(runGateMessage(closed('noToolchain'), 'run', { discovering: true })).toBe(
      'Looking for a C++ compiler (g++)…',
    );
  });

  it('writes counts in words a person reads', () => {
    expect(countErrors(1)).toBe('1 error');
    expect(countErrors(12)).toBe('12 errors');
  });
});
