import { act, render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { expectNoAxeViolations } from '../test/axe';
import { type AppRuntime, type BootPhase, createAppRuntime } from './bootstrap';
import { createCommandRegistry } from './commands';
import { createDialogQueue } from './dialogs';
import { Root } from './Root';
import { createScreenRegistry } from './screens';
import { resetAppStore, useAppStore } from './store';
import { appInfoFixture, createFakeIpc } from './testing/fixtures';

vi.mock('./App', () => ({ App: () => <p>the main window</p> }));

let runtime: AppRuntime | null = null;

function makeRuntime() {
  const ipc = createFakeIpc();
  runtime = createAppRuntime({
    ipc,
    store: useAppStore,
    commands: createCommandRegistry(),
    screens: createScreenRegistry(),
    dialogs: createDialogQueue(),
    installFeatures: () => () => undefined,
    installBlocklyDialogs: () => () => undefined,
    window,
  });
  return { runtime, ipc };
}

/** A runtime that is already in `phase`, for the screens that cannot be reached otherwise. */
function fixedRuntime(phase: BootPhase): AppRuntime {
  return {
    ...makeRuntime().runtime,
    getPhase: () => phase,
  };
}

afterEach(() => {
  runtime?.stop();
  runtime = null;
  resetAppStore();
});

describe('Root', () => {
  it('says it is starting, then shows the main window', async () => {
    const { runtime } = makeRuntime();
    render(<Root runtime={runtime} />);
    expect(screen.getByRole('status').textContent).toBe('Starting Blocks2Cpp…');

    await act(async () => {
      await runtime.start();
    });
    expect(screen.getByText('the main window')).toBeTruthy();
  });

  it('shows a blocking error screen when the IPC versions differ', async () => {
    const { runtime, ipc } = makeRuntime();
    ipc.appInfo.mockResolvedValue(appInfoFixture({ ipcVersion: 99 }));
    const { container } = render(<Root runtime={runtime} />);

    await act(async () => {
      await runtime.start();
    });

    expect(screen.getByRole('heading', { level: 1 }).textContent).toBe('✖ Blocks2Cpp cannot start');
    expect(screen.getByRole('alert')).toBeTruthy();
    expect(screen.getByTestId('startup-details').textContent).toBe(
      'Technical details: the window speaks IPC version 1, the program behind it speaks IPC version 99.',
    );
    expect(screen.queryByText('the main window')).toBeNull();
    await expectNoAxeViolations(container);
  });

  it('says when the backend did not name its version', () => {
    render(
      <Root runtime={fixedRuntime({ kind: 'versionMismatch', frontend: 1, backend: null })} />,
    );
    expect(screen.getByTestId('startup-details').textContent).toBe(
      'Technical details: the window speaks IPC version 1, the program behind it did not say which version it speaks.',
    );
  });

  it('shows the error code when the backend cannot be reached', async () => {
    const { container } = render(
      <Root runtime={fixedRuntime({ kind: 'failed', code: 'transport' })} />,
    );
    expect(screen.getByRole('alert').textContent).toContain(
      'The window could not reach the program behind it.',
    );
    expect(screen.getByTestId('startup-details').textContent).toBe('Technical details: transport.');
    await expectNoAxeViolations(container);
  });

  it("shows the runtime's dialogs", async () => {
    const { runtime } = makeRuntime();
    render(<Root runtime={runtime} />);

    await act(async () => {
      void runtime.dialogs.alert({ message: 'Hello' });
      await Promise.resolve();
    });
    expect(screen.getByRole('dialog', { name: 'Hello' })).toBeTruthy();
  });
});
