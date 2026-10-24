import { afterEach, describe, expect, it, vi } from 'vitest';

import { copyPlainText, MAX_COPY_CHARS } from './clipboard';

/** Makes `document.execCommand('copy')` fire a `copy` event and return what a webview would. */
function fakeExecCommand(record: (data: DataTransfer) => void) {
  const execCommand = vi.fn((command: string) => {
    if (command !== 'copy') {
      return false;
    }
    const data = new DataTransfer();
    const event = new ClipboardEvent('copy', { clipboardData: data, cancelable: true });
    document.dispatchEvent(event);
    record(data);
    return true;
  });
  Object.defineProperty(document, 'execCommand', { configurable: true, value: execCommand });
  return execCommand;
}

afterEach(() => {
  Reflect.deleteProperty(document, 'execCommand');
});

describe('copyPlainText', () => {
  it('writes text/plain through the Clipboard API', async () => {
    const writeText = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue();
    await expect(copyPlainText('int main() {}\n')).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith('int main() {}\n');
  });

  it('falls back to a copy event that only it handles when the Clipboard API refuses', async () => {
    vi.spyOn(navigator.clipboard, 'writeText').mockRejectedValue(new Error('NotAllowedError'));
    let copied: string | null = null;
    const execCommand = fakeExecCommand((data) => {
      copied = data.getData('text/plain');
    });
    const otherListener = vi.fn();
    document.addEventListener('copy', otherListener);

    await expect(copyPlainText('return 0;')).resolves.toBe(true);

    expect(execCommand).toHaveBeenCalledWith('copy');
    expect(copied).toBe('return 0;');
    // The editor's own copy handling (blocks) must not see this event.
    expect(otherListener).not.toHaveBeenCalled();
    document.removeEventListener('copy', otherListener);
  });

  it('reports failure when neither route works', async () => {
    vi.spyOn(navigator.clipboard, 'writeText').mockRejectedValue(new Error('NotAllowedError'));
    await expect(copyPlainText('text')).resolves.toBe(false);
  });

  it('refuses text over the limit', async () => {
    const writeText = vi.spyOn(navigator.clipboard, 'writeText').mockResolvedValue();
    await expect(copyPlainText('x'.repeat(MAX_COPY_CHARS + 1))).resolves.toBe(false);
    expect(writeText).not.toHaveBeenCalled();
  });
});
