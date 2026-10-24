/**
 * The three features in the real window: the shell's toolbar, status bar and banners with the
 * app's own registries, as `src/features/index.ts` installs them.
 */
import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { installShellCommands, SHELL } from '../../app/actions';
import { App } from '../../app/App';
import { createDialogQueue, DialogHost } from '../../app/dialogs';
import { createAppEventBus } from '../../app/events';
import type { FeatureContext } from '../../app/features';
import { resetAppStore, useAppStore } from '../../app/store';
import {
  createFakeIpc,
  projectFixture,
  settingsFixture,
  toolchainFixture,
} from '../../app/testing/fixtures';
import { HintProvider } from '../../app/ui/Hint';
import { expectNoAxeViolations } from '../../test/axe';
import { settingsFeature } from '../settings';
import { settle } from '../settings/page/testing';
import { toolchainFeature } from '../toolchain';
import { trustFeature } from './feature';

vi.mock('../../editor/EditorWorkspace', () => ({
  EditorWorkspace: function EditorWorkspace() {
    return <div className="blockly-host" />;
  },
}));

/** A ResizeObserver that does nothing (happy-dom computes no layout). */
class NoResizeObserver {
  observe(): void {
    // Nothing to observe without layout.
  }
  unobserve(): void {
    // Nothing to observe without layout.
  }
  disconnect(): void {
    // Nothing to observe without layout.
  }
}

const cleanups: (() => void)[] = [];

function startWindow() {
  const ipc = createFakeIpc();
  ipc.toolchainSetupInfo.mockResolvedValue({
    platform: 'linux',
    noUsableToolchain: false,
    distro: { id: 'ubuntu', idLike: ['debian'] },
  });
  ipc.toolchainList.mockResolvedValue({ toolchains: [toolchainFixture()], discovering: false });
  const dialogs = createDialogQueue();
  const ctx: FeatureContext = {
    ipc,
    store: useAppStore,
    commands: SHELL.commands,
    screens: SHELL.screens,
    dialogs,
    events: createAppEventBus(),
    core: () => null,
    editor: () => null,
  };
  const { actions } = useAppStore.getState();
  actions.setSettings({ value: settingsFixture() });
  actions.setToolchains({ list: [toolchainFixture()] });
  cleanups.push(installShellCommands(SHELL));
  for (const feature of [toolchainFeature, settingsFeature, trustFeature]) {
    cleanups.push(feature(ctx));
  }
  const { container } = render(
    <HintProvider>
      <App />
      <DialogHost queue={dialogs} />
    </HintProvider>,
  );
  return { ipc, container };
}

beforeEach(() => {
  resetAppStore();
  vi.stubGlobal('ResizeObserver', NoResizeObserver);
});

afterEach(() => {
  for (const cleanup of cleanups.splice(0).reverse()) {
    cleanup();
  }
});

describe('the features in the window', () => {
  it('shows the Restricted Mode banner under the toolbar, with Run held back', async () => {
    const { ipc, container } = startWindow();
    await settle();
    act(() => {
      useAppStore.getState().actions.setProject(
        projectFixture({
          trust: {
            state: 'restricted',
            source: null,
            restrictedReason: 'noRecord',
            markOfTheWeb: false,
          },
        }),
      );
      useAppStore.getState().actions.setUi({ screen: 'editor' });
    });

    const banner = screen.getByRole('region', { name: 'Restricted Mode' });
    const toolbar = screen.getByRole('banner');
    expect(toolbar.compareDocumentPosition(banner) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const run = screen.getByTestId('toolbar-run');
    expect(run.getAttribute('aria-disabled')).toBe('true');
    expect(run.getAttribute('aria-describedby')).not.toBeNull();
    expect(screen.getByTestId('status-restricted')).toBeTruthy();
    // The container, as the shell's own test does: page-level rules need the whole document.
    await expectNoAxeViolations(container);

    ipc.trustGrant.mockResolvedValue({
      trust: { state: 'trusted', source: 'project', restrictedReason: null, markOfTheWeb: false },
    });
    fireEvent.click(within(banner).getByRole('button', { name: 'Trust…' }));
    await settle();

    expect(screen.queryByRole('region', { name: 'Restricted Mode' })).toBeNull();
    expect(screen.getByTestId('toolbar-run').getAttribute('aria-disabled')).toBe('false');
  });

  it('opens the toolchain page from the status bar and the Settings page from the toolbar', async () => {
    startWindow();
    await settle();

    fireEvent.click(screen.getByTestId('status-toolchain'));
    expect(useAppStore.getState().ui.screen).toBe('toolchainSetup');
    expect(screen.getByRole('main', { name: 'Set up a C++ compiler' })).toBeTruthy();
    expect(screen.getByTestId('toolchain-page')).toBeTruthy();
    await expectNoAxeViolations(screen.getByTestId('toolchain-page'));

    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    await settle();
    expect(useAppStore.getState().ui.screen).toBe('settings');
    expect(screen.getByTestId('settings-page')).toBeTruthy();
    await expectNoAxeViolations(screen.getByTestId('settings-page'));
  });
});
