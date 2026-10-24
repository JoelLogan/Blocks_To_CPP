/** One run's channels: ordering by `afterSeq`, acknowledgements, the prelude and detaching. */
import type { RunEvent } from '@blocks2cpp/ipc-types';
import { afterEach, beforeEach, describe, expect, it, type Mock, vi } from 'vitest';

import { createFakeIpc } from '../../app/testing/fixtures';
import { systemClock } from './clock';
import { ConsoleBridge } from './consoleBridge';
import { MISSING_OUTPUT_WAIT_MS, RunSession, type RunSessionHooks } from './runSession';
import { FakeConsole, runIdOf, settle } from './testing';

const encoder = new TextEncoder();

function batch(text: string): ArrayBuffer {
  return encoder.encode(text).slice().buffer;
}

function exit(afterSeq: number): RunEvent {
  return {
    kind: 'exit',
    afterSeq,
    elapsedMs: 5,
    status: { type: 'exited', code: 3 },
    crash: null,
    sanitizer: null,
    message: 'Finished with exit code 3',
  };
}

let terminal: FakeConsole;
let bridge: ConsoleBridge;
let onStarted: Mock<RunSessionHooks['onStarted']>;
let onExit: Mock<RunSessionHooks['onExit']>;
let ipc: ReturnType<typeof createFakeIpc>;

function session(prelude: string | null = null): RunSession {
  return new RunSession({ ipc, bridge, clock: systemClock, hooks: { onStarted, onExit }, prelude });
}

beforeEach(() => {
  vi.useFakeTimers();
  terminal = new FakeConsole();
  terminal.autoResolve = false;
  bridge = new ConsoleBridge();
  bridge.attach(terminal);
  onStarted = vi.fn<RunSessionHooks['onStarted']>();
  onExit = vi.fn<RunSessionHooks['onExit']>();
  ipc = createFakeIpc();
  ipc.runAck.mockResolvedValue({});
  ipc.runStop.mockResolvedValue({});
  ipc.runResize.mockResolvedValue({});
});

afterEach(() => {
  vi.useRealTimers();
});

describe('RunSession', () => {
  it('applies started at once and the exit only after afterSeq batches are written', async () => {
    const run = session();
    run.onEvent({ kind: 'started', containment: 'jobObject', mode: 'pipes', ideHelpers: false });
    expect(onStarted).toHaveBeenCalledWith({
      containment: 'jobObject',
      mode: 'pipes',
      ideHelpers: false,
      at: Date.now(),
    });
    expect(bridge.mode()).toBe('pipes');

    run.onEvent(exit(2));
    run.onOutput(batch('a'));
    run.onOutput(batch('b'));
    await settle();
    expect(onExit).not.toHaveBeenCalled();
    // Written out of order: batch 2 first does not count until batch 1 is written.
    const [first, second] = terminal.pending.splice(0);
    second?.();
    await settle();
    expect(run.writtenBatches).toBe(0);
    expect(onExit).not.toHaveBeenCalled();
    first?.();
    await settle();
    expect(run.writtenBatches).toBe(2);
    expect(onExit).toHaveBeenCalledWith(exit(2));
    expect(run.hasEnded).toBe(true);
    await expect(run.ended).resolves.toBeUndefined();
  });

  it('acknowledges once the run ID is known', async () => {
    const run = session();
    terminal.autoResolve = true;
    run.onOutput(batch('x'));
    await settle();
    expect(ipc.runAck).not.toHaveBeenCalled();
    run.started(runIdOf(7));
    await settle();
    expect(ipc.runAck).toHaveBeenCalledWith({ runId: runIdOf(7), seq: 1 });
  });

  it('writes the prelude before the first output or at the start, once', async () => {
    terminal.autoResolve = true;
    const run = session('--sep--');
    run.onOutput(batch('first'));
    run.onEvent({ kind: 'started', containment: 'cgroup', mode: 'pty', ideHelpers: true });
    run.onOutput(batch('second'));
    await settle();
    expect(terminal.text).toBe('--sep--firstsecond');

    const quiet = session('==');
    quiet.onEvent({ kind: 'started', containment: 'cgroup', mode: 'pty', ideHelpers: true });
    expect(terminal.text).toBe('--sep--firstsecond==');
  });

  it('ignores everything after the exit and events of an unknown shape', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    terminal.autoResolve = true;
    const run = session();
    run.onEvent({ kind: 'teleported' });
    expect(warn).toHaveBeenCalledWith('Ignored a run event of an unknown shape');
    run.onEvent(exit(0));
    run.onEvent(exit(0));
    run.onEvent({ kind: 'skipped', lines: 2, afterSeq: 0 });
    await settle();
    expect(onExit).toHaveBeenCalledTimes(1);
    expect(terminal.skipped).toEqual([]);
  });

  it('applies the waiting events when output batches never arrive', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const run = session();
    run.onOutput(batch('1'));
    run.onEvent({ kind: 'skipped', lines: 4, afterSeq: 2 });
    run.onEvent(exit(3));
    // Batch 1 is not written yet: nothing is missing so far.
    await vi.advanceTimersByTimeAsync(MISSING_OUTPUT_WAIT_MS * 2);
    expect(onExit).not.toHaveBeenCalled();
    terminal.resolveWrites();
    await settle();
    await vi.advanceTimersByTimeAsync(MISSING_OUTPUT_WAIT_MS - 1);
    expect(onExit).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(terminal.skipped).toEqual([4]);
    expect(onExit).toHaveBeenCalledTimes(1);
    expect(warn).toHaveBeenCalledTimes(1);
  });

  it('a late batch disarms the fallback', async () => {
    terminal.autoResolve = true;
    const run = session();
    run.onEvent(exit(1));
    await vi.advanceTimersByTimeAsync(MISSING_OUTPUT_WAIT_MS / 2);
    run.onOutput(batch('late'));
    await settle();
    expect(onExit).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(MISSING_OUTPUT_WAIT_MS);
    expect(onExit).toHaveBeenCalledTimes(1);
  });

  it('stops now or as soon as the ID is known, and not after the end', () => {
    const run = session();
    run.stop();
    expect(ipc.runStop).not.toHaveBeenCalled();
    run.started(runIdOf(3));
    expect(ipc.runStop).toHaveBeenCalledWith({ runId: runIdOf(3) });

    const ended = session();
    ended.started(runIdOf(4));
    terminal.autoResolve = true;
    ended.onEvent(exit(0));
    ended.stop();
    expect(ipc.runStop).toHaveBeenCalledTimes(1);
  });

  it('logs a failed stop unless the program had just ended', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const { IpcCallError } = await import('@blocks2cpp/ipc-types');
    ipc.runStop.mockRejectedValueOnce(new IpcCallError('run_stop', { code: 'notRunning' }));
    ipc.runStop.mockRejectedValueOnce(new IpcCallError('run_stop', { code: 'internal' }));
    const a = session();
    a.started(runIdOf(5));
    a.stop();
    const b = session();
    b.started(runIdOf(6));
    b.stop();
    await settle();
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn).toHaveBeenCalledWith('run_stop failed', 'internal');
  });

  it('sends only the newest size, one call at a time', async () => {
    let answer: () => void = () => undefined;
    ipc.runResize.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          answer = () => {
            resolve({});
          };
        }),
    );
    const run = session();
    run.resized({ cols: 90, rows: 20 });
    expect(ipc.runResize).not.toHaveBeenCalled();
    run.started(runIdOf(8));
    expect(ipc.runResize).toHaveBeenCalledWith({ runId: runIdOf(8), cols: 90, rows: 20 });
    run.resized({ cols: 91, rows: 20 });
    run.resized({ cols: 92, rows: 21 });
    expect(ipc.runResize).toHaveBeenCalledTimes(1);
    answer();
    await settle();
    expect(ipc.runResize.mock.calls.map(([request]) => request.cols)).toEqual([90, 92]);
  });

  it('after detach, writes, applies and sends nothing', async () => {
    terminal.autoResolve = true;
    const run = session();
    run.started(runIdOf(9));
    run.detach();
    expect(run.hasEnded).toBe(true);
    run.onOutput(batch('gone'));
    run.onEvent(exit(1));
    run.typed('typed');
    await settle();
    expect(terminal.text).toBe('');
    expect(onExit).not.toHaveBeenCalled();
    expect(ipc.runInput).not.toHaveBeenCalled();
  });
});
