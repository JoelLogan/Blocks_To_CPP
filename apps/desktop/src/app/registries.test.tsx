/** The command and screen registries and the app event bus. */
import { act, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import {
  createCommandRegistry,
  registerCommand,
  runCommand,
  triggerCommand,
  useCommandAvailable,
} from './commands';
import { createAppEventBus, isAppEvent } from './events';
import { createScreenRegistry, registerScreen, screens, useRegisteredScreen } from './screens';

/** A promise the test settles when it wants to. */
function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

describe('the command registry', () => {
  it('runs the newest handler and falls back when it is removed', async () => {
    const registry = createCommandRegistry();
    const first = vi.fn();
    const second = vi.fn();
    registry.registerCommand('project.save', first);
    const removeSecond = registry.registerCommand('project.save', second);

    await registry.runCommand('project.save');
    expect(second).toHaveBeenCalledTimes(1);
    expect(first).not.toHaveBeenCalled();

    removeSecond();
    removeSecond(); // a second call does nothing
    await registry.runCommand('project.save');
    expect(first).toHaveBeenCalledTimes(1);
  });

  it('keeps registrations of the same function apart', async () => {
    const registry = createCommandRegistry();
    const handler = vi.fn();
    const removeA = registry.registerCommand('run.stop', handler);
    registry.registerCommand('run.stop', handler);

    removeA();
    expect(registry.hasCommand('run.stop')).toBe(true);
    await registry.runCommand('run.stop');
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it('does nothing for a command without a handler', async () => {
    const registry = createCommandRegistry();
    expect(registry.hasCommand('edit.copy')).toBe(false);
    await expect(registry.runCommand('edit.copy')).resolves.toBeUndefined();
  });

  it('runs a handler synchronously, inside the event that triggered it', () => {
    const registry = createCommandRegistry();
    const handler = vi.fn();
    registry.registerCommand('edit.copy', handler);

    void registry.runCommand('edit.copy');
    expect(handler).toHaveBeenCalledTimes(1);
  });

  it('does not start a running command again', async () => {
    const registry = createCommandRegistry();
    const gate = deferred();
    const handler = vi.fn(() => gate.promise);
    registry.registerCommand('build.start', handler);

    const first = registry.runCommand('build.start');
    const second = registry.runCommand('build.start');
    expect(second).toBe(first);
    expect(handler).toHaveBeenCalledTimes(1);

    gate.resolve();
    await first;
    await registry.runCommand('build.start');
    expect(handler).toHaveBeenCalledTimes(2);
  });

  it('rejects when the handler fails, synchronously or not', async () => {
    const registry = createCommandRegistry();
    registry.registerCommand('project.open', () => {
      throw new Error('broken');
    });
    registry.registerCommand('project.new', () => Promise.reject(new Error('later')));
    registry.registerCommand('project.close', () => {
      // A thrown value that is not an Error is wrapped in one.
      throw 'text' as unknown as Error;
    });

    await expect(registry.runCommand('project.open')).rejects.toThrow('broken');
    await expect(registry.runCommand('project.new')).rejects.toThrow('later');
    await expect(registry.runCommand('project.close')).rejects.toMatchObject({ cause: 'text' });
    // A failed command can run again.
    await expect(registry.runCommand('project.new')).rejects.toThrow('later');
  });

  it('logs the failure of a command run from the user interface', async () => {
    const registry = createCommandRegistry();
    const failure = new Error('broken');
    registry.registerCommand('run.again', () => Promise.reject(failure));
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);

    triggerCommand('run.again', registry);
    await vi.waitFor(() => {
      expect(consoleError).toHaveBeenCalledWith('Command run.again failed', failure);
    });
  });

  it('works through the module functions on the app registry', async () => {
    const handler = vi.fn();
    const remove = registerCommand('project.saveAs', handler);
    await runCommand('project.saveAs');
    triggerCommand('project.saveAs');
    remove();

    expect(handler).toHaveBeenCalledTimes(2);
  });

  it('tells components whether a command is available', () => {
    const registry = createCommandRegistry();
    function Probe() {
      return <p>{useCommandAvailable('settings.open', registry) ? 'yes' : 'no'}</p>;
    }
    render(<Probe />);
    expect(screen.getByText('no')).toBeTruthy();

    let remove: () => void = () => undefined;
    act(() => {
      remove = registry.registerCommand('settings.open', vi.fn());
    });
    expect(screen.getByText('yes')).toBeTruthy();

    act(() => {
      remove();
    });
    expect(screen.getByText('no')).toBeTruthy();
  });
});

describe('the screen registry', () => {
  function Start() {
    return <p>start</p>;
  }
  function OtherStart() {
    return <p>other start</p>;
  }

  it('shows the newest component and falls back when it is removed', () => {
    const registry = createScreenRegistry();
    expect(registry.screen('start')).toBeNull();

    const removeFirst = registry.registerScreen('start', Start);
    const removeSecond = registry.registerScreen('start', OtherStart);
    expect(registry.screen('start')).toBe(OtherStart);

    removeSecond();
    removeSecond();
    expect(registry.screen('start')).toBe(Start);
    removeFirst();
    expect(registry.screen('start')).toBeNull();
  });

  it('never has a component for the editor, which belongs to the shell', () => {
    expect(createScreenRegistry().screen('editor')).toBeNull();
  });

  it('tells components what is registered, on the app registry', () => {
    function Probe() {
      const Component = useRegisteredScreen('settings');
      return Component === null ? <p>none</p> : <Component />;
    }
    render(<Probe />);
    expect(screen.getByText('none')).toBeTruthy();

    let remove: () => void = () => undefined;
    act(() => {
      remove = registerScreen('settings', Start);
    });
    expect(screen.getByText('start')).toBeTruthy();
    expect(screens.screen('settings')).toBe(Start);

    act(() => {
      remove();
    });
    expect(screen.getByText('none')).toBeTruthy();
  });
});

describe('the app event bus', () => {
  it('delivers events of a kind to their handlers, in order, until they unsubscribe', () => {
    const bus = createAppEventBus();
    const calls: string[] = [];
    const offA = bus.on('closeRequested', () => calls.push('a'));
    bus.on('closeRequested', () => calls.push('b'));
    bus.on('toolchainsUpdated', () => calls.push('other kind'));

    bus.emit({ kind: 'closeRequested' });
    offA();
    bus.emit({ kind: 'closeRequested' });

    expect(calls).toEqual(['a', 'b', 'b']);
  });

  it('carries local events with their fields', () => {
    const bus = createAppEventBus();
    const handler = vi.fn();
    bus.on('project:changedOnDisk', handler);

    bus.emit({ kind: 'project:changedOnDisk', handle: 'ph_0123456789abcdef0123456789abcdef' });
    expect(handler).toHaveBeenCalledWith({
      kind: 'project:changedOnDisk',
      handle: 'ph_0123456789abcdef0123456789abcdef',
    });
  });

  it('keeps going when a handler throws', () => {
    const bus = createAppEventBus();
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    const after = vi.fn();
    bus.on('settingsNotice', () => {
      throw new Error('broken handler');
    });
    bus.on('settingsNotice', after);

    bus.emit({ kind: 'settingsNotice', notices: [] });
    expect(after).toHaveBeenCalledTimes(1);
    expect(consoleError).toHaveBeenCalledTimes(1);
  });

  it('accepts only well-formed app events', () => {
    expect(isAppEvent({ kind: 'closeRequested' })).toBe(true);
    expect(isAppEvent({ kind: 'toolchainsUpdated', toolchains: [], discovering: false })).toBe(
      true,
    );
    expect(isAppEvent({ kind: 'settingsNotice', notices: [] })).toBe(true);
    expect(
      isAppEvent({
        kind: 'projectChangedOnDisk',
        handle: 'ph_0123456789abcdef0123456789abcdef',
        deleted: true,
      }),
    ).toBe(true);

    for (const bad of [
      null,
      'closeRequested',
      [],
      {},
      { kind: 'somethingNew' },
      { kind: 'toolchainsUpdated', toolchains: 'many', discovering: false },
      { kind: 'toolchainsUpdated', toolchains: [] },
      { kind: 'settingsNotice', notices: {} },
      { kind: 'projectChangedOnDisk', handle: '../etc/passwd', deleted: false },
      { kind: 'projectChangedOnDisk', handle: 'ph_0123', deleted: 'yes' },
    ]) {
      expect(isAppEvent(bad)).toBe(false);
    }
  });
});
