/**
 * The keyboard side of the accessibility baseline on the full-window pages
 * (docs/spec/04-user-interface.md §4.8): where the focus goes when a page appears (WCAG 2.4.3),
 * and that Tab reaches every control of the start, settings and toolchain pages in reading order,
 * each with a name (2.1.1, 4.1.2), and nothing out of order (no positive `tabindex`). The pages'
 * own tests run axe on each of their states.
 */
import { act, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { DialogHost } from '../app/dialogs';
import { describeStop, tabThrough as walkThrough } from '../app/layout/sequentialFocus';
import { resetAppStore, useAppStore } from '../app/store';
import { settingsFixture, toolchainFixture } from '../app/testing/fixtures';
import { type Harness, installHarness, recentEntry } from './project/testing';
import { createSettingsFeature } from './settings/feature';
import {
  featureContext,
  renderScreen,
  settle,
  type TestFeatureContext,
} from './settings/page/testing';
import { toolchainFeature } from './toolchain/feature';

/** Tabs through `container`, letting React handle each focus change. */
function tabThrough(container: Element): string[] {
  let reached: string[] = [];
  act(() => {
    reached = walkThrough(container);
  });
  return reached;
}

/** Every stop has a name after its role (`button: `, with nothing after it, has none). */
function expectAllNamed(stops: readonly string[]): void {
  for (const stop of stops) {
    expect(stop, `a control without a name: ${stop}`).toMatch(/^[\w-]+: \S/);
  }
}

/** No element in `container` puts itself ahead of the reading order. */
function expectNoPositiveTabIndex(container: Element): void {
  const ahead = [...container.querySelectorAll('[tabindex]')].filter(
    (element) => Number(element.getAttribute('tabindex')) > 0,
  );
  expect(ahead).toEqual([]);
}

beforeEach(() => {
  resetAppStore();
  vi.spyOn(console, 'warn').mockImplementation(() => undefined);
});

describe('the start page', () => {
  let harness: Harness;

  beforeEach(() => {
    harness = installHarness(null);
  });

  afterEach(() => {
    harness.dispose();
  });

  function renderStartPage(before?: HTMLElement) {
    const StartScreen = harness.ctx.screens.screen('start');
    if (StartScreen === null) {
      throw new Error('the start page is not registered');
    }
    before?.focus();
    return render(
      <main aria-label="Start page">
        <StartScreen />
        <DialogHost queue={harness.dialogs} />
      </main>,
    );
  }

  async function listRead(): Promise<void> {
    await waitFor(() => {
      expect(harness.feature.model.getState().recent.status).not.toBe('loading');
    });
  }

  it('takes the focus at its heading when nothing has it (the window opened, a project closed)', async () => {
    renderStartPage();
    await listRead();
    expect(document.activeElement).toBe(screen.getByRole('heading', { level: 2, name: 'Start' }));
  });

  it('leaves the focus where it is when something still has it', async () => {
    const toolbarButton = document.createElement('button');
    toolbarButton.textContent = 'Settings';
    document.body.append(toolbarButton);
    try {
      renderStartPage(toolbarButton);
      await listRead();
      expect(document.activeElement).toBe(toolbarButton);
    } finally {
      toolbarButton.remove();
    }
  });

  it('reaches the templates, Open… and each recent project with Tab, in reading order', async () => {
    harness.ipc.recentList.mockResolvedValue({ entries: [recentEntry(1), recentEntry(2)] });
    const { container } = renderStartPage();
    await listRead();
    const stops = tabThrough(container);
    // The heading takes the focus from code only: Tab never stops on it.
    expect(stops).toEqual([
      'button: Empty project',
      'button: Hello World',
      'button: Open…',
      'button: Project 1',
      'button: Remove “Project 1” from the list',
      'button: Project 2',
      'button: Remove “Project 2” from the list',
    ]);
    expectNoPositiveTabIndex(container);
  });
});

describe('the Settings page', () => {
  let uninstall: (() => void) | null = null;

  afterEach(() => {
    uninstall?.();
    uninstall = null;
  });

  async function showPage(): Promise<TestFeatureContext> {
    const created = featureContext();
    const settings = settingsFixture();
    useAppStore.getState().actions.setSettings({ value: settings, notices: [] });
    created.ipc.settingsGet.mockResolvedValue({ settings, notices: [] });
    uninstall = createSettingsFeature({ banners: created.banners, sections: created.sections })(
      created.ctx,
    );
    renderScreen(created, 'settings');
    await settle();
    return created;
  }

  it('reaches every setting with Tab, one stop per group of choices, each named', async () => {
    await showPage();
    const page = screen.getByTestId('settings-page');
    const stops = tabThrough(page);
    expect(stops[0]).toBe('button: Back to the start page');
    expect(stops.at(-1)).toBe('button: Clear build cache…');
    expectAllNamed(stops);
    // A group of radio buttons is one stop, its chosen button (the arrow keys choose within it).
    const chosen = [...page.querySelectorAll('input[type="radio"]:checked')].map(describeStop);
    expect(chosen.length).toBeGreaterThan(1);
    expect(stops.filter((stop) => stop.startsWith('radio: '))).toEqual(chosen);
    expect(stops).toContain('textbox: Scrollback lines');
    expectNoPositiveTabIndex(page);
  });
});

describe('the toolchain page', () => {
  let uninstall: (() => void) | null = null;

  afterEach(() => {
    uninstall?.();
    uninstall = null;
  });

  it('reaches Back, Rescan, Choose g++ manually… and each compiler with Tab, each named', async () => {
    const created = featureContext();
    const usable = toolchainFixture({
      id: 'tc_1111111111111111',
      version: '13.3.0',
      target: 'x86_64-linux-gnu',
      flavor: null,
      displayPath: '/usr/bin/g++',
      selected: true,
    });
    const other = toolchainFixture({
      id: 'tc_2222222222222222',
      version: '14.1.0',
      target: 'x86_64-linux-gnu',
      flavor: null,
      displayPath: '/usr/local/bin/g++',
      selected: false,
    });
    created.ipc.toolchainSetupInfo.mockResolvedValue({
      platform: 'linux',
      noUsableToolchain: false,
      distro: null,
    });
    created.ipc.toolchainList.mockResolvedValue({
      toolchains: [usable, other],
      discovering: false,
    });
    useAppStore.getState().actions.setToolchains({ list: [usable, other], discovering: false });
    uninstall = toolchainFeature(created.ctx);
    renderScreen(created, 'toolchainSetup');
    await settle();

    const page = screen.getByTestId('toolchain-page');
    const stops = tabThrough(page);
    expect(stops[0]).toBe('button: Back to the start page');
    expectAllNamed(stops);
    expect(stops).toContain('button: Rescan');
    expect(stops).toContain('button: Choose g++ manually…');
    expectNoPositiveTabIndex(page);
  });
});
