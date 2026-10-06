/**
 * The DOM side of the clipboard: armed copies and pastes from the keys, copy, cut and paste events
 * aimed at the canvas, events aimed elsewhere (left alone), the one-off copy of the menus, and the
 * fallback to the in-app copy when no event comes.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { ClipboardBridge, type ClipboardBridgeOptions } from './bridge';
import { CLIPBOARD_MIME, type ClipboardData, PLAIN_TEXT_MIME } from './formats';

const DATA: ClipboardData = { payload: '{"format": "blocks2cpp/clipboard"}', text: 'f();\n' };

/** A scheduler whose tasks run only when the test says so. */
function manualTasks() {
  const tasks = new Set<() => void>();
  return {
    schedule: (run: () => void) => {
      tasks.add(run);
      return () => {
        tasks.delete(run);
      };
    },
    /** Runs the tasks scheduled so far ("the current task has ended"). */
    run() {
      const due = [...tasks];
      tasks.clear();
      for (const task of due) {
        task();
      }
    },
    get pending() {
      return tasks.size;
    },
  };
}

/** A clipboard event as the webview fires it, with a fresh `DataTransfer`. */
function clipboardEvent(type: 'copy' | 'cut' | 'paste', transfer: DataTransfer | null = null) {
  return new ClipboardEvent(type, {
    clipboardData: transfer ?? new DataTransfer(),
    bubbles: true,
    cancelable: true,
  });
}

/** A paste event whose clipboard holds `payload` as the Blocks2Cpp type. */
function pasteEvent(payload: string) {
  const transfer = new DataTransfer();
  transfer.setData(CLIPBOARD_MIME, payload);
  transfer.setData(PLAIN_TEXT_MIME, 'some text');
  return clipboardEvent('paste', transfer);
}

let canvas: HTMLElement;
let block: HTMLElement;
let field: HTMLInputElement;
let outside: HTMLElement;
let bridge: ClipboardBridge;
let tasks: ReturnType<typeof manualTasks>;
let copyFocused: ReturnType<typeof vi.fn<ClipboardBridgeOptions['copyFocused']>>;
let pasteFocused: ReturnType<typeof vi.fn<ClipboardBridgeOptions['pasteFocused']>>;

beforeEach(() => {
  canvas = document.createElement('div');
  block = document.createElement('div');
  block.tabIndex = 0;
  field = document.createElement('input');
  canvas.append(block, field);
  outside = document.createElement('button');
  document.body.append(canvas, outside);
  tasks = manualTasks();
  copyFocused = vi.fn<ClipboardBridgeOptions['copyFocused']>(() => DATA);
  pasteFocused = vi.fn<ClipboardBridgeOptions['pasteFocused']>();
  bridge = new ClipboardBridge({
    document,
    container: () => canvas,
    copyFocused,
    pasteFocused,
    schedule: tasks.schedule,
  });
});

afterEach(() => {
  bridge.dispose();
  Reflect.deleteProperty(document, 'execCommand');
  document.body.replaceChildren();
});

describe('a copy armed by a key', () => {
  it('goes into the copy event of the same task, which is cancelled', () => {
    bridge.armWrite(DATA);
    expect(bridge.armed).toBe(true);
    const event = clipboardEvent('copy');
    outside.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
    expect(event.clipboardData?.getData(CLIPBOARD_MIME)).toBe(DATA.payload);
    expect(event.clipboardData?.getData(PLAIN_TEXT_MIME)).toBe(DATA.text);
    expect(copyFocused).not.toHaveBeenCalled();
    expect(bridge.armed).toBe(false);
  });

  it('goes into a cut event too', () => {
    bridge.armWrite(DATA);
    const event = clipboardEvent('cut');
    block.dispatchEvent(event);
    expect(event.clipboardData?.getData(CLIPBOARD_MIME)).toBe(DATA.payload);
    expect(copyFocused).not.toHaveBeenCalled();
  });

  it('keeps other listeners from seeing the event', () => {
    const other = vi.fn();
    document.body.addEventListener('copy', other);
    bridge.armWrite(DATA);
    block.dispatchEvent(clipboardEvent('copy'));
    expect(other).not.toHaveBeenCalled();
  });

  it('cancels beforecopy and beforecut, which enables the command in WebKit', () => {
    const idle = new Event('beforecopy', { cancelable: true });
    document.dispatchEvent(idle);
    expect(idle.defaultPrevented).toBe(false);
    bridge.armWrite(DATA);
    const before = new Event('beforecopy', { cancelable: true });
    document.dispatchEvent(before);
    expect(before.defaultPrevented).toBe(true);
    const beforeCut = new Event('beforecut', { cancelable: true });
    document.dispatchEvent(beforeCut);
    expect(beforeCut.defaultPrevented).toBe(true);
  });

  it('is forgotten when the task ends without an event', () => {
    bridge.armWrite(DATA);
    tasks.run();
    expect(bridge.armed).toBe(false);
    const event = clipboardEvent('copy');
    outside.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    expect(event.clipboardData?.types).toEqual([]);
  });

  it('is replaced by a later one', () => {
    bridge.armWrite({ payload: 'first', text: null });
    bridge.armWrite(DATA);
    expect(tasks.pending).toBe(1);
    const event = clipboardEvent('copy');
    outside.dispatchEvent(event);
    expect(event.clipboardData?.getData(CLIPBOARD_MIME)).toBe(DATA.payload);
  });

  it('leaves an event without a DataTransfer alone', () => {
    bridge.armWrite(DATA);
    const event = new ClipboardEvent('copy', { bubbles: true, cancelable: true });
    outside.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
  });
});

describe('a paste armed by a key', () => {
  it('pastes the payload of the paste event of the same task', () => {
    const run = vi.fn();
    bridge.armPaste(run);
    const event = pasteEvent('{"blocks": []}');
    block.dispatchEvent(event);
    expect(run).toHaveBeenCalledExactlyOnceWith('{"blocks": []}');
    expect(event.defaultPrevented).toBe(true);
    expect(pasteFocused).not.toHaveBeenCalled();
    // The fallback is cancelled.
    tasks.run();
    expect(run).toHaveBeenCalledOnce();
  });

  it('pastes the in-app copy when the event holds no Blocks2Cpp data', () => {
    const run = vi.fn();
    bridge.armPaste(run);
    const transfer = new DataTransfer();
    transfer.setData(PLAIN_TEXT_MIME, 'int x;');
    block.dispatchEvent(clipboardEvent('paste', transfer));
    expect(run).toHaveBeenCalledExactlyOnceWith(null);
  });

  it('pastes the in-app copy after the task when no event comes', () => {
    const run = vi.fn();
    bridge.armPaste(run);
    expect(run).not.toHaveBeenCalled();
    const before = new Event('beforepaste', { cancelable: true });
    document.dispatchEvent(before);
    expect(before.defaultPrevented).toBe(true);
    tasks.run();
    expect(run).toHaveBeenCalledExactlyOnceWith(null);
    expect(bridge.armed).toBe(false);
  });

  it('runs a waiting paste first when another key arms one', () => {
    const order: string[] = [];
    bridge.armPaste((payload) => order.push(`first ${String(payload)}`));
    bridge.armPaste((payload) => order.push(`second ${String(payload)}`));
    expect(order).toEqual(['first null']);
    block.dispatchEvent(pasteEvent('P'));
    expect(order).toEqual(['first null', 'second P']);
  });
});

describe('events no key armed', () => {
  it('copy what has the focus when aimed at the canvas', () => {
    const event = clipboardEvent('copy');
    block.dispatchEvent(event);
    expect(copyFocused).toHaveBeenCalledExactlyOnceWith(false);
    expect(event.defaultPrevented).toBe(true);
    expect(event.clipboardData?.getData(CLIPBOARD_MIME)).toBe(DATA.payload);
  });

  it('cut what has the focus when aimed at the canvas', () => {
    block.dispatchEvent(clipboardEvent('cut'));
    expect(copyFocused).toHaveBeenCalledExactlyOnceWith(true);
  });

  it('follow the focus when the engine aims them at the body', () => {
    block.focus();
    const event = clipboardEvent('copy');
    document.body.dispatchEvent(event);
    expect(copyFocused).toHaveBeenCalledOnce();
    outside.focus();
    document.body.dispatchEvent(clipboardEvent('copy'));
    expect(copyFocused).toHaveBeenCalledOnce();
  });

  it('are left alone when nothing could be copied', () => {
    copyFocused.mockReturnValue(null);
    const event = clipboardEvent('copy');
    block.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
  });

  it('paste the event payload at the focus when aimed at the canvas', () => {
    const event = pasteEvent('P');
    block.dispatchEvent(event);
    expect(pasteFocused).toHaveBeenCalledExactlyOnceWith('P');
    expect(event.defaultPrevented).toBe(true);
  });

  it('are left alone when aimed at a text field, even one on the canvas', () => {
    for (const target of [field, outside]) {
      const copy = clipboardEvent('copy');
      const paste = pasteEvent('P');
      target.dispatchEvent(copy);
      target.dispatchEvent(paste);
      expect(copy.defaultPrevented).toBe(false);
      expect(paste.defaultPrevented).toBe(false);
    }
    expect(copyFocused).not.toHaveBeenCalled();
    expect(pasteFocused).not.toHaveBeenCalled();
  });

  it('are left alone when another handler has already taken them', () => {
    const takeIt = (event: Event) => {
      event.preventDefault();
    };
    window.addEventListener('copy', takeIt, { capture: true });
    window.addEventListener('paste', takeIt, { capture: true });
    try {
      block.dispatchEvent(clipboardEvent('copy'));
      block.dispatchEvent(pasteEvent('P'));
    } finally {
      window.removeEventListener('copy', takeIt, { capture: true });
      window.removeEventListener('paste', takeIt, { capture: true });
    }
    expect(copyFocused).not.toHaveBeenCalled();
    expect(pasteFocused).not.toHaveBeenCalled();
  });

  it('are left alone while there is no canvas', () => {
    bridge.dispose();
    bridge = new ClipboardBridge({
      document,
      container: () => null,
      copyFocused,
      pasteFocused,
      schedule: tasks.schedule,
    });
    block.dispatchEvent(clipboardEvent('copy'));
    expect(copyFocused).not.toHaveBeenCalled();
  });

  it('are not cancelled before they come (beforecopy, beforepaste)', () => {
    const before = new Event('beforepaste', { cancelable: true });
    block.dispatchEvent(before);
    expect(before.defaultPrevented).toBe(false);
  });
});

describe('writeNow (menus and commands)', () => {
  /** Makes `document.execCommand('copy')` fire a copy event as a webview would. */
  function fakeExecCommand(accept = true) {
    const transfer = new DataTransfer();
    const execCommand = vi.fn((command: string) => {
      if (command !== 'copy') {
        return false;
      }
      document.body.dispatchEvent(clipboardEvent('copy', transfer));
      return accept;
    });
    Object.defineProperty(document, 'execCommand', { configurable: true, value: execCommand });
    return { execCommand, transfer };
  }

  it('writes the data through a one-off copy event', () => {
    const { execCommand, transfer } = fakeExecCommand();
    expect(bridge.writeNow(DATA)).toBe(true);
    expect(execCommand).toHaveBeenCalledExactlyOnceWith('copy');
    expect(transfer.getData(CLIPBOARD_MIME)).toBe(DATA.payload);
    expect(transfer.getData(PLAIN_TEXT_MIME)).toBe(DATA.text);
    expect(copyFocused).not.toHaveBeenCalled();
    expect(bridge.armed).toBe(false);
  });

  it('reports a copy the webview refused', () => {
    fakeExecCommand(false);
    expect(bridge.writeNow(DATA)).toBe(false);
    expect(bridge.armed).toBe(false);
  });

  it('reports false without execCommand, or when it throws', () => {
    expect(bridge.writeNow(DATA)).toBe(false);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    Object.defineProperty(document, 'execCommand', {
      configurable: true,
      value: () => {
        throw new Error('denied');
      },
    });
    expect(bridge.writeNow(DATA)).toBe(false);
    expect(warn).toHaveBeenCalledOnce();
    expect(bridge.armed).toBe(false);
  });
});

describe('dispose', () => {
  it('stops listening and drops a waiting paste', () => {
    const run = vi.fn();
    bridge.armPaste(run);
    bridge.armWrite(DATA);
    bridge.dispose();
    bridge.dispose();
    expect(bridge.armed).toBe(false);
    tasks.run();
    expect(run).not.toHaveBeenCalled();
    const event = clipboardEvent('copy');
    block.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    expect(copyFocused).not.toHaveBeenCalled();
    bridge.armWrite(DATA);
    bridge.armPaste(run);
    expect(bridge.armed).toBe(false);
    expect(bridge.writeNow(DATA)).toBe(false);
  });
});
