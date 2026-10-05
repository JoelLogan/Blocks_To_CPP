import type {
  BuildCacheClearResponse,
  Settings,
  SettingsUpdateResponse,
} from '@blocks2cpp/ipc-types';
import { IpcCallError } from '@blocks2cpp/ipc-types';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { runGate } from '../../app/runGate';
import { resetAppStore, useAppStore } from '../../app/store';
import {
  diagnosticFixture,
  previewFixture,
  projectFixture,
  settingsFixture,
  toolchainFixture,
} from '../../app/testing/fixtures';
import { expectNoAxeViolations } from '../../test/axe';
import { CLEAR_CACHE_CONFIRMATION, settingsFailureText } from './controller';
import { createSettingsFeature } from './feature';
import {
  createSettingsSectionRegistry,
  registerSettingsSection,
  settingsSections,
} from './sections';
import {
  deferred,
  featureContext,
  renderBanners,
  renderScreen,
  settle,
  type TestFeatureContext,
} from './page/testing';
import {
  cacheClearedText,
  formatBytes,
  formatLines,
  noticeText,
  parseScrollback,
  settingLabel,
  shownNotices,
} from './texts';

let uninstall: (() => void) | null = null;

/** Installs the feature after the bootstrap read `settings` (or failed to, with `null`). */
function install(settings: Settings | null = settingsFixture()): TestFeatureContext {
  const created = featureContext();
  if (settings !== null) {
    useAppStore.getState().actions.setSettings({ value: settings, notices: [] });
    created.ipc.settingsGet.mockResolvedValue({ settings, notices: [] });
  }
  uninstall = createSettingsFeature({ banners: created.banners, sections: created.sections })(
    created.ctx,
  );
  return created;
}

async function showPage(
  settings: Settings | null = settingsFixture(),
): Promise<TestFeatureContext> {
  const created = install(settings);
  renderScreen(created, 'settings');
  await settle();
  return created;
}

/** The answer of `settings_update` for `settings`. */
function updated(settings: Settings): SettingsUpdateResponse {
  return { settings };
}

function radio(name: string): HTMLInputElement {
  return screen.getByRole<HTMLInputElement>('radio', { name });
}

beforeEach(() => {
  resetAppStore();
});

afterEach(() => {
  uninstall?.();
  uninstall = null;
});

describe('the Settings page', () => {
  it('reads the settings when it opens and shows them', async () => {
    const page = await showPage(settingsFixture({ console: { scrollbackLines: 25_000 } }));

    expect(page.ipc.settingsGet).toHaveBeenCalledTimes(1);
    expect(screen.getByRole('heading', { level: 2, name: 'Settings' })).toBeTruthy();
    expect(radio('4 spaces').checked).toBe(true);
    expect(radio('2 spaces').checked).toBe(false);
    expect(radio('Disable Run').checked).toBe(true);
    expect(screen.getByLabelText<HTMLInputElement>('Scrollback lines').value).toBe('25,000');
    expect(screen.getByText(/none of them is saved\s+in a project file/)).toBeTruthy();
    await expectNoAxeViolations(screen.getByTestId('settings-page'));
  });

  it('shows the settings the backend reads when the page opens', async () => {
    const created = install(settingsFixture());
    created.ipc.settingsGet.mockResolvedValue({
      settings: settingsFixture({ codeStyle: { indentWidth: 2 } }),
      notices: [{ key: 'codeStyle.indentWidth', reason: 'invalidValue' }],
    });
    renderScreen(created, 'settings');
    await settle();

    expect(radio('2 spaces').checked).toBe(true);
    expect(useAppStore.getState().settings.notices).toHaveLength(1);
  });

  it('reads them on install when the bootstrap could not, and offers to try again', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const created = featureContext();
    created.ipc.settingsGet.mockRejectedValue(
      new IpcCallError('settings_get', { code: 'internal' }),
    );
    uninstall = createSettingsFeature({ banners: created.banners, sections: created.sections })(
      created.ctx,
    );
    await settle();
    expect(created.ipc.settingsGet).toHaveBeenCalledTimes(1);
    renderScreen(created, 'settings');
    await settle();

    expect(screen.getByTestId('settings-error').textContent).toContain(
      'The settings could not be read (internal).',
    );
    expect(screen.queryByRole('radio')).toBeNull();
    await expectNoAxeViolations(screen.getByTestId('settings-page'));

    created.ipc.settingsGet.mockResolvedValue({ settings: settingsFixture(), notices: [] });
    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    await settle();

    expect(screen.queryByTestId('settings-error')).toBeNull();
    expect(radio('4 spaces').checked).toBe(true);
  });

  it('changes the indent width with a partial update and applies it at once', async () => {
    const page = await showPage();
    const answer = deferred<SettingsUpdateResponse>();
    page.ipc.settingsUpdate.mockReturnValue(answer.promise);

    fireEvent.click(radio('2 spaces'));
    await settle();

    expect(page.ipc.settingsUpdate).toHaveBeenCalledWith({ codeStyle: { indentWidth: 2 } });
    // The choice shows while it is saved; the store changes with the backend's answer.
    expect(radio('2 spaces').checked).toBe(true);
    expect(screen.getByTestId('settings-saving').textContent).toBe('Saving…');
    expect(useAppStore.getState().settings.value?.codeStyle.indentWidth).toBe(4);

    await act(async () => {
      answer.resolve(updated(settingsFixture({ codeStyle: { indentWidth: 2 } })));
      await answer.promise;
    });
    await settle();

    expect(useAppStore.getState().settings.value?.codeStyle.indentWidth).toBe(2);
    expect(radio('2 spaces').checked).toBe(true);
    expect(screen.getByTestId('settings-saving').textContent).toBe('');
  });

  it('changes Run on errors, which the run gate follows', async () => {
    const page = await showPage();
    act(() => {
      const { actions } = useAppStore.getState();
      actions.setProject(projectFixture());
      actions.setToolchains({ list: [toolchainFixture()] });
      actions.setAnalysis({ preview: previewFixture([diagnosticFixture()]) });
    });
    expect(runGate(useAppStore.getState()).enabled).toBe(false);
    page.ipc.settingsUpdate.mockResolvedValue(
      updated(settingsFixture({ run: { onErrors: 'showProblems' } })),
    );

    fireEvent.click(radio('Show problems'));
    await settle();

    expect(page.ipc.settingsUpdate).toHaveBeenCalledWith({ run: { onErrors: 'showProblems' } });
    expect(runGate(useAppStore.getState())).toMatchObject({
      enabled: true,
      onErrorsMode: 'showProblems',
    });
  });

  it('puts the saved choice back when an update is refused', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const page = await showPage();
    page.ipc.settingsUpdate.mockRejectedValue(
      new IpcCallError('settings_update', { code: 'io', kind: 'permissionDenied' }),
    );

    fireEvent.click(radio('Show problems'));
    await settle();

    expect(radio('Disable Run').checked).toBe(true);
    expect(useAppStore.getState().settings.value?.run.onErrors).toBe('disableRun');
    const error = screen.getByTestId('settings-error');
    expect(error.getAttribute('role')).toBe('alert');
    expect(error.textContent).toContain('The settings file could not be written');
  });

  it('saves the scrollback on Enter or when the field is left, and only valid values', async () => {
    const page = await showPage();
    page.ipc.settingsUpdate.mockImplementation((patch) =>
      Promise.resolve(
        updated(
          settingsFixture({
            console: { scrollbackLines: patch.console?.scrollbackLines ?? 10_000 },
          }),
        ),
      ),
    );
    const field = screen.getByLabelText<HTMLInputElement>('Scrollback lines');

    fireEvent.change(field, { target: { value: '500' } });
    fireEvent.blur(field);
    await settle();
    expect(page.ipc.settingsUpdate).not.toHaveBeenCalled();
    expect(field.getAttribute('aria-invalid')).toBe('true');
    expect(screen.getByRole('alert').textContent).toBe(
      'Enter a whole number from 1,000 to 100,000.',
    );
    await expectNoAxeViolations(screen.getByTestId('settings-page'));

    fireEvent.keyDown(field, { key: 'Escape' });
    expect(field.value).toBe('10,000');
    expect(field.getAttribute('aria-invalid')).toBe('false');

    fireEvent.change(field, { target: { value: '50 000' } });
    fireEvent.keyDown(field, { key: 'Enter' });
    await settle();
    expect(page.ipc.settingsUpdate).toHaveBeenCalledWith({ console: { scrollbackLines: 50_000 } });
    expect(useAppStore.getState().settings.value?.console.scrollbackLines).toBe(50_000);
    const refreshed = screen.getByLabelText<HTMLInputElement>('Scrollback lines');
    expect(refreshed.value).toBe('50,000');

    // An unchanged value is not sent again.
    fireEvent.blur(refreshed);
    await settle();
    expect(page.ipc.settingsUpdate).toHaveBeenCalledTimes(1);
  });

  it('sends updates one after the other, in order', async () => {
    const page = await showPage();
    const first = deferred<SettingsUpdateResponse>();
    page.ipc.settingsUpdate
      .mockReturnValueOnce(first.promise)
      .mockResolvedValueOnce(
        updated(
          settingsFixture({ codeStyle: { indentWidth: 2 }, run: { onErrors: 'showProblems' } }),
        ),
      );

    fireEvent.click(radio('2 spaces'));
    fireEvent.click(radio('Show problems'));
    await settle();
    expect(page.ipc.settingsUpdate).toHaveBeenCalledTimes(1);

    await act(async () => {
      first.resolve(updated(settingsFixture({ codeStyle: { indentWidth: 2 } })));
      await first.promise;
    });
    await settle();

    expect(page.ipc.settingsUpdate).toHaveBeenCalledTimes(2);
    expect(useAppStore.getState().settings.value).toMatchObject({
      codeStyle: { indentWidth: 2 },
      run: { onErrors: 'showProblems' },
    });
  });

  it('lists the notices about settings.json', async () => {
    await showPage();
    act(() => {
      useAppStore.getState().actions.setSettings({
        notices: [
          { key: 'codeStyle.indentWidth', reason: 'invalidValue' },
          { key: 'console.scrollbackLines', reason: 'invalidValue' },
          { key: '', reason: 'newerVersion' },
        ],
      });
    });

    const notices = screen.getByTestId('settings-notices');
    expect(notices.getAttribute('role')).toBe('status');
    const items = within(notices)
      .getAllByRole('listitem')
      .map((item) => item.textContent);
    expect(items).toEqual([
      'Code style: indent width had a value that is not allowed and was reset to its default.',
      'Console: scrollback lines had a value that is not allowed and was reset to its default.',
      'The settings file comes from a newer version of Blocks2Cpp. The settings this version knows are used, and the others are kept as they are.',
    ]);
    await expectNoAxeViolations(screen.getByTestId('settings-page'));
  });

  it('clears the build cache only after an in-app confirmation', async () => {
    const page = await showPage();
    const result: BuildCacheClearResponse = { freedBytes: 12 * 1024 * 1024, skippedInUse: 1 };
    page.ipc.buildCacheClear.mockResolvedValue(result);

    fireEvent.click(screen.getByRole('button', { name: 'Clear build cache…' }));
    await settle();
    const dialog = screen.getByRole('dialog', { name: CLEAR_CACHE_CONFIRMATION.title });
    await expectNoAxeViolations(dialog);
    expect(screen.getByTestId('settings-clear-cache').textContent).toBe('Clearing…');
    fireEvent.click(within(dialog).getByRole('button', { name: 'Cancel' }));
    await settle();
    expect(page.ipc.buildCacheClear).not.toHaveBeenCalled();
    expect(screen.queryByTestId('settings-cache-result')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: 'Clear build cache…' }));
    await settle();
    fireEvent.click(screen.getByRole('button', { name: 'Clear build cache' }));
    await settle();

    expect(page.ipc.buildCacheClear).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId('settings-cache-result').textContent).toContain(
      'The build cache was cleared: 12 MB freed. 1 build was kept because it is in use right now.',
    );
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
    await expectNoAxeViolations(screen.getByTestId('settings-page'));
  });

  it('says when the build cache could not be cleared', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const page = await showPage();
    page.ipc.buildCacheClear.mockRejectedValue(
      new IpcCallError('build_cache_clear', { code: 'io', kind: 'other' }),
    );

    fireEvent.click(screen.getByTestId('settings-clear-cache'));
    await settle();
    fireEvent.click(screen.getByRole('button', { name: 'Clear build cache' }));
    await settle();

    expect(screen.getByTestId('settings-error').textContent).toContain(
      'The build cache could not be cleared (io).',
    );
    expect(screen.getByTestId('settings-clear-cache').textContent).toBe('Clear build cache…');
  });

  it('links to the toolchain page once it is registered', async () => {
    const page = await showPage();
    expect(screen.queryByRole('region', { name: 'Toolchain' })).toBeNull();

    act(() => {
      page.ctx.screens.registerScreen('toolchainSetup', () => null);
    });
    fireEvent.click(screen.getByRole('button', { name: 'Open the toolchain page' }));
    expect(useAppStore.getState().ui.screen).toBe('toolchainSetup');
  });

  it('shows the sections other features add, in order', async () => {
    const page = await showPage();
    act(() => {
      page.sections.registerSection('b', () => <p>Second section</p>, { order: 2 });
      page.sections.registerSection('a', () => <p>First section</p>, { order: 1 });
    });
    const text = screen.getByTestId('settings-page').textContent;
    expect(text.indexOf('First section')).toBeLessThan(text.indexOf('Second section'));
    expect(text.indexOf('Build cache')).toBeLessThan(text.indexOf('First section'));
  });

  it('goes back to where the person came from', async () => {
    await showPage();
    fireEvent.click(screen.getByRole('button', { name: 'Back to the start page' }));
    expect(useAppStore.getState().ui.screen).toBe('start');
  });

  it('removes its page and banner when uninstalled', () => {
    const created = install();
    uninstall?.();
    uninstall = null;
    expect(created.ctx.screens.screen('settings')).toBeNull();
    expect(created.banners.banners()).toEqual([]);
  });
});

describe('the settings notices banner', () => {
  it('appears for notices, opens the page and can be dismissed', async () => {
    const created = install();
    const open = vi.fn();
    created.ctx.commands.registerCommand('settings.open', open);
    renderBanners(created);
    expect(screen.queryByTestId('settings-notices-banner')).toBeNull();

    act(() => {
      useAppStore.getState().actions.setSettings({ notices: [{ key: '', reason: 'corruptFile' }] });
    });
    const banner = screen.getByTestId('settings-notices-banner');
    await expectNoAxeViolations(banner);
    fireEvent.click(within(banner).getByRole('button', { name: 'Show settings' }));
    await settle();
    expect(open).toHaveBeenCalledTimes(1);

    act(() => {
      useAppStore.getState().actions.setUi({ screen: 'settings' });
    });
    expect(screen.queryByTestId('settings-notices-banner')).toBeNull();
    act(() => {
      useAppStore.getState().actions.setUi({ screen: 'editor' });
    });
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(screen.queryByTestId('settings-notices-banner')).toBeNull();
  });
});

describe('the section registry', () => {
  it('keeps one section per ID, refuses a bad order and removes a section once', () => {
    const registry = createSettingsSectionRegistry();
    const First = () => <p>first</p>;
    const Second = () => <p>second</p>;
    const removeFirst = registry.registerSection('trust', First);
    const removeSecond = registry.registerSection('trust', Second);
    expect(registry.sections().map((section) => section.component)).toEqual([Second]);
    removeSecond();
    removeSecond();
    expect(registry.sections().map((section) => section.component)).toEqual([First]);
    removeFirst();
    expect(registry.sections()).toEqual([]);
    expect(() => registry.registerSection('bad', First, { order: Infinity })).toThrow(RangeError);
  });

  it('registers with the app registry', () => {
    const remove = registerSettingsSection('test-only', () => null, { order: 5 });
    expect(settingsSections.sections().map((section) => section.id)).toContain('test-only');
    remove();
    expect(settingsSections.sections().map((section) => section.id)).not.toContain('test-only');
  });
});

describe('texts', () => {
  it('formats sizes', () => {
    expect(formatBytes(0)).toBe('0 bytes');
    expect(formatBytes(1)).toBe('1 byte');
    expect(formatBytes(1023)).toBe('1023 bytes');
    expect(formatBytes(1024)).toBe('1 KB');
    expect(formatBytes(1536)).toBe('1.5 KB');
    expect(formatBytes(1024 * 1024 - 1)).toBe('1 MB');
    expect(formatBytes(12 * 1024 * 1024)).toBe('12 MB');
    expect(formatBytes(3.25 * 1024 ** 3)).toBe('3.3 GB');
    expect(formatBytes(5 * 1024 ** 5)).toBe('5120 TB');
    expect(formatBytes(-1)).toBe('an unknown amount');
    expect(formatBytes(Number.NaN)).toBe('an unknown amount');
  });

  it('describes what clearing did', () => {
    expect(cacheClearedText({ freedBytes: 0, skippedInUse: 0 })).toBe(
      'The build cache was already empty.',
    );
    expect(cacheClearedText({ freedBytes: 2048, skippedInUse: 0 })).toBe(
      'The build cache was cleared: 2 KB freed.',
    );
    expect(cacheClearedText({ freedBytes: 0, skippedInUse: 3 })).toBe(
      'The build cache was already empty. 3 builds were kept because they are in use right now.',
    );
  });

  it('reads the scrollback field strictly', () => {
    expect(parseScrollback('10000')).toBe(10_000);
    expect(parseScrollback(' 10,000 ')).toBe(10_000);
    expect(parseScrollback("100'000")).toBe(100_000);
    expect(parseScrollback('1 000')).toBe(1000);
    expect(parseScrollback('999')).toBeNull();
    expect(parseScrollback('100001')).toBeNull();
    expect(parseScrollback('10.5')).toBeNull();
    expect(parseScrollback('1e4')).toBeNull();
    expect(parseScrollback('-1000')).toBeNull();
    expect(parseScrollback('')).toBeNull();
    expect(parseScrollback('9'.repeat(400))).toBeNull();
    expect(formatLines(100_000)).toBe('100,000');
  });

  it('names settings keys and explains notices', () => {
    expect(settingLabel('toolchain.selectedId')).toBe('Toolchain: the default compiler');
    expect(settingLabel('run')).toBe('Run on errors');
    expect(settingLabel('lints.‮evil')).toBe('The setting “lints.⟨U+202E⟩evil”');
    expect(settingLabel('x'.repeat(100))).toHaveLength('The setting “”'.length + 64);
    expect(settingLabel('__proto__')).toBe('The setting “__proto__”');
    expect(noticeText({ key: '', reason: 'corruptFile' })).toContain('could not be read');
    expect(noticeText({ key: '', reason: 'invalidValue' })).toBe(
      'A setting had a value that is not allowed and was reset to its default.',
    );
    const many = Array.from({ length: 25 }, () => ({
      key: 'run',
      reason: 'invalidValue' as const,
    }));
    expect(shownNotices(many)).toMatchObject({ more: 5 });
    expect(shownNotices(many).shown).toHaveLength(20);
  });

  it('explains failures without any text from the backend', () => {
    expect(settingsFailureText('update', 'invalidRequest')).toBe(
      'That value is not allowed, so the setting was not changed.',
    );
    expect(settingsFailureText('update', 'internal')).toBe(
      'The setting could not be changed (internal).',
    );
    expect(settingsFailureText('read', 'transport')).toBe(
      'The settings could not be read (transport).',
    );
  });
});
