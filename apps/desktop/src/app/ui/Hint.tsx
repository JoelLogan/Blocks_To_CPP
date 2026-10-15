import * as Tooltip from '@radix-ui/react-tooltip';
import { cloneElement, type ReactElement, useState } from 'react';

/** How long the pointer rests on a control before its hint shows, in milliseconds. */
export const HINT_DELAY_MS = 500;

/**
 * The provider every {@link Hint} needs; the app mounts it once at the root, tests around what
 * they render.
 */
export function HintProvider({ children }: { children: ReactElement | ReactElement[] }) {
  return <Tooltip.Provider delayDuration={HINT_DELAY_MS}>{children}</Tooltip.Provider>;
}

/**
 * A tooltip for a control: shown on hover and at once on keyboard focus, hidden on Escape. The
 * control keeps its own accessible description (`aria-describedby`), which says what the hint says;
 * the hint only shows it. Radix also describes the control by the open tooltip (since
 * `@radix-ui/react-tooltip` 1.3), which would read the same text twice, so while the hint is shown
 * the control is described by the hint alone.
 */
export function Hint({
  text,
  children,
}: {
  text: string;
  children: ReactElement<{ 'aria-describedby'?: string | undefined }>;
}) {
  const [open, setOpen] = useState(false);
  return (
    <Tooltip.Root open={open} onOpenChange={setOpen}>
      <Tooltip.Trigger asChild>
        {open ? cloneElement(children, { 'aria-describedby': undefined }) : children}
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Content className="hint" side="bottom" sideOffset={6} collisionPadding={8}>
          {text}
          <Tooltip.Arrow className="hint-arrow" />
        </Tooltip.Content>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
}
