/**
 * The canvas controls (docs/spec/04-user-interface.md §4.1, §4.8; decision D10 of the M2 polish
 * plan): *Zoom out*, *Zoom in*, *Reset zoom* (which shows the zoom now and goes back to 100 %) and
 * *Show all blocks* (which scrolls to the blocks and keeps the zoom), as named HTML buttons at the
 * top right of the visible canvas. They replace Blockly's SVG zoom controls, which had no names and
 * no keyboard access.
 *
 * The editor renders them into the workspace's container right after Blockly's injection `div`,
 * so they come right after the canvas in the Tab order, and Blockly's pixel coordinates are theirs
 * (./placement.ts keeps them in place). Each button has a hint with its key, and the zoom keys
 * work on the controls too (Blockly hears them only inside its injection `div`, which the controls
 * are beside); what a button or key did is announced in a polite live region. Pressing one with
 * the pointer leaves the keyboard focus where it was (on the canvas, typically), as Blockly's own
 * controls did.
 */
import * as Blockly from 'blockly/core';
import {
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';

import { Hint, HintProvider } from '../../app/ui/Hint';
import { attachControlsPlacement } from './placement';
import { resetZoom, showAllBlocks, zoomAnnouncement, zoomPercent, zoomStep } from './zoom';
import './CanvasControls.css';

/** The zoom keys on the canvas (../keyboard/shortcuts.ts), as hints write them and for `aria-keyshortcuts`. */
export const ZOOM_KEYS = {
  out: { label: 'Ctrl+-', aria: 'Control+-' },
  in: { label: 'Ctrl+=', aria: 'Control+=' },
  reset: { label: 'Ctrl+0', aria: 'Control+0' },
} as const;

/** A zoom key's action: one step in or out, or back to 100 %. */
export type ZoomKey = 'in' | 'out' | 'reset';

/**
 * The zoom key `event` presses, if any: the keys of the canvas (../keyboard/shortcuts.ts), `Ctrl`
 * with `=` or `+` (main or numeric keypad), `-` and `0`, without `Alt` or `Meta`.
 */
export function zoomKeyOf(
  event: Pick<globalThis.KeyboardEvent, 'key' | 'code' | 'ctrlKey' | 'altKey' | 'metaKey'>,
): ZoomKey | null {
  if (!event.ctrlKey || event.altKey || event.metaKey) {
    return null;
  }
  if (event.key === '=' || event.key === '+' || event.code === 'NumpadAdd') {
    return 'in';
  }
  if (event.key === '-' || event.code === 'NumpadSubtract') {
    return 'out';
  }
  if (event.key === '0' || event.code === 'Numpad0') {
    return 'reset';
  }
  return null;
}

/** What *Show all blocks* announces: it scrolled, or there was nothing to scroll to. */
export const SHOW_ALL_ANNOUNCEMENTS = {
  shown: 'Scrolled to the blocks',
  empty: 'The canvas has no blocks',
} as const;

/** Keeps the keyboard focus where it was when a control is pressed with the pointer. */
function keepFocus(event: MouseEvent): void {
  event.preventDefault();
}

/** One control: a button with an accessible name, a hint (also its description) and a test ID. */
function Control({
  testId,
  name,
  hint,
  keys,
  onPress,
  onKeyDown,
  children,
}: {
  testId: string;
  name: string;
  hint: string;
  keys?: string;
  onPress: () => void;
  onKeyDown: (event: KeyboardEvent) => void;
  children: ReactNode;
}) {
  const descriptionId = useId();
  return (
    <>
      <Hint text={hint}>
        <button
          type="button"
          className="canvas-control"
          data-testid={testId}
          aria-label={name}
          aria-describedby={descriptionId}
          aria-keyshortcuts={keys}
          onMouseDown={keepFocus}
          onClick={onPress}
          onKeyDown={onKeyDown}
        >
          {children}
        </button>
      </Hint>
      <span id={descriptionId} className="visually-hidden">
        {hint}
      </span>
    </>
  );
}

/**
 * The canvas controls of `workspace`. The editor (../EditorWorkspace.tsx) renders them into the
 * workspace's container once Blockly is injected, and stops rendering them when it disposes of the
 * workspace; their clean-up only removes what they added to the workspace, so it may come after.
 */
export function CanvasControls({ workspace }: { workspace: Blockly.WorkspaceSvg }) {
  const group = useRef<HTMLDivElement>(null);
  const [scale, setScale] = useState(() => workspace.scale);
  // The text of the live region; a trailing no-break space on every other message makes the same
  // text twice in a row a change, so it is announced again.
  const [status, setStatus] = useState({ text: '', toggle: false });

  // Placed before the first paint, so the controls never show anywhere else.
  useLayoutEffect(() => {
    const element = group.current;
    return element === null ? undefined : attachControlsPlacement(workspace, element);
  }, [workspace]);

  // The zoom shown follows every zoom: the buttons, the keys, Ctrl + wheel, a pinch and a project's
  // saved view (each fires Blockly's viewport event).
  useEffect(() => {
    const follow = (event: Blockly.Events.Abstract): void => {
      if (event instanceof Blockly.Events.ViewportChange) {
        setScale(workspace.scale);
      }
    };
    setScale(workspace.scale);
    workspace.addChangeListener(follow);
    return () => {
      workspace.removeChangeListener(follow);
    };
  }, [workspace]);

  const announce = (text: string): void => {
    setStatus((previous) => ({ text, toggle: !previous.toggle }));
  };
  const zoomed = (now: number): void => {
    setScale(now);
    announce(zoomAnnouncement(now));
  };
  // The zoom keys the hints and `aria-keyshortcuts` name, also while a control has the focus.
  const onKeyDown = (event: KeyboardEvent): void => {
    const key = zoomKeyOf(event);
    if (key === null) {
      return;
    }
    event.preventDefault();
    zoomed(key === 'reset' ? resetZoom(workspace) : zoomStep(workspace, key));
  };
  const percent = zoomPercent(scale);

  return (
    <HintProvider>
      <div
        ref={group}
        className="canvas-controls"
        role="group"
        aria-label="Canvas zoom"
        data-testid="canvas-controls"
      >
        <Control
          testId="canvas-zoom-out"
          onKeyDown={onKeyDown}
          name="Zoom out"
          hint={`Zoom out (${ZOOM_KEYS.out.label})`}
          keys={ZOOM_KEYS.out.aria}
          onPress={() => {
            zoomed(zoomStep(workspace, 'out'));
          }}
        >
          <span aria-hidden="true">−</span>
        </Control>
        <Control
          testId="canvas-zoom-in"
          onKeyDown={onKeyDown}
          name="Zoom in"
          hint={`Zoom in (${ZOOM_KEYS.in.label})`}
          keys={ZOOM_KEYS.in.aria}
          onPress={() => {
            zoomed(zoomStep(workspace, 'in'));
          }}
        >
          <span aria-hidden="true">+</span>
        </Control>
        <Control
          testId="canvas-zoom-reset"
          onKeyDown={onKeyDown}
          name={`Reset zoom (now ${percent})`}
          hint={`Reset the zoom to 100% (${ZOOM_KEYS.reset.label})`}
          keys={ZOOM_KEYS.reset.aria}
          onPress={() => {
            zoomed(resetZoom(workspace));
          }}
        >
          {percent}
        </Control>
        <Control
          testId="canvas-show-all"
          onKeyDown={onKeyDown}
          name="Show all blocks"
          hint="Show all blocks: scroll to them, keeping the zoom"
          onPress={() => {
            announce(
              showAllBlocks(workspace)
                ? SHOW_ALL_ANNOUNCEMENTS.shown
                : SHOW_ALL_ANNOUNCEMENTS.empty,
            );
          }}
        >
          Show all
        </Control>
        <span className="visually-hidden" role="status" aria-live="polite" aria-atomic="true">
          {status.text === '' ? '' : status.toggle ? status.text : `${status.text}\u00a0`}
        </span>
      </div>
    </HintProvider>
  );
}
