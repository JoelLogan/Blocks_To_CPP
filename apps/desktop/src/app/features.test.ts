import { describe, expect, it, vi } from 'vitest';

import { installFeatures } from '../features';
import { createCommandRegistry } from './commands';
import { createDialogQueue } from './dialogs';
import { EDITOR_PLUGINS } from './editorPlugins';
import { createAppEventBus } from './events';
import { type Feature, type FeatureContext, installAll } from './features';
import { createScreenRegistry } from './screens';
import { useAppStore } from './store';
import { createFakeIpc } from './testing/fixtures';

function context(): FeatureContext {
  return {
    ipc: createFakeIpc(),
    store: useAppStore,
    commands: createCommandRegistry(),
    screens: createScreenRegistry(),
    dialogs: createDialogQueue(),
    events: createAppEventBus(),
    core: () => null,
    editor: () => null,
  };
}

describe('installAll', () => {
  it('installs features in order and uninstalls them in reverse, once', () => {
    const calls: string[] = [];
    const feature =
      (name: string): Feature =>
      () => {
        calls.push(`install ${name}`);
        return () => calls.push(`uninstall ${name}`);
      };
    const ctx = context();

    const uninstall = installAll([feature('a'), feature('b')], ctx);
    uninstall();
    uninstall();

    expect(calls).toEqual(['install a', 'install b', 'uninstall b', 'uninstall a']);
  });

  it('passes the context to every feature', () => {
    const ctx = context();
    const feature = vi.fn(() => () => undefined);
    installAll([feature], ctx);
    expect(feature).toHaveBeenCalledWith(ctx);
  });

  it('skips a feature that fails to install, and logs failing clean-ups', () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const after = vi.fn(() => () => {
      throw new Error('cannot uninstall');
    });
    function broken(): () => void {
      throw new Error('cannot install');
    }

    const uninstall = installAll([broken, after], context());
    expect(after).toHaveBeenCalledTimes(1);
    expect(consoleError).toHaveBeenCalledWith(
      'The feature broken failed to install',
      expect.any(Error),
    );

    uninstall();
    expect(consoleError).toHaveBeenCalledWith('A feature failed to uninstall', expect.any(Error));
  });

  it('names an anonymous feature in the log', () => {
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const features: Feature[] = [
      () => {
        throw new Error('x');
      },
    ];
    installAll(features, context());
    expect(consoleError).toHaveBeenCalledWith(
      'The feature (unnamed) failed to install',
      expect.any(Error),
    );
  });
});

describe('the lists later waves extend', () => {
  it('start empty', () => {
    expect(EDITOR_PLUGINS).toEqual([]);
    const uninstall = installFeatures(context());
    expect(uninstall).toBeTypeOf('function');
    uninstall();
  });
});
