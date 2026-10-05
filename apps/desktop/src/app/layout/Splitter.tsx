import { type KeyboardEvent, type PointerEvent, useRef } from 'react';

/** How far one arrow-key press moves a splitter, in CSS pixels (with Shift: {@link BIG_STEP}). */
export const STEP = 16;
export const BIG_STEP = 64;

export interface SplitterProps {
  /**
   * `vertical`: a bar between two columns that changes the width of the panel after it.
   * `horizontal`: a bar between two rows that changes the height of the panel below it.
   */
  orientation: 'vertical' | 'horizontal';
  /** The panel's current size in CSS pixels. */
  value: number;
  min: number;
  max: number;
  /** Called with the new size, already clamped to `min`–`max`. */
  onChange: (value: number) => void;
  /** Called on Enter: collapses the panel (the window-splitter pattern of WAI-ARIA). */
  onCollapse: () => void;
  /** The accessible name, such as "Resize the C++ panel". */
  label: string;
  /** The ID of the panel it resizes. */
  controls: string;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, Math.round(value)));
}

/**
 * A bar that resizes the panel after it, by pointer and by keyboard (WAI-ARIA window splitter):
 * arrow keys move it by {@link STEP} pixels (Shift: {@link BIG_STEP}), Home and End go to the
 * smallest and largest size, and Enter collapses the panel.
 */
export function Splitter({
  orientation,
  value,
  min,
  max,
  onChange,
  onCollapse,
  label,
  controls,
}: SplitterProps) {
  /** Where a pointer drag started: the pointer position and the size then. */
  const drag = useRef<{ start: number; size: number } | null>(null);
  const vertical = orientation === 'vertical';

  const onKeyDown = (event: KeyboardEvent) => {
    const step = event.shiftKey ? BIG_STEP : STEP;
    // The panel is after the bar: moving the bar towards it (right or down) makes it smaller.
    const grow = vertical ? 'ArrowLeft' : 'ArrowUp';
    const shrink = vertical ? 'ArrowRight' : 'ArrowDown';
    let next: number;
    switch (event.key) {
      case grow:
        next = value + step;
        break;
      case shrink:
        next = value - step;
        break;
      case 'Home':
        next = min;
        break;
      case 'End':
        next = max;
        break;
      case 'Enter':
        event.preventDefault();
        onCollapse();
        return;
      default:
        return;
    }
    event.preventDefault();
    onChange(clamp(next, min, max));
  };

  const position = (event: PointerEvent) => (vertical ? event.clientX : event.clientY);

  // A focusable separator is the WAI-ARIA window-splitter widget; jsx-a11y counts every separator
  // as static content.
  /* eslint-disable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex */
  return (
    <div
      role="separator"
      tabIndex={0}
      className={`splitter splitter-${orientation}`}
      aria-orientation={orientation}
      aria-label={label}
      aria-controls={controls}
      aria-valuenow={value}
      aria-valuemin={min}
      aria-valuemax={max}
      onKeyDown={onKeyDown}
      onPointerDown={(event) => {
        if (event.button !== 0) {
          return;
        }
        event.preventDefault();
        event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = { start: position(event), size: value };
      }}
      onPointerMove={(event) => {
        const current = drag.current;
        if (current === null) {
          return;
        }
        onChange(clamp(current.size - (position(event) - current.start), min, max));
      }}
      onPointerUp={(event) => {
        drag.current = null;
        if (event.currentTarget.hasPointerCapture(event.pointerId)) {
          event.currentTarget.releasePointerCapture(event.pointerId);
        }
      }}
      onPointerCancel={() => {
        drag.current = null;
      }}
    />
  );
  /* eslint-enable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex */
}
