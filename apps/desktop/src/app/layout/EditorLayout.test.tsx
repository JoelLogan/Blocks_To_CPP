import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';

import { resetAppStore, useAppStore } from '../store';
import { HintProvider } from '../ui/Hint';
import { EditorLayout } from './EditorLayout';

beforeEach(() => {
  resetAppStore();
});

function renderLayout() {
  return render(
    <HintProvider>
      <EditorLayout workspace={<div />} hidden={false} />
    </HintProvider>,
  );
}

describe('EditorLayout keyboard focus', () => {
  it.each([
    {
      dock: 'bottom',
      splitter: 'Resize the bottom panel',
      show: 'Show the bottom panel',
      collapsed: () => useAppStore.getState().ui.bottomCollapsed,
    },
    {
      dock: 'right',
      splitter: 'Resize the C++ panel',
      show: 'Show the C++ panel',
      collapsed: () => useAppStore.getState().ui.rightCollapsed,
    },
  ])(
    'moves the focus to the $dock dock’s toggle when Enter on its splitter collapses it',
    ({ splitter, show, collapsed }) => {
      renderLayout();
      const separator = screen.getByRole('separator', { name: splitter });
      act(() => {
        separator.focus();
      });
      expect(document.activeElement).toBe(separator);

      fireEvent.keyDown(separator, { key: 'Enter' });
      expect(collapsed()).toBe(true);
      expect(screen.queryByRole('separator', { name: splitter })).toBeNull();
      // The splitter is gone; the focus is on the button that brings the dock back, never on body.
      const toggle = screen.getByRole('button', { name: show });
      expect(document.activeElement).toBe(toggle);
      expect(toggle.getAttribute('aria-expanded')).toBe('false');

      fireEvent.click(toggle);
      expect(collapsed()).toBe(false);
      expect(screen.getByRole('separator', { name: splitter })).toBeTruthy();
    },
  );
});
