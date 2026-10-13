/**
 * The splitter (WAI-ARIA window splitter): which way the keys and the pointer resize the panel
 * depends on the side of the bar the panel is on, Enter collapses only a panel that can be
 * collapsed, and which changes end a series (settled).
 */
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { STEP, Splitter, type SplitterProps } from './Splitter';

function renderSplitter(props: Partial<SplitterProps> = {}) {
  const onChange = vi.fn();
  render(
    <Splitter
      orientation="vertical"
      value={300}
      min={100}
      max={500}
      onChange={onChange}
      label="Resize"
      {...props}
    />,
  );
  const bar = screen.getByRole('separator', { name: 'Resize' });
  bar.setPointerCapture = vi.fn();
  bar.hasPointerCapture = vi.fn(() => true);
  bar.releasePointerCapture = vi.fn();
  return { bar, onChange };
}

describe('the splitter', () => {
  it.each([
    { orientation: 'vertical', side: 'after', grow: 'ArrowLeft', shrink: 'ArrowRight' },
    { orientation: 'vertical', side: 'before', grow: 'ArrowRight', shrink: 'ArrowLeft' },
    { orientation: 'horizontal', side: 'after', grow: 'ArrowUp', shrink: 'ArrowDown' },
    { orientation: 'horizontal', side: 'before', grow: 'ArrowDown', shrink: 'ArrowUp' },
  ] as const)(
    'grows a panel $side a $orientation bar with $grow',
    ({ orientation, side, grow, shrink }) => {
      const { bar, onChange } = renderSplitter({ orientation, side });
      fireEvent.keyDown(bar, { key: grow });
      expect(onChange).toHaveBeenLastCalledWith(300 + STEP, false);
      fireEvent.keyDown(bar, { key: shrink });
      expect(onChange).toHaveBeenLastCalledWith(300 - STEP, false);
      fireEvent.keyDown(bar, { key: 'Home' });
      expect(onChange).toHaveBeenLastCalledWith(100, true);
      fireEvent.keyDown(bar, { key: 'End' });
      expect(onChange).toHaveBeenLastCalledWith(500, true);

      // Moving the pointer away from the panel grows it.
      const axis = orientation === 'vertical' ? 'clientX' : 'clientY';
      const away = side === 'before' ? 1 : -1;
      fireEvent.pointerDown(bar, { button: 0, pointerId: 1, [axis]: 200 });
      fireEvent.pointerMove(bar, { pointerId: 1, [axis]: 200 + away * 40 });
      expect(onChange).toHaveBeenLastCalledWith(340, false);
      fireEvent.pointerMove(bar, { pointerId: 1, [axis]: 200 - away * 1000 });
      expect(onChange).toHaveBeenLastCalledWith(100, false);
      // Letting go ends the drag at the size it reached, settled.
      fireEvent.pointerUp(bar, { pointerId: 1 });
      expect(onChange).toHaveBeenLastCalledWith(100, true);
    },
  );

  it('reports a drag that ends as settled only when the bar moved', () => {
    const { bar, onChange } = renderSplitter();
    // A press without a move is no change.
    fireEvent.pointerDown(bar, { button: 0, pointerId: 1, clientX: 200 });
    fireEvent.pointerUp(bar, { pointerId: 1, clientX: 200 });
    expect(onChange).not.toHaveBeenCalled();

    // A drag the system cancels ends at the size it reached, too.
    fireEvent.pointerDown(bar, { button: 0, pointerId: 2, clientX: 200 });
    fireEvent.pointerMove(bar, { pointerId: 2, clientX: 180 });
    expect(onChange).toHaveBeenLastCalledWith(320, false);
    fireEvent.pointerCancel(bar, { pointerId: 2 });
    expect(onChange).toHaveBeenLastCalledWith(320, true);
    expect(onChange).toHaveBeenCalledTimes(2);
  });

  it('collapses its panel with Enter only when the panel can be collapsed', () => {
    const onCollapse = vi.fn();
    const collapsible = renderSplitter({ onCollapse });
    fireEvent.keyDown(collapsible.bar, { key: 'Enter' });
    expect(onCollapse).toHaveBeenCalledTimes(1);
  });

  it('lets Enter through when its panel cannot be collapsed', () => {
    const { bar, onChange } = renderSplitter();
    const event = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true });
    bar.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    expect(onChange).not.toHaveBeenCalled();
  });

  it('takes extra classes and an inline style, and names the panel only when there is one', () => {
    const { bar } = renderSplitter({ className: 'splitter-toolbox', style: { left: 420 } });
    expect(bar.className).toBe('splitter splitter-vertical splitter-toolbox');
    expect(bar.style.left).toBe('420px');
    expect(bar.hasAttribute('aria-controls')).toBe(false);
  });
});
