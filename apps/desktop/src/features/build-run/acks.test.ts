/** The pacing of `run_ack`: at most every 100 ms, with the highest batch written. */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ACK_INTERVAL_MS, AckPacer } from './acks';
import { systemClock } from './clock';

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

/** The pacer with a recording `send`, and the times (ms from the start) of each call. */
function pacer(send = vi.fn<(seq: number) => Promise<void>>(() => Promise.resolve())) {
  const start = Date.now();
  const times: number[] = [];
  const recording = vi.fn((seq: number) => {
    times.push(Date.now() - start);
    return send(seq);
  });
  return { acks: new AckPacer(recording, systemClock), send: recording, times };
}

describe('AckPacer', () => {
  it('sends nothing until it is ready, then the highest batch written', async () => {
    const { acks, send } = pacer();
    acks.written(1);
    acks.written(3);
    expect(send).not.toHaveBeenCalled();
    acks.ready();
    await vi.advanceTimersByTimeAsync(0);
    expect(send.mock.calls).toEqual([[3]]);
    expect(acks.acked).toBe(3);
  });

  it('acknowledges at most every 100 ms, always the highest batch written', async () => {
    const { acks, send, times } = pacer();
    acks.ready();
    // A batch is written every 10 ms for one second.
    for (let seq = 1; seq <= 100; seq++) {
      acks.written(seq);
      await vi.advanceTimersByTimeAsync(10);
    }
    await vi.advanceTimersByTimeAsync(ACK_INTERVAL_MS);

    const seqs = send.mock.calls.map(([seq]) => seq);
    expect(seqs.at(-1)).toBe(100);
    expect(seqs).toEqual([...seqs].sort((a, b) => a - b));
    for (let index = 1; index < times.length; index++) {
      expect((times[index] ?? 0) - (times[index - 1] ?? 0)).toBeGreaterThanOrEqual(ACK_INTERVAL_MS);
    }
    expect(send.mock.calls.length).toBeLessThanOrEqual(11);
  });

  it('ignores a batch number it has seen, and sends nothing new when nothing new was written', async () => {
    const { acks, send } = pacer();
    acks.ready();
    acks.written(5);
    acks.written(4);
    await vi.advanceTimersByTimeAsync(ACK_INTERVAL_MS * 3);
    expect(send.mock.calls).toEqual([[5]]);
  });

  it('waits for the acknowledgement on its way and carries on after a failure', async () => {
    const debug = vi.spyOn(console, 'debug').mockImplementation(() => undefined);
    let fail: (error: Error) => void = () => undefined;
    const send = vi
      .fn<(seq: number) => Promise<void>>(() => Promise.resolve())
      .mockImplementationOnce(
        () =>
          new Promise<void>((_resolve, reject) => {
            fail = reject;
          }),
      );
    const { acks, send: recorded } = pacer(send);
    acks.ready();
    acks.written(1);
    acks.written(2);
    await vi.advanceTimersByTimeAsync(ACK_INTERVAL_MS * 2);
    expect(recorded).toHaveBeenCalledTimes(1);
    fail(new Error('unknownRun'));
    await vi.advanceTimersByTimeAsync(0);
    expect(recorded.mock.calls).toEqual([[1], [2]]);
    expect(debug).toHaveBeenCalled();
  });

  it('sends nothing more once disposed', async () => {
    const { acks, send } = pacer();
    acks.ready();
    acks.written(1);
    acks.written(2);
    acks.dispose();
    await vi.advanceTimersByTimeAsync(ACK_INTERVAL_MS * 2);
    expect(send.mock.calls).toEqual([[1]]);
  });
});
