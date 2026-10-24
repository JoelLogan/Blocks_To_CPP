/**
 * The clipboard's two types in a `DataTransfer` (05 §5.12), and the in-app copy: what is written,
 * what is read back (only the Blocks2Cpp type), and engines that refuse a type.
 */
import { describe, expect, it, vi } from 'vitest';

import {
  CLIPBOARD_MIME,
  type ClipboardData,
  MAX_PAYLOAD_CHARS,
  PLAIN_TEXT_MIME,
  readTransfer,
  writeTransfer,
} from './formats';
import { ClipboardMemory } from './memory';

const DATA: ClipboardData = {
  payload: '{"format": "blocks2cpp/clipboard"}',
  text: 'std::cout << "hi" << std::endl;\n',
};

/** A transfer whose `setData` refuses the types in `refused`, as some engines do. */
function refusingTransfer(refused: readonly string[]): DataTransfer {
  const transfer = new DataTransfer();
  const setData = transfer.setData.bind(transfer);
  vi.spyOn(transfer, 'setData').mockImplementation((type: string, value: string) => {
    if (refused.includes(type)) {
      throw new DOMException('not allowed', 'NotAllowedError');
    }
    setData(type, value);
  });
  return transfer;
}

/** A transfer whose `getData(type)` answers `read(type)`. */
function readingTransfer(read: (type: string) => string): DataTransfer {
  const transfer = new DataTransfer();
  vi.spyOn(transfer, 'getData').mockImplementation(read);
  return transfer;
}

describe('writeTransfer', () => {
  it('writes the payload as the Blocks2Cpp type and the C++ as text', () => {
    const transfer = new DataTransfer();
    expect(writeTransfer(transfer, DATA)).toBe(true);
    expect(transfer.getData(CLIPBOARD_MIME)).toBe(DATA.payload);
    expect(transfer.getData(PLAIN_TEXT_MIME)).toBe(DATA.text);
  });

  it('writes no text when the blocks give no C++', () => {
    const transfer = new DataTransfer();
    expect(writeTransfer(transfer, { payload: DATA.payload, text: null })).toBe(true);
    expect([...transfer.types]).toEqual([CLIPBOARD_MIME]);
  });

  it('still writes the C++ when the engine refuses the Blocks2Cpp type', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const transfer = refusingTransfer([CLIPBOARD_MIME]);
    expect(writeTransfer(transfer, DATA)).toBe(false);
    expect(transfer.getData(PLAIN_TEXT_MIME)).toBe(DATA.text);
    expect(warn).toHaveBeenCalledOnce();
  });

  it('keeps the payload when the engine refuses the text', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const transfer = refusingTransfer([PLAIN_TEXT_MIME]);
    expect(writeTransfer(transfer, DATA)).toBe(true);
    expect(transfer.getData(CLIPBOARD_MIME)).toBe(DATA.payload);
    expect(warn).toHaveBeenCalledOnce();
  });
});

describe('readTransfer', () => {
  it('reads the Blocks2Cpp type and nothing else', () => {
    const transfer = new DataTransfer();
    transfer.setData(CLIPBOARD_MIME, DATA.payload);
    transfer.setData(PLAIN_TEXT_MIME, 'int main() {}');
    expect(readTransfer(transfer)).toBe(DATA.payload);
  });

  it('finds no payload in plain text, an empty transfer or no transfer', () => {
    const text = new DataTransfer();
    text.setData(PLAIN_TEXT_MIME, '{"format": "blocks2cpp/clipboard"}');
    expect(readTransfer(text)).toBeNull();
    expect(readTransfer(new DataTransfer())).toBeNull();
    expect(readTransfer(null)).toBeNull();
  });

  it('finds no payload when the engine refuses to read the type', () => {
    const transfer = readingTransfer(() => {
      throw new DOMException('not allowed', 'NotAllowedError');
    });
    expect(readTransfer(transfer)).toBeNull();
  });

  it('cuts an oversized payload to one unit over the limit, which the core refuses', () => {
    const huge = 'x'.repeat(MAX_PAYLOAD_CHARS + 1000);
    const read = readTransfer(readingTransfer(() => huge));
    expect(read?.length).toBe(MAX_PAYLOAD_CHARS);
  });

  it('keeps a payload at the limit whole', () => {
    const atLimit = 'y'.repeat(MAX_PAYLOAD_CHARS);
    expect(readTransfer(readingTransfer(() => atLimit))).toBe(atLimit);
  });
});

describe('ClipboardMemory', () => {
  it('keeps the last copy until it is cleared', () => {
    const memory = new ClipboardMemory();
    expect(memory.get()).toBeNull();
    memory.set(DATA);
    memory.set({ payload: 'second', text: null });
    expect(memory.get()).toEqual({ payload: 'second', text: null });
    memory.clear();
    expect(memory.get()).toBeNull();
  });

  it('keeps its own copy of the data', () => {
    const memory = new ClipboardMemory();
    const data = { payload: 'p', text: 't' };
    memory.set(data);
    data.payload = 'changed';
    expect(memory.get()).toEqual({ payload: 'p', text: 't' });
  });
});
