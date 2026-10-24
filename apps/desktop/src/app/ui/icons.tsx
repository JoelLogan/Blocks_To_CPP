import type { ReactNode } from 'react';

/**
 * The shell's few icons, as inline SVG drawn with `currentColor`. They are decorative: every icon
 * sits next to text or inside a control with an accessible name, so colour or shape is never the
 * only signal (docs/spec/04-user-interface.md §4.8).
 */

interface IconProps {
  className?: string;
}

function Svg({ className, children }: IconProps & { children: ReactNode }) {
  return (
    <svg
      className={className === undefined ? 'icon' : `icon ${className}`}
      viewBox="0 0 16 16"
      width="16"
      height="16"
      aria-hidden="true"
      focusable="false"
    >
      {children}
    </svg>
  );
}

/** A padlock: Restricted Mode. */
export function LockIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <path
        d="M5 7V5a3 3 0 0 1 6 0v2M4 7h8v7H4z"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinejoin="round"
      />
    </Svg>
  );
}

/** A chevron pointing `direction`: collapse and expand. */
export function ChevronIcon({ direction, ...props }: IconProps & { direction: Direction }) {
  const paths: Record<Direction, string> = {
    left: 'M10 3 5 8l5 5',
    right: 'M6 3l5 5-5 5',
    up: 'M3 10l5-5 5 5',
    down: 'M3 6l5 5 5-5',
  };
  return (
    <Svg {...props}>
      <path
        d={paths[direction]}
        fill="none"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </Svg>
  );
}

export type Direction = 'left' | 'right' | 'up' | 'down';

/** A gear: Settings. */
export function GearIcon(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="8" cy="8" r="2.25" fill="none" stroke="currentColor" strokeWidth="1.5" />
      <path
        d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
      />
    </Svg>
  );
}
