/**
 * The hints on the toolbar, canvas and title controls (docs/spec/04-user-interface.md §4.8): a hint
 * shows on keyboard focus and hides on Escape, and a screen reader hears the control's description
 * once, whether or not the hint is shown.
 */
import { act, fireEvent, render, screen } from '@testing-library/react';
import { useId } from 'react';
import { describe, expect, it } from 'vitest';

import { expectNoAxeViolations } from '../../test/axe';
import { Hint, HintProvider } from './Hint';

const HINT = 'Build (Ctrl+B)';

/** A control as the toolbar renders one: its own description says what its hint says. */
function HintedButton() {
  const descriptionId = useId();
  return (
    <HintProvider>
      <div>
        <Hint text={HINT}>
          <button type="button" aria-describedby={descriptionId}>
            Build
          </button>
        </Hint>
        <span id={descriptionId} className="visually-hidden">
          {HINT}
        </span>
      </div>
    </HintProvider>
  );
}

describe('a hint', () => {
  it('leaves the control described once, by the same text, while it is shown', async () => {
    const { container } = render(<HintedButton />);
    const button = screen.getByRole('button', { name: 'Build' });
    const ownDescription = button.getAttribute('aria-describedby');
    expect(ownDescription).not.toBeNull();
    expect(screen.getByRole('button', { name: 'Build', description: HINT })).toBe(button);

    act(() => {
      button.focus();
    });
    const tooltip = await screen.findByRole('tooltip');
    expect(tooltip.textContent).toBe(HINT);
    // Radix also describes a control by its open tooltip; the text is not read twice.
    expect(screen.getByRole('button', { name: 'Build', description: HINT })).toBe(button);
    await expectNoAxeViolations(container);

    fireEvent.keyDown(button, { key: 'Escape' });
    expect(screen.queryByRole('tooltip')).toBeNull();
    expect(button.getAttribute('aria-describedby')).toBe(ownDescription);
    expect(screen.getByRole('button', { name: 'Build', description: HINT })).toBe(button);
  });
});
