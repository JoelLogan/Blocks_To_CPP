import type { Trust, TrustResponse } from '@blocks2cpp/ipc-types';
import { IpcCallError } from '@blocks2cpp/ipc-types';
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { resetAppStore, useAppStore } from '../../app/store';
import { projectFixture, settingsFixture } from '../../app/testing/fixtures';
import { expectNoAxeViolations } from '../../test/axe';
import { createSettingsFeature } from '../settings/feature';
import {
  deferred,
  featureContext,
  renderBanners,
  renderScreen,
  settle,
  type TestFeatureContext,
} from '../settings/page/testing';
import { TrustController } from './controller';
import { createTrustFeature } from './feature';
import { RUN_BUTTON_SELECTOR } from './RestrictedModeBanner';
import {
  MARK_OF_THE_WEB_TEXT,
  RESTRICTED_MODE_TEXT,
  restrictedReasonText,
  trustedText,
  trustMessageText,
} from './texts';

const RESTRICTED: Trust = {
  state: 'restricted',
  source: null,
  restrictedReason: 'noRecord',
  markOfTheWeb: false,
};
const CHANGED_OUTSIDE: Trust = { ...RESTRICTED, restrictedReason: 'changedOutside' };
const TRUSTED_PROJECT: Trust = {
  state: 'trusted',
  source: 'project',
  restrictedReason: null,
  markOfTheWeb: false,
};
const TRUSTED_FOLDER: Trust = { ...TRUSTED_PROJECT, source: 'folder' };
const CREATED_HERE: Trust = { ...TRUSTED_PROJECT, source: 'createdHere' };

const OTHER_HANDLE = 'ph_ffffffffffffffffffffffffffffffff';

const uninstallers: (() => void)[] = [];

/** Installs the trust feature (and the settings feature, for its section). */
function install(options: { settings?: boolean } = {}): TestFeatureContext {
  const created = featureContext();
  const registries = { banners: created.banners, sections: created.sections };
  if (options.settings === true) {
    useAppStore.getState().actions.setSettings({ value: settingsFixture() });
    created.ipc.settingsGet.mockResolvedValue({ settings: settingsFixture(), notices: [] });
    uninstallers.push(createSettingsFeature(registries)(created.ctx));
  }
  uninstallers.push(createTrustFeature(registries)(created.ctx));
  return created;
}

function openProject(trust: Trust, handle = projectFixture().handle): void {
  act(() => {
    useAppStore.getState().actions.setProject(projectFixture({ trust, handle }));
  });
}

function banner(): HTMLElement {
  return screen.getByTestId('restricted-banner');
}

beforeEach(() => {
  resetAppStore();
});

afterEach(() => {
  for (const uninstall of uninstallers.splice(0).reverse()) {
    uninstall();
  }
});

describe('the Restricted Mode banner', () => {
  it('appears only while the open project is restricted', async () => {
    const created = install();
    renderBanners(created);
    expect(screen.queryByTestId('restricted-banner')).toBeNull();

    openProject(TRUSTED_PROJECT);
    expect(screen.queryByTestId('restricted-banner')).toBeNull();

    openProject(RESTRICTED);
    const shown = banner();
    expect(within(shown).getByRole('heading', { level: 2, name: 'Restricted Mode' })).toBeTruthy();
    expect(screen.getByRole('region', { name: 'Restricted Mode' })).toBe(shown);
    expect(shown.textContent).toContain(restrictedReasonText('noRecord'));
    expect(shown.textContent).toContain(RESTRICTED_MODE_TEXT);
    expect(shown.querySelector('svg')).not.toBeNull();
    expect(screen.queryByTestId('restricted-banner-motw')).toBeNull();
    expect(within(shown).getByRole('button', { name: 'Trust…' })).toBeTruthy();
    // The banners' container announces the banner when it appears.
    expect(screen.getByTestId('window-banners').getAttribute('aria-live')).toBe('polite');
    await expectNoAxeViolations(shown);

    act(() => {
      useAppStore.getState().actions.setProject(null);
    });
    expect(screen.queryByTestId('restricted-banner')).toBeNull();
  });

  it('says when security-relevant content changed outside the app, and warns about the Internet', async () => {
    const created = install();
    renderBanners(created);
    openProject({ ...CHANGED_OUTSIDE, markOfTheWeb: true });

    expect(screen.getByTestId('restricted-banner-reason').textContent).toBe(
      restrictedReasonText('changedOutside'),
    );
    expect(screen.getByTestId('restricted-banner-motw').textContent).toContain(
      MARK_OF_THE_WEB_TEXT,
    );
    await expectNoAxeViolations(banner());
  });

  it('trusts the project through the backend and then moves the focus to Run', async () => {
    const created = install();
    renderBanners(created);
    const run = document.createElement('button');
    run.dataset['testid'] = 'toolbar-run';
    run.textContent = 'Run';
    document.body.append(run);
    openProject(RESTRICTED);
    const answer = deferred<TrustResponse>();
    created.ipc.trustGrant.mockReturnValue(answer.promise);

    const trust = within(banner()).getByRole('button', { name: 'Trust…' });
    fireEvent.click(trust);
    await settle();
    expect(created.ipc.trustGrant).toHaveBeenCalledWith({ handle: projectFixture().handle });
    expect(trust.textContent).toBe('Waiting for your answer…');
    expect(trust.getAttribute('aria-disabled')).toBe('true');
    fireEvent.click(trust);
    expect(created.ipc.trustGrant).toHaveBeenCalledTimes(1);

    await act(async () => {
      answer.resolve({ trust: TRUSTED_PROJECT });
      await answer.promise;
    });
    await settle();

    expect(useAppStore.getState().project?.trust).toEqual(TRUSTED_PROJECT);
    expect(screen.queryByTestId('restricted-banner')).toBeNull();
    await waitFor(() => {
      expect(document.activeElement).toBe(document.querySelector(RUN_BUTTON_SELECTOR));
    });
    run.remove();
  });

  it('stays when the dialog is cancelled, and says so', async () => {
    const created = install();
    renderBanners(created);
    openProject(RESTRICTED);
    created.ipc.trustGrant.mockResolvedValue({ trust: RESTRICTED });

    fireEvent.click(screen.getByTestId('restricted-banner-trust'));
    await settle();

    expect(useAppStore.getState().project?.trust).toEqual(RESTRICTED);
    expect(screen.getByTestId('restricted-banner-message').textContent).toBe(
      'The project stays in Restricted Mode.',
    );
    expect(screen.getByTestId('restricted-banner-trust').textContent).toBe('Trust…');
    await expectNoAxeViolations(banner());
  });

  it.each([
    ['rateLimited', 'Please wait a moment, then try again.'],
    ['busy', 'Another dialog is already open. Close it, then try again.'],
    ['io', 'The trust choice could not be recorded (io), so the project stays in Restricted Mode.'],
  ] as const)('explains a %s failure', async (code, text) => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const created = install();
    renderBanners(created);
    openProject(RESTRICTED);
    const error =
      code === 'io'
        ? new IpcCallError('trust_grant', { code, kind: 'permissionDenied' })
        : new IpcCallError('trust_grant', { code });
    created.ipc.trustGrant.mockRejectedValue(error);

    fireEvent.click(screen.getByTestId('restricted-banner-trust'));
    await settle();

    const message = screen.getByTestId('restricted-banner-message');
    expect(message.textContent).toBe(`✖ ${text}`);
    expect(useAppStore.getState().project?.trust).toEqual(RESTRICTED);
  });

  it('never applies an answer to another project opened meanwhile', async () => {
    const created = install();
    renderBanners(created);
    openProject(RESTRICTED);
    const answer = deferred<TrustResponse>();
    created.ipc.trustGrant.mockReturnValue(answer.promise);

    fireEvent.click(screen.getByTestId('restricted-banner-trust'));
    await settle();
    openProject(CHANGED_OUTSIDE, OTHER_HANDLE);
    await act(async () => {
      answer.resolve({ trust: TRUSTED_PROJECT });
      await answer.promise;
    });
    await settle();

    expect(useAppStore.getState().project).toMatchObject({
      handle: OTHER_HANDLE,
      trust: CHANGED_OUTSIDE,
    });
    expect(screen.queryByTestId('restricted-banner-message')).toBeNull();
  });

  it('asks nothing without a project', async () => {
    const created = featureContext();
    const controller = new TrustController(created.ctx);
    await expect(controller.grant()).resolves.toBeNull();
    await expect(controller.revoke()).resolves.toBeNull();
    expect(created.ipc.trustGrant).not.toHaveBeenCalled();
    expect(created.ipc.trustRevoke).not.toHaveBeenCalled();
    controller.dispose();
  });
});

describe('the This project section of the Settings page', () => {
  it('revokes a project trust record, and the banner comes back', async () => {
    const created = install({ settings: true });
    openProject(TRUSTED_PROJECT);
    renderScreen(created, 'settings');
    await settle();
    created.ipc.trustRevoke.mockResolvedValue({ trust: RESTRICTED });

    const section = screen.getByTestId('settings-trust');
    expect(within(section).getByTestId('settings-trust-state').textContent).toBe(
      trustedText(TRUSTED_PROJECT),
    );
    expect(section.textContent).toContain('Guessing Game');
    await expectNoAxeViolations(screen.getByTestId('settings-page'));

    fireEvent.click(within(section).getByRole('button', { name: 'Revoke trust' }));
    await settle();

    expect(created.ipc.trustRevoke).toHaveBeenCalledWith({ handle: projectFixture().handle });
    expect(useAppStore.getState().project?.trust).toEqual(RESTRICTED);
    expect(banner()).toBeTruthy();
    expect(screen.getByTestId('settings-trust-message').textContent).toContain(
      trustMessageText({ kind: 'revoked' }),
    );
    // While restricted, the section offers Trust… instead.
    expect(within(section).queryByRole('button', { name: 'Revoke trust' })).toBeNull();
    expect(within(section).getByRole('button', { name: 'Trust…' })).toBeTruthy();
    await expectNoAxeViolations(screen.getByTestId('settings-page'));
  });

  it('explains when a folder keeps the project trusted', async () => {
    const created = install({ settings: true });
    openProject(TRUSTED_PROJECT);
    renderScreen(created, 'settings');
    await settle();
    created.ipc.trustRevoke.mockResolvedValue({ trust: TRUSTED_FOLDER });

    fireEvent.click(screen.getByTestId('settings-trust-revoke'));
    await settle();

    expect(screen.queryByTestId('restricted-banner')).toBeNull();
    expect(screen.getByTestId('settings-trust-message').textContent).toContain(
      'still trusted because you trusted everything in its folder',
    );
    expect(screen.getByTestId('settings-trust-state').textContent).toBe(
      trustedText(TRUSTED_FOLDER),
    );
    expect(screen.queryByTestId('settings-trust-revoke')).toBeNull();
  });

  it('shows hidden characters in the project name as placeholders', async () => {
    const created = install({ settings: true });
    act(() => {
      const project = projectFixture({ trust: TRUSTED_PROJECT });
      project.document.project.name = 'Game\u202Etxt.exe';
      useAppStore.getState().actions.setProject(project);
    });
    renderScreen(created, 'settings');
    await settle();
    expect(screen.getByTestId('settings-trust').textContent).toContain('Game⟨U+202E⟩txt.exe');
  });

  it('offers no revoke for folder trust or a project created here', async () => {
    const created = install({ settings: true });
    openProject(CREATED_HERE);
    renderScreen(created, 'settings');
    await settle();
    expect(screen.getByTestId('settings-trust-state').textContent).toBe(trustedText(CREATED_HERE));
    expect(screen.queryByTestId('settings-trust-revoke')).toBeNull();

    openProject(TRUSTED_FOLDER, OTHER_HANDLE);
    expect(screen.queryByTestId('settings-trust-revoke')).toBeNull();
  });

  it('grants trust from the section too, and reports a failed revoke', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const created = install({ settings: true });
    openProject(RESTRICTED);
    renderScreen(created, 'settings');
    await settle();
    created.ipc.trustGrant.mockResolvedValue({ trust: TRUSTED_PROJECT });

    fireEvent.click(
      within(screen.getByTestId('settings-trust')).getByRole('button', { name: 'Trust…' }),
    );
    await settle();
    expect(screen.getByTestId('settings-trust-message').textContent).toContain(
      trustMessageText({ kind: 'granted' }),
    );

    created.ipc.trustRevoke.mockRejectedValue(
      new IpcCallError('trust_revoke', { code: 'internal' }),
    );
    fireEvent.click(screen.getByTestId('settings-trust-revoke'));
    await settle();
    const message = screen.getByTestId('settings-trust-message');
    expect(message.getAttribute('role')).toBe('alert');
    expect(message.textContent).toContain('Trust could not be revoked (internal).');
  });

  it('forgets its message when another project is opened, and hides without a project', async () => {
    const created = install({ settings: true });
    openProject(RESTRICTED);
    renderScreen(created, 'settings');
    await settle();
    created.ipc.trustGrant.mockResolvedValue({ trust: RESTRICTED });
    fireEvent.click(
      within(screen.getByTestId('settings-trust')).getByRole('button', { name: 'Trust…' }),
    );
    await settle();
    expect(screen.getByTestId('settings-trust-message')).toBeTruthy();

    openProject(RESTRICTED, OTHER_HANDLE);
    expect(screen.queryByTestId('settings-trust-message')).toBeNull();

    act(() => {
      useAppStore.getState().actions.setProject(null);
    });
    expect(screen.queryByTestId('settings-trust')).toBeNull();
  });
});

describe('the trust feature', () => {
  it('registers the banner first and the section after the page’s own ones, and removes both', () => {
    const created = install();
    expect(created.banners.banners().map((entry) => [entry.id, entry.order])).toEqual([
      ['restrictedMode', 0],
    ]);
    expect(created.sections.sections().map((entry) => entry.id)).toEqual(['projectTrust']);

    for (const uninstall of uninstallers.splice(0).reverse()) {
      uninstall();
    }
    expect(created.banners.banners()).toEqual([]);
    expect(created.sections.sections()).toEqual([]);
  });

  it('ignores answers that arrive after it was uninstalled', async () => {
    const created = install();
    renderBanners(created);
    openProject(RESTRICTED);
    const answer = deferred<TrustResponse>();
    created.ipc.trustGrant.mockReturnValue(answer.promise);
    fireEvent.click(screen.getByTestId('restricted-banner-trust'));
    await settle();

    for (const uninstall of uninstallers.splice(0).reverse()) {
      uninstall();
    }
    await act(async () => {
      answer.resolve({ trust: TRUSTED_PROJECT });
      await answer.promise;
    });
    expect(useAppStore.getState().project?.trust).toEqual(RESTRICTED);
  });
});

describe('texts', () => {
  it('has a text for every reason and source', () => {
    expect(restrictedReasonText(null)).toBe('This project is not trusted on this computer.');
    expect(trustedText({ ...TRUSTED_PROJECT, source: null })).toBe('This project is trusted.');
    expect(trustMessageText({ kind: 'failed', action: 'grant', code: 'unknownHandle' })).toBe(
      'This project is no longer open.',
    );
  });
});
