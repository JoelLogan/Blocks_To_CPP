/**
 * The window's status region (docs/plans/m3-contracts/w3-modules.md §2.11): the last message, with
 * its action; a new message replaces it, and a feature takes back only its own.
 */
import { describe, expect, it, vi } from 'vitest';

import { appStatus, createStatusRegion } from './status';

describe('the status region', () => {
  it('shows the last message, each with its own serial', () => {
    const region = createStatusRegion();
    expect(region.state.getState().message).toBeNull();
    const first = region.show('Module “utils” added.');
    const second = region.show('Module “utils” added.');
    expect(second).toBeGreaterThan(first);
    expect(region.state.getState().message).toEqual({
      text: 'Module “utils” added.',
      serial: second,
      action: null,
    });
  });

  it('keeps a message’s action with it, and drops it with the next message', () => {
    const region = createStatusRegion();
    const run = vi.fn();
    region.show('Module “utils” deleted.', { label: 'Undo delete', run });
    region.state.getState().message?.action?.run();
    expect(run).toHaveBeenCalledOnce();
    region.show('Module “main” renamed to “app”.');
    expect(region.state.getState().message?.action).toBeNull();
  });

  it('takes a message away only while it is still shown', () => {
    const region = createStatusRegion();
    const old = region.show('one');
    const shown = region.show('two');
    region.clear(old);
    expect(region.state.getState().message?.text).toBe('two');
    region.clear(shown);
    expect(region.state.getState().message).toBeNull();
    region.show('three');
    region.clear();
    expect(region.state.getState().message).toBeNull();
  });

  it('is one region for the whole app', () => {
    expect(appStatus.state.getState()).toEqual({ message: null });
  });
});
