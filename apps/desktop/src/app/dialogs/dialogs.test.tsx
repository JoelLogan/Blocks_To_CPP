import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import * as Blockly from 'blockly/core';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { expectNoAxeViolations } from '../../test/axe';
import { installBlocklyDialogs } from './blockly';
import { DialogHost } from './DialogHost';
import { createDialogQueue, type DialogQueue, MAX_PENDING_DIALOGS } from './service';

function renderHost(): DialogQueue {
  const queue = createDialogQueue();
  render(<DialogHost queue={queue} />);
  return queue;
}

/**
 * Starts a request inside `act`, so the dialog has rendered when it returns. The answer comes back
 * wrapped, because an async function would otherwise wait for it.
 */
async function open<T>(start: () => Promise<T>): Promise<{ answer: Promise<T> }> {
  let answer!: Promise<T>;
  await act(async () => {
    answer = start();
    await Promise.resolve();
  });
  return { answer };
}

let restoreBlockly: (() => void) | null = null;

afterEach(() => {
  restoreBlockly?.();
  restoreBlockly = null;
});

describe('DialogHost', () => {
  it('shows nothing while no dialog is asked for', () => {
    renderHost();
    expect(screen.queryByRole('dialog')).toBeNull();
  });

  it('shows an alert, focuses OK, and resolves when it is closed', async () => {
    const queue = renderHost();
    const { answer: closed } = await open(() =>
      queue.alert({ title: 'Saved', message: 'Your project was saved.' }),
    );

    const dialog = screen.getByRole('dialog', { name: 'Saved' });
    expect(dialog.textContent).toContain('Your project was saved.');
    const ok = screen.getByRole('button', { name: 'OK' });
    expect(document.activeElement).toBe(ok);
    await expectNoAxeViolations(dialog);

    fireEvent.click(ok);
    await expect(closed).resolves.toBeUndefined();
    await waitFor(() => {
      expect(screen.queryByRole('dialog')).toBeNull();
    });
  });

  it('uses the message as the heading when there is no title', async () => {
    const queue = renderHost();
    await open(() => queue.alert({ message: 'Only a message' }));
    expect(screen.getByRole('dialog', { name: 'Only a message' })).toBeTruthy();
  });

  it('confirms: OK is true, Cancel and Escape are false', async () => {
    const queue = renderHost();

    const { answer: yes } = await open(() =>
      queue.confirm({ title: 'Delete?', message: 'Delete 3 blocks?' }),
    );
    await expectNoAxeViolations(screen.getByRole('dialog'));
    fireEvent.click(screen.getByRole('button', { name: 'OK' }));
    await expect(yes).resolves.toBe(true);

    const { answer: no } = await open(() =>
      queue.confirm({ message: 'Sure?', cancelLabel: 'Keep' }),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Keep' }));
    await expect(no).resolves.toBe(false);

    const { answer: dismissed } = await open(() => queue.confirm({ message: 'Sure?' }));
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    await expect(dismissed).resolves.toBe(false);
  });

  it('focuses Cancel first when the action loses data', async () => {
    const queue = renderHost();
    await open(() =>
      queue.confirm({ message: 'Clear?', confirmLabel: 'Clear', destructive: true }),
    );
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Cancel' }));
  });

  it('prompts with the default selected, validates, and returns the text', async () => {
    const queue = renderHost();
    const { answer: answer } = await open(() =>
      queue.prompt({
        title: 'New variable',
        message: 'Name of the new variable:',
        defaultValue: 'value',
        validate: (text) => (/^[A-Za-z_]\w*$/.test(text) ? null : 'Use letters, digits and _'),
      }),
    );

    const input = screen.getByRole<HTMLInputElement>('textbox', {
      name: 'Name of the new variable:',
    });
    expect(input.value).toBe('value');
    expect(document.activeElement).toBe(input);
    expect([input.selectionStart, input.selectionEnd]).toEqual([0, 5]);
    await expectNoAxeViolations(screen.getByRole('dialog'));

    fireEvent.change(input, { target: { value: '2nd' } });
    expect(screen.getByRole('alert').textContent).toContain('Use letters, digits and _');
    expect(input.getAttribute('aria-invalid')).toBe('true');
    await expectNoAxeViolations(screen.getByRole('dialog'));
    fireEvent.click(screen.getByRole('button', { name: 'OK' }));
    expect(screen.getByRole('dialog')).toBeTruthy();

    fireEvent.change(input, { target: { value: 'guess' } });
    fireEvent.submit(input);
    await expect(answer).resolves.toBe('guess');
  });

  it('returns null when a prompt is cancelled, and bounds its length', async () => {
    const queue = renderHost();
    const { answer: cancelled } = await open(() => queue.prompt({ message: 'Name?' }));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await expect(cancelled).resolves.toBeNull();

    const { answer: bounded } = await open(() =>
      queue.prompt({ message: 'Name?', defaultValue: 'abcdefgh', maxLength: 4 }),
    );
    const input = screen.getByRole<HTMLInputElement>('textbox');
    expect(input.value).toBe('abcd');
    expect(input.maxLength).toBe(4);
    fireEvent.click(screen.getByRole('button', { name: 'OK' }));
    await expect(bounded).resolves.toBe('abcd');
  });

  it('offers several answers, focusing the primary one', async () => {
    const queue = renderHost();
    const { answer: choice } = await open(() =>
      queue.choose({
        title: 'Save changes?',
        message: 'Your project has unsaved changes.',
        choices: [
          { id: 'save', label: 'Save', primary: true },
          { id: 'discard', label: "Don't save", destructive: true },
          { id: 'cancel', label: 'Cancel' },
        ],
        cancel: 'cancel',
      }),
    );

    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Save' }));
    await expectNoAxeViolations(screen.getByRole('dialog'));
    fireEvent.click(screen.getByRole('button', { name: "Don't save" }));
    await expect(choice).resolves.toBe('discard');

    const { answer: dismissed } = await open(() =>
      queue.choose({
        message: 'Reload?',
        choices: [
          { id: 'reload', label: 'Reload' },
          { id: 'keep', label: 'Keep mine' },
        ],
        cancel: 'keep',
      }),
    );
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Reload' }));
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    await expect(dismissed).resolves.toBe('keep');
  });

  it('refuses a choice whose cancel answer is not one of its choices', async () => {
    const queue = createDialogQueue();
    await expect(
      queue.choose({ message: 'x', choices: [{ id: 'a', label: 'A' }], cancel: 'b' as 'a' }),
    ).rejects.toThrow('the cancel answer of a choice must be one of its choices');
  });

  it('shows dialogs one at a time, in order', async () => {
    const queue = renderHost();
    const { answer: first } = await open(() => queue.alert({ message: 'First' }));
    const { answer: second } = await open(() => queue.alert({ message: 'Second' }));

    expect(screen.getAllByRole('dialog')).toHaveLength(1);
    expect(screen.getByRole('dialog', { name: 'First' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'OK' }));
    await first;
    expect(await screen.findByRole('dialog', { name: 'Second' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'OK' }));
    await second;
  });

  it('cancels requests beyond the queue limit, and all of them on cancelAll', async () => {
    const queue = createDialogQueue();
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const waiting = Array.from({ length: MAX_PENDING_DIALOGS }, () =>
      queue.confirm({ message: 'waiting' }),
    );

    await expect(queue.prompt({ message: 'one too many' })).resolves.toBeNull();
    expect(warn).toHaveBeenCalledTimes(1);
    expect(queue.store.getState().queue).toHaveLength(MAX_PENDING_DIALOGS);

    queue.cancelAll();
    await expect(Promise.all(waiting)).resolves.toEqual(Array(MAX_PENDING_DIALOGS).fill(false));
    expect(queue.store.getState().queue).toHaveLength(0);
  });

  it('settles a request only once', async () => {
    const queue = createDialogQueue();
    const answer = queue.confirm({ message: 'once' });
    const [request] = queue.store.getState().queue;
    if (request?.kind !== 'confirm') {
      throw new Error('expected a confirm request');
    }
    request.settle(true);
    request.settle(false);
    await expect(answer).resolves.toBe(true);
  });

  it('calls onOpen with the dialog and its clean-up when it closes', async () => {
    const queue = renderHost();
    const release = vi.fn();
    const onOpen = vi.fn(() => release);
    await open(() => queue.alert({ message: 'hello', onOpen }));

    expect(onOpen).toHaveBeenCalledWith(screen.getByRole('dialog'));
    fireEvent.click(screen.getByRole('button', { name: 'OK' }));
    await waitFor(() => {
      expect(release).toHaveBeenCalledTimes(1);
    });
  });
});

describe('installBlocklyDialogs', () => {
  it("routes Blockly's prompt, confirm and alert through the app's dialogs", async () => {
    const queue = renderHost();
    restoreBlockly = installBlocklyDialogs(queue);

    const promptResult = vi.fn();
    await act(async () => {
      Blockly.dialog.prompt('New variable name:', 'x', promptResult);
      await Promise.resolve();
    });
    const input = screen.getByRole<HTMLInputElement>('textbox', { name: 'New variable name:' });
    expect(Blockly.getFocusManager().ephemeralFocusTaken()).toBe(true);
    fireEvent.change(input, { target: { value: 'total' } });
    fireEvent.submit(input);
    await waitFor(() => {
      expect(promptResult).toHaveBeenCalledWith('total');
    });
    await waitFor(() => {
      expect(Blockly.getFocusManager().ephemeralFocusTaken()).toBe(false);
    });

    const confirmResult = vi.fn();
    await act(async () => {
      Blockly.dialog.confirm('Delete all 4 blocks?', confirmResult);
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole('button', { name: 'OK' }));
    await waitFor(() => {
      expect(confirmResult).toHaveBeenCalledWith(true);
    });

    const alerted = vi.fn();
    await act(async () => {
      Blockly.dialog.alert('Nothing to undo.', alerted);
      await Promise.resolve();
    });
    expect(screen.getByRole('dialog', { name: 'Nothing to undo.' })).toBeTruthy();
    fireEvent.click(screen.getByRole('button', { name: 'OK' }));
    await waitFor(() => {
      expect(alerted).toHaveBeenCalledTimes(1);
    });
  });

  it('works when something else already holds Blockly’s focus', async () => {
    const queue = renderHost();
    restoreBlockly = installBlocklyDialogs(queue);
    const holder = document.createElement('div');
    holder.tabIndex = -1;
    document.body.append(holder);
    const returnFocus = Blockly.getFocusManager().takeEphemeralFocus(holder);

    const answer = vi.fn();
    await act(async () => {
      Blockly.dialog.prompt('Name?', '', answer);
      await Promise.resolve();
    });
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(answer).toHaveBeenCalledWith(null);
    });
    returnFocus();
    holder.remove();
  });

  it('still shows the dialog when Blockly refuses to lend its focus', async () => {
    const queue = renderHost();
    restoreBlockly = installBlocklyDialogs(queue);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    vi.spyOn(Blockly.getFocusManager(), 'takeEphemeralFocus').mockImplementation(() => {
      throw new Error('locked');
    });

    const answer = vi.fn();
    await act(async () => {
      Blockly.dialog.confirm('Go on?', answer);
      await Promise.resolve();
    });
    expect(warn).toHaveBeenCalledWith(
      'Blockly did not lend its focus to the dialog',
      expect.any(Error),
    );
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    await waitFor(() => {
      expect(answer).toHaveBeenCalledWith(false);
    });
  });

  it("uses the app's dialog queue by default", async () => {
    const { dialogs } = await import('./instance');
    restoreBlockly = installBlocklyDialogs();

    Blockly.dialog.confirm('Default queue?', () => undefined);
    expect(dialogs.store.getState().queue).toHaveLength(1);
    dialogs.cancelAll();
  });

  it("restores Blockly's own dialogs when removed", () => {
    const queue = createDialogQueue();
    installBlocklyDialogs(queue)();
    const alert = vi.fn();
    vi.stubGlobal('alert', alert);

    Blockly.dialog.alert('native');
    expect(alert).toHaveBeenCalledWith('native');
    expect(queue.store.getState().queue).toHaveLength(0);
  });
});
