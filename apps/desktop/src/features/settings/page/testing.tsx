/**
 * Test helpers for the toolchain, settings and trust features: a feature context over the fake
 * IPC client with fresh registries, and rendering a registered screen with the dialog host. Only
 * tests import this module; it is never part of the app.
 */
import type { IpcCallError } from '@blocks2cpp/ipc-types';
import { act, render, type RenderResult } from '@testing-library/react';

import { type BannerRegistry, createBannerRegistry, WindowBanners } from '../../../app/banners';
import { createCommandRegistry } from '../../../app/commands';
import { createDialogQueue, DialogHost, type DialogQueue } from '../../../app/dialogs';
import { createAppEventBus } from '../../../app/events';
import type { FeatureContext } from '../../../app/features';
import { type RegisteredScreenId, createScreenRegistry } from '../../../app/screens';
import { useAppStore } from '../../../app/store';
import { createFakeIpc, type FakeIpc } from '../../../app/testing/fixtures';
import { createSettingsSectionRegistry, type SettingsSectionRegistry } from '../sections';

/** A feature context for tests, with its parts. */
export interface TestFeatureContext {
  ctx: FeatureContext;
  ipc: FakeIpc;
  dialogs: DialogQueue;
  banners: BannerRegistry;
  sections: SettingsSectionRegistry;
}

/** A context over a fake IPC client, the app's store and fresh registries. */
export function featureContext(): TestFeatureContext {
  const ipc = createFakeIpc();
  const dialogs = createDialogQueue();
  const ctx: FeatureContext = {
    ipc,
    store: useAppStore,
    commands: createCommandRegistry(),
    screens: createScreenRegistry(),
    dialogs,
    events: createAppEventBus(),
    core: () => null,
    editor: () => null,
  };
  return {
    ctx,
    ipc,
    dialogs,
    banners: createBannerRegistry(),
    sections: createSettingsSectionRegistry(),
  };
}

/** Renders the screen registered as `id`, the window banners and the dialog host. */
export function renderScreen(test: TestFeatureContext, id: RegisteredScreenId): RenderResult {
  const Screen = test.ctx.screens.screen(id);
  if (Screen === null) {
    throw new Error(`no screen is registered as ${id}`);
  }
  return render(
    <>
      <WindowBanners registry={test.banners} />
      <main aria-label="Screen">
        <Screen />
      </main>
      <DialogHost queue={test.dialogs} />
    </>,
  );
}

/** Renders only the window banners and the dialog host. */
export function renderBanners(test: TestFeatureContext): RenderResult {
  return render(
    <>
      <WindowBanners registry={test.banners} />
      <DialogHost queue={test.dialogs} />
    </>,
  );
}

/** A promise with its resolve and reject functions, to control when a fake call answers. */
export interface Deferred<T> {
  promise: Promise<T>;
  resolve: (value: T) => void;
  reject: (reason: IpcCallError | Error) => void;
}

/** Creates a {@link Deferred}. */
export function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: IpcCallError | Error) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Lets pending promises and the React updates they cause run. */
export async function settle(): Promise<void> {
  await act(async () => {
    for (let round = 0; round < 5; round += 1) {
      await Promise.resolve();
    }
  });
}
