/** Typed input: UTF-8, chunks of at most 64 KiB, base64, the backend's limits and retries. */
import { IpcCallError } from '@blocks2cpp/ipc-types';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { systemClock } from './clock';
import {
  chunkBytes,
  encodeInput,
  FIRST_RETRY_DELAY_MS,
  INPUT_BYTES_PER_SECOND,
  INPUT_CALLS_PER_SECOND,
  InputSender,
  MAX_PENDING_INPUT_BYTES,
  MAX_RUN_INPUT_BASE64,
  MAX_RUN_INPUT_BYTES,
  toBase64,
} from './input';
import { fromBase64 } from './testing';

const decoder = new TextDecoder();

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

function sender(send = vi.fn<(data: string) => Promise<void>>(() => Promise.resolve())) {
  const input = new InputSender(send, systemClock);
  input.ready();
  const sent = () => send.mock.calls.map(([data]) => fromBase64(data));
  return { input, send, sent };
}

describe('encoding', () => {
  it('encodes UTF-8 as standard base64', () => {
    expect(toBase64(new Uint8Array([]))).toBe('');
    expect(encodeInput('42\r')).toEqual(['NDIN']);
    expect(encodeInput('héllo ✓')).toEqual([btoa('hÃ©llo â\u009c\u0093')]);
  });

  it('cuts input into chunks of at most 64 KiB, each at most 87,384 base64 characters', () => {
    const text = 'é'.repeat(MAX_RUN_INPUT_BYTES); // two bytes each
    const chunks = encodeInput(text);
    expect(chunks).toHaveLength(2);
    for (const chunk of chunks) {
      expect(chunk.length).toBeLessThanOrEqual(MAX_RUN_INPUT_BASE64);
      expect(fromBase64(chunk).length).toBe(MAX_RUN_INPUT_BYTES);
    }
    const joined = new Uint8Array(chunks.flatMap((chunk) => [...fromBase64(chunk)]));
    expect(decoder.decode(joined)).toBe(text);
    expect(chunkBytes(new Uint8Array(5), 2).map((chunk) => chunk.length)).toEqual([2, 2, 1]);
    expect(btoa('x'.repeat(MAX_RUN_INPUT_BYTES)).length).toBe(MAX_RUN_INPUT_BASE64);
  });
});

describe('InputSender', () => {
  it('waits until it is ready, then sends in order, combining what waited', async () => {
    const send = vi.fn<(data: string) => Promise<void>>(() => Promise.resolve());
    const input = new InputSender(send, systemClock);
    input.push('4');
    input.push('2');
    input.push('\r');
    await vi.advanceTimersByTimeAsync(0);
    expect(send).not.toHaveBeenCalled();
    input.ready();
    await vi.advanceTimersByTimeAsync(0);
    expect(send.mock.calls.map(([data]) => decoder.decode(fromBase64(data)))).toEqual(['42\r']);
  });

  it('sends a long paste in 64 KiB chunks, in order', async () => {
    const { input, sent } = sender();
    const text = 'abc'.repeat(50_000);
    input.push(text);
    await vi.advanceTimersByTimeAsync(0);
    const chunks = sent();
    expect(chunks.map((chunk) => chunk.length)).toEqual([65_536, 65_536, 150_000 - 2 * 65_536]);
    expect(chunks.map((chunk) => decoder.decode(chunk)).join('')).toBe(text);
  });

  it('stays within the bytes a second the backend accepts', async () => {
    const { input, send } = sender();
    input.push('x'.repeat(MAX_PENDING_INPUT_BYTES));
    await vi.advanceTimersByTimeAsync(0);
    const firstSecond = send.mock.calls.length;
    expect(firstSecond * MAX_RUN_INPUT_BYTES).toBeLessThanOrEqual(INPUT_BYTES_PER_SECOND);
    await vi.advanceTimersByTimeAsync(1000);
    expect(send.mock.calls.length).toBe(MAX_PENDING_INPUT_BYTES / MAX_RUN_INPUT_BYTES);
  });

  it('stays within the calls a second the backend accepts', async () => {
    let calls = 0;
    const send = vi.fn<(data: string) => Promise<void>>(() => {
      calls += 1;
      return Promise.resolve();
    });
    const input = new InputSender(send, systemClock);
    input.ready();
    for (let key = 0; key < INPUT_CALLS_PER_SECOND + 50; key++) {
      input.push('k');
      // Each key press is sent before the next one comes.
      await vi.advanceTimersByTimeAsync(0);
    }
    expect(calls).toBe(INPUT_CALLS_PER_SECOND);
    await vi.advanceTimersByTimeAsync(1000);
    const total = send.mock.calls
      .map(([data]) => fromBase64(data).length)
      .reduce((sum, length) => sum + length, 0);
    expect(total).toBe(INPUT_CALLS_PER_SECOND + 50);
  });

  it('tries again later when the backend says rateLimited', async () => {
    const send = vi
      .fn<(data: string) => Promise<void>>(() => Promise.resolve())
      .mockRejectedValueOnce(new IpcCallError('run_input', { code: 'rateLimited' }));
    const { input, sent } = sender(send);
    input.push('guess');
    await vi.advanceTimersByTimeAsync(0);
    expect(send).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(FIRST_RETRY_DELAY_MS);
    expect(sent().map((chunk) => decoder.decode(chunk))).toEqual(['guess', 'guess']);
    expect(input.pendingBytes).toBe(0);
  });

  it('drops everything once the program is no longer running', async () => {
    const send = vi
      .fn<(data: string) => Promise<void>>(() => Promise.resolve())
      .mockRejectedValueOnce(new IpcCallError('run_input', { code: 'notRunning' }));
    const { input } = sender(send);
    input.push('a');
    await vi.advanceTimersByTimeAsync(0);
    input.push('b');
    await vi.advanceTimersByTimeAsync(0);
    expect(send).toHaveBeenCalledTimes(1);
  });

  it('logs other failures and carries on', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const send = vi
      .fn<(data: string) => Promise<void>>(() => Promise.resolve())
      .mockRejectedValueOnce(new IpcCallError('run_input', { code: 'io', kind: 'other' }));
    const { input, sent } = sender(send);
    input.push('a');
    await vi.advanceTimersByTimeAsync(0);
    input.push('b');
    await vi.advanceTimersByTimeAsync(0);
    expect(sent().map((chunk) => decoder.decode(chunk))).toEqual(['a', 'b']);
    expect(warn).toHaveBeenCalledWith('Typed input could not be sent to the program', 'io');
  });

  it('drops typing beyond the bound while the program does not read', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const send = vi.fn(() => new Promise<void>(() => undefined));
    const input = new InputSender(send, systemClock);
    input.push('x'.repeat(MAX_PENDING_INPUT_BYTES));
    input.push('more');
    input.push('and more');
    expect(input.pendingBytes).toBe(MAX_PENDING_INPUT_BYTES);
    expect(warn).toHaveBeenCalledTimes(1);
    input.dispose();
    expect(input.pendingBytes).toBe(0);
    input.push('after');
    expect(input.pendingBytes).toBe(0);
  });
});
