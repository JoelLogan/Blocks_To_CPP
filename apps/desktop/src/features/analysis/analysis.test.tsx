/**
 * The live analysis' notices in a window banner: shown while they hold, never silently; a canvas
 * that does not load offers *Undo last change* and names a limit the person went past.
 */
import { act, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { createBannerRegistry, WindowBanners } from '../../app/banners';
import { type CommandRegistry, createCommandRegistry } from '../../app/commands';
import { createDialogQueue } from '../../app/dialogs/service';
import { createAppEventBus } from '../../app/events';
import type { FeatureContext } from '../../app/features';
import { createScreenRegistry } from '../../app/screens';
import { resetAppStore, useAppStore } from '../../app/store';
import { createFakeIpc, projectFixture } from '../../app/testing/fixtures';
import { expectNoAxeViolations } from '../../test/axe';
import { syncFailedText } from './AnalysisNoticeBanner';
import { ANALYSIS_BANNER_ORDER, ANALYSIS_NOTICE_TEXT, createAnalysisNoticeFeature } from './index';

let uninstall: () => void = () => undefined;
/** The commands of the installed feature's context. */
let commands: CommandRegistry;

function install() {
  const banners = createBannerRegistry();
  commands = createCommandRegistry();
  const ctx: FeatureContext = {
    ipc: createFakeIpc(),
    store: useAppStore,
    commands,
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
    // It holds until the canvas loads again: nothing to dismiss, but the change can be undone.
    expect(screen.queryByRole('button', { name: 'Dismiss' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Undo last change' })).toBeTruthy();
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

  it('undoes the last change from its button, as often as it is pressed', () => {
    const banners = install();
    const undo = vi.fn();
    commands.registerCommand('edit.undo', undo);
    render(<WindowBanners registry={banners} />);
    act(() => {
      useAppStore.getState().actions.setProject(projectFixture());
      useAppStore
        .getState()
        .actions.setAnalysis({ notice: 'syncFailed', syncCodes: ['B2C-E0104'] });
    });
    const button = screen.getByRole('button', { name: 'Undo last change' });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(undo).toHaveBeenCalledTimes(2);
    // The notice stays until the canvas loads again: only the next preview clears it.
    expect(useAppStore.getState().analysis.notice).toBe('syncFailed');
  });

  it('names a limit the canvas went past instead of calling it a bug', async () => {
    const banners = install();
    const { container } = render(<WindowBanners registry={banners} />);
    act(() => {
      useAppStore.getState().actions.setProject(projectFixture());
      useAppStore.getState().actions.setAnalysis({
        notice: 'syncFailed',
        syncCodes: ['B2C-E0104', 'B2C-E0104'],
      });
    });
    const banner = screen.getByTestId('analysis-notice-banner');
    expect(banner.textContent).toContain(
      'Blocks are nested more deeply than a project allows (B2C-E0104).',
    );
    expect(banner.textContent).toContain('move some of the innermost blocks into a function');
    expect(banner.textContent).toContain('Build and Run will not start');
    expect(banner.textContent).not.toContain('cannot read your latest change back');
    expect(banner.textContent).not.toContain('bug');
    await expectNoAxeViolations(container);

    act(() => {
      useAppStore.getState().actions.setAnalysis({ syncCodes: ['B2C-E0121'] });
    });
    expect(banner.textContent).toContain('more than 100,000 blocks');
    expect(banner.textContent).toContain('split the program into several projects');
  });

  it('words each reachable limit, and anything else as before', () => {
    expect(syncFailedText(['B2C-E0101'])).toContain('larger than 32 MiB');
    expect(syncFailedText(['B2C-E0110', 'B2C-E0104'])).toContain('nested more deeply');
    expect(syncFailedText(['B2C-E0110'])).toBe(ANALYSIS_NOTICE_TEXT.syncFailed);
    expect(syncFailedText([])).toBe(ANALYSIS_NOTICE_TEXT.syncFailed);
  });

  it('shows nothing without a project', () => {
    const banners = install();
    useAppStore.getState().actions.setAnalysis({ notice: 'syncFailed' });
    render(<WindowBanners registry={banners} />);
    expect(screen.queryByTestId('analysis-notice-banner')).toBeNull();
  });
});
