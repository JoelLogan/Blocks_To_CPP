import { type CSSProperties, type KeyboardEvent, type PointerEvent, useRef } from 'react';

/** How far one arrow-key press moves a splitter, in CSS pixels (with Shift: {@link BIG_STEP}). */
export const STEP = 16;
export const BIG_STEP = 64;

export interface SplitterProps {
  /**
   * `vertical`: a bar between two columns that changes the width of a panel beside it.
   * `horizontal`: a bar between two rows that changes the height of a panel above or below it.
   */
  orientation: 'vertical' | 'horizontal';
  /**
   * Which side of the bar the panel it resizes is on: `after` (right of a vertical bar, below a
   * horizontal one; the default) or `before` (left, or above). Moving the bar away from the panel
   * makes it larger.
   */
  side?: 'before' | 'after';
  /** The panel's current size in CSS pixels. */
  value: number;
  min: number;
  max: number;
  /** Called with the new size, already clamped to `min`–`max`. */
  onChange: (value: number) => void;
  /**
   * Called on Enter: collapses the panel (the window-splitter pattern of WAI-ARIA). If that removes
   * the splitter, the caller moves the keyboard focus to the control that restores the panel.
   * Without it, Enter does nothing (a panel that cannot be collapsed).
   */
  onCollapse?: () => void;
  /** The accessible name, such as "Resize the C++ panel". */
  label: string;
  /** The ID of the panel it resizes, when it has one. */
  controls?: string;
  /** More classes, after `splitter splitter-<orientation>`. */
  className?: string;
  /** Inline style, such as the position of a bar laid over its panel's edge. */
  style?: CSSProperties;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, Math.round(value)));
}

/**
 * A bar that resizes the panel beside it, by pointer and by keyboard (WAI-ARIA window splitter):
 * arrow keys move it by {@link STEP} pixels (Shift: {@link BIG_STEP}), Home and End go to the
 * smallest and largest size, and Enter collapses the panel when it can be collapsed.
 */
export function Splitter({
  orientation,
  side = 'after',
  value,
  min,
  max,
  onChange,
  onCollapse,
  label,
  controls,
  className,
  style,
}: SplitterProps) {
  /** Where a pointer drag started: the pointer position and the size then. */
  const drag = useRef<{ start: number; size: number } | null>(null);
  const vertical = orientation === 'vertical';
  /** +1 when moving the bar right or down makes the panel larger (it is before the bar). */
  const direction = side === 'before' ? 1 : -1;

  const onKeyDown = (event: KeyboardEvent) => {
    const step = event.shiftKey ? BIG_STEP : STEP;
    // Right or down moves the bar towards a panel after it, and away from one before it.
    const forward = vertical ? 'ArrowRight' : 'ArrowDown';
    const backward = vertical ? 'ArrowLeft' : 'ArrowUp';
    let next: number;
    switch (event.key) {
      case forward:
        next = value + direction * step;
        break;
      case backward:
        next = value - direction * step;
        break;
      case 'Home':
        next = min;
        break;
      case 'End':
        next = max;
        break;
      case 'Enter':
        if (onCollapse === undefined) {
          return;
        }
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
      className={`splitter splitter-${orientation}${className === undefined ? '' : ` ${className}`}`}
      style={style}
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
        onChange(clamp(current.size + direction * (position(event) - current.start), min, max));
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
