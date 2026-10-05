import { useLayoutEffect, useRef } from 'react';

import '../panels.css';
import { visibleInvisibles } from '../shared/invisibles';

/** One line of build output. */
export interface BuildOutputLine {
  /** Build progress, a toolchain note (such as B2C-T1011), or the compiler's own text. */
  kind: 'progress' | 'note' | 'raw';
  text: string;
}

/** The props of {@link BuildOutputPanel}. */
export interface BuildOutputPanelProps {
  lines: readonly BuildOutputLine[];
}

/** The most lines shown: the latest ones, with a note saying how many earlier ones are hidden. */
export const MAX_BUILD_OUTPUT_LINES = 5000;

/** The most characters shown of one line. */
export const MAX_BUILD_LINE_CHARS = 16 * 1024;

/** How close to the end (in pixels) still counts as "at the end" for following new output. */
const FOLLOW_SLACK_PX = 8;

/**
 * The Build output tab (docs/spec/04-user-interface.md §4.1, 07 §7.5.3): build progress, toolchain
 * notes and the compiler's raw text, as plain text. It follows new output while the person is at
 * the end, and stays put once they scroll up.
 */
export function BuildOutputPanel({ lines }: BuildOutputPanelProps) {
  const log = useRef<HTMLDivElement>(null);
  const follow = useRef(true);

  const hidden = Math.max(0, lines.length - MAX_BUILD_OUTPUT_LINES);
  const shown = hidden > 0 ? lines.slice(hidden) : lines;

  useLayoutEffect(() => {
    const element = log.current;
    if (element !== null && follow.current) {
      element.scrollTop = element.scrollHeight;
    }
  }, [lines]);

  if (lines.length === 0) {
    return (
      <div className="b2c-panel b2c-build-output-panel" data-testid="build-output-panel">
        <p className="b2c-panel-empty">
          No build output yet. Building shows its progress and the compiler&apos;s messages here.
        </p>
      </div>
    );
  }

  return (
    <div className="b2c-panel b2c-build-output-panel" data-testid="build-output-panel">
      <div
        ref={log}
        className="b2c-build-log"
        role="log"
        aria-label="Build output"
        // A scrollable log must be reachable with the keyboard to be scrolled (WCAG 2.1.1).
        // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
        tabIndex={0}
        onScroll={(event) => {
          const element = event.currentTarget;
          follow.current =
            element.scrollTop + element.clientHeight >= element.scrollHeight - FOLLOW_SLACK_PX;
        }}
      >
        {hidden > 0 && (
          <div className="b2c-build-line b2c-build-line-progress">
            … {hidden.toLocaleString('en-US')} earlier {hidden === 1 ? 'line' : 'lines'} not shown
          </div>
        )}
        {shown.map((line, index) => (
          <div
            key={hidden + index}
            className={`b2c-build-line b2c-build-line-${line.kind}`}
            data-testid="build-output-line"
          >
            {line.kind === 'note' && <span aria-hidden="true">ℹ </span>}
            {lineForDisplay(line.text)}
          </div>
        ))}
      </div>
    </div>
  );
}

/** A line as shown: hidden characters made visible, very long lines shortened. */
function lineForDisplay(text: string): string {
  const visible = visibleInvisibles(text);
  if (visible.length <= MAX_BUILD_LINE_CHARS) {
    return visible;
  }
  const rest = visible.length - MAX_BUILD_LINE_CHARS;
  return `${visible.slice(0, MAX_BUILD_LINE_CHARS)} … (${rest.toLocaleString('en-US')} more characters)`;
}
