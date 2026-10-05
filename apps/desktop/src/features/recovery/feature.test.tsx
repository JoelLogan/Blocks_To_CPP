/**
 * Installing the recovery feature: it reads the offered snapshots at start-up, puts its offer on
 * the start page through `registerStartPageSection`, runs autosave against the window's `blur`,
 * and takes all of that away again when uninstalled.
 */
import { act, render, screen } from '@testing-library/react';
import type { ComponentType } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { installAll } from '../../app/features';
import { useAppStore } from '../../app/store';
import { projectFixture } from '../../app/testing/fixtures';
import { AUTOSAVE_INTERVAL_MS } from './autosave';
import {
  createRecoveryFeature,
  installRecoveryFeature,
  RECOVERY_SECTION_ID,
  RECOVERY_SECTION_ORDER,
  recoveryFeature,
  type RegisterStartPageSection,
} from './feature';
import { createHarness, HANDLE_A, type Harness, snapshotInfo } from './testing';

interface Section {
  id: string;
  component: ComponentType;
  order: number | undefined;
}

let harness: Harness;
let sections: Section[];
let register: RegisterStartPageSection;

beforeEach(() => {
  harness = createHarness();
  sections = [];
  register = vi.fn((id: string, component: ComponentType, options?: { order?: number }) => {
    const section = { id, component, order: options?.order };
    sections.push(section);
    return () => {
      sections = sections.filter((candidate) => candidate !== section);
    };
  });
});

afterEach(() => {
  harness.dispose();
  vi.useRealTimers();
});

describe('the recovery feature', () => {
  it('lists the snapshots at start-up and offers them on the start page', async () => {
    harness.ipc.recoveryList.mockResolvedValue({ snapshots: [snapshotInfo(1)] });
    const feature = installRecoveryFeature(harness.ctx, {
      registerStartPageSection: register,
      window: null,
    });
    expect(harness.ipc.recoveryList).toHaveBeenCalledTimes(1);
    expect(sections.map(({ id, order }) => [id, order])).toEqual([
      [RECOVERY_SECTION_ID, RECOVERY_SECTION_ORDER],
    ]);
    const Section = sections[0]?.component;
    if (Section === undefined) {
      throw new Error('no section');
    }
    render(<Section />);
    expect(await screen.findByRole('button', { name: 'Restore “Project 1”' })).toBeTruthy();

    feature.uninstall();
    feature.uninstall();
    expect(sections).toEqual([]);
  });

  it('writes a snapshot when the window loses focus, until it is uninstalled', async () => {
    const target = new EventTarget();
    const feature = installRecoveryFeature(harness.ctx, {
      registerStartPageSection: register,
      window: target,
    });
    useAppStore
      .getState()
      .actions.setProject(
        projectFixture({ handle: HANDLE_A, dirty: true, canonicalText: '{"a":1}' }),
      );
    target.dispatchEvent(new Event('blur'));
    await vi.waitFor(() => {
      expect(harness.ipc.recoverySave).toHaveBeenCalledWith({
        handle: HANDLE_A,
        document: '{"a":1}',
      });
    });

    feature.uninstall();
    useAppStore.getState().actions.updateProject({ canonicalText: '{"a":2}' });
    target.dispatchEvent(new Event('blur'));
    await act(() => Promise.resolve());
    expect(harness.ipc.recoverySave).toHaveBeenCalledTimes(1);
  });

  it('autosaves with the interval it is given', async () => {
    vi.useFakeTimers();
    const feature = installRecoveryFeature(harness.ctx, {
      registerStartPageSection: register,
      window: null,
      intervalMs: 1000,
    });
    useAppStore.getState().actions.setProject(projectFixture({ handle: HANDLE_A, dirty: true }));
    await vi.advanceTimersByTimeAsync(1000);
    expect(harness.ipc.recoverySave).toHaveBeenCalledTimes(1);
    feature.uninstall();
    expect(AUTOSAVE_INTERVAL_MS).toBe(30_000);
  });

  it('listens to the app window by default', () => {
    const add = vi.spyOn(window, 'addEventListener');
    const remove = vi.spyOn(window, 'removeEventListener');
    const feature = installRecoveryFeature(harness.ctx, { registerStartPageSection: register });
    expect(add).toHaveBeenCalledWith('blur', expect.any(Function));
    feature.uninstall();
    expect(remove).toHaveBeenCalledWith('blur', expect.any(Function));
  });

  it('works as a Feature, and says so when it has no start page', () => {
    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const uninstall = installAll(
      [createRecoveryFeature({ registerStartPageSection: register, window: null })],
      harness.ctx,
    );
    expect(sections).toHaveLength(1);
    uninstall();
    expect(sections).toHaveLength(0);
    expect(error).not.toHaveBeenCalled();

    const autosaveOnly = installAll([recoveryFeature], harness.ctx);
    expect(error).toHaveBeenCalledWith(
      'The recovery feature has no start page section; unsaved work is not offered',
    );
    autosaveOnly();
  });
});
