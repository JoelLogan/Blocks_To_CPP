/** The live analysis' notices in a window banner: shown while they hold, never silently. */
import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';

import { createBannerRegistry, WindowBanners } from '../../app/banners';
import { createCommandRegistry } from '../../app/commands';
import { createDialogQueue } from '../../app/dialogs/service';
import { createAppEventBus } from '../../app/events';
import type { FeatureContext } from '../../app/features';
import { createScreenRegistry } from '../../app/screens';
import { resetAppStore, useAppStore } from '../../app/store';
import { createFakeIpc, projectFixture } from '../../app/testing/fixtures';
import { expectNoAxeViolations } from '../../test/axe';
import { ANALYSIS_BANNER_ORDER, ANALYSIS_NOTICE_TEXT, createAnalysisNoticeFeature } from './index';

let uninstall: () => void = () => undefined;

function install() {
  const banners = createBannerRegistry();
  const ctx: FeatureContext = {
    ipc: createFakeIpc(),
    store: useAppStore,
    commands: createCommandRegistry(),
    screens: createScreenRegistry(),
    dialogs: createDialogQueue(),
    events: createAppEventBus(),
    core: () => null,
    editor: () => null,
  };
  uninstall = createAnalysisNoticeFeature({ banners })(ctx);
  return banners;
}

beforeEach(() => {
  resetAppStore();
});

afterEach(() => {
  uninstall();
});

describe('the analysis notice banner', () => {
  it('registers between Restricted Mode and the settings notices', () => {
    const banners = install();
    expect(banners.banners().map((banner) => [banner.id, banner.order])).toEqual([
      ['analysisNotice', ANALYSIS_BANNER_ORDER],
    ]);
    uninstall();
    expect(banners.banners()).toEqual([]);
  });

  it('says that the latest change was not checked while the canvas does not load', async () => {
    const banners = install();
    const { container } = render(<WindowBanners registry={banners} />);
    expect(screen.queryByTestId('analysis-notice-banner')).toBeNull();

    act(() => {
      useAppStore.getState().actions.setProject(projectFixture());
      useAppStore.getState().actions.setAnalysis({ notice: 'syncFailed' });
    });
    const banner = screen.getByTestId('analysis-notice-banner');
    expect(
      screen.getByRole('heading', { name: 'Your latest change was not checked' }),
    ).toBeTruthy();
    expect(banner.textContent).toContain(ANALYSIS_NOTICE_TEXT.syncFailed);
    expect(banner.textContent).toContain('Build and Run will not start');
    // It holds until the canvas loads again: nothing to dismiss.
    expect(screen.queryByRole('button')).toBeNull();
    await expectNoAxeViolations(container);

    act(() => {
      useAppStore.getState().actions.setAnalysis({ notice: null });
    });
    expect(screen.queryByTestId('analysis-notice-banner')).toBeNull();
  });

  it('says that the compiler core was restarted, until dismissed', async () => {
    const banners = install();
    const { container } = render(<WindowBanners registry={banners} />);
    act(() => {
      useAppStore.getState().actions.setProject(projectFixture());
      useAppStore.getState().actions.setAnalysis({ notice: 'trapRecovered' });
    });
    expect(screen.getByTestId('analysis-notice-banner').textContent).toContain(
      ANALYSIS_NOTICE_TEXT.trapRecovered,
    );
    await expectNoAxeViolations(container);

    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));
    expect(useAppStore.getState().analysis.notice).toBeNull();
    expect(screen.queryByTestId('analysis-notice-banner')).toBeNull();
  });

  it('shows nothing without a project', () => {
    const banners = install();
    useAppStore.getState().actions.setAnalysis({ notice: 'syncFailed' });
    render(<WindowBanners registry={banners} />);
    expect(screen.queryByTestId('analysis-notice-banner')).toBeNull();
  });
});
