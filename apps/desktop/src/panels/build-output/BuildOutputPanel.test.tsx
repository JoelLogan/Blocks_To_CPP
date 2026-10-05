import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { expectNoAxeViolations } from '../../test/axe';
import {
  BuildOutputPanel,
  MAX_BUILD_LINE_CHARS,
  MAX_BUILD_OUTPUT_LINES,
  type BuildOutputLine,
} from './BuildOutputPanel';

const LINES: BuildOutputLine[] = [
  { kind: 'progress', text: 'Generating C++' },
  { kind: 'note', text: 'B2C-T1011: AddressSanitizer is not available here; building without it.' },
  { kind: 'progress', text: 'Compiling main.cpp (1/1)' },
  { kind: 'raw', text: "main.cpp:5:3: error: expected ';' before '}' token" },
  { kind: 'raw', text: '<script>alert(1)</script>' },
];

function shownLines(): string[] {
  return screen.getAllByTestId('build-output-line').map((line) => line.textContent);
}

describe('BuildOutputPanel', () => {
  it('shows every line as text, in order, with notes marked', () => {
    render(<BuildOutputPanel lines={LINES} />);
    expect(shownLines()).toEqual([
      'Generating C++',
      'ℹ B2C-T1011: AddressSanitizer is not available here; building without it.',
      'Compiling main.cpp (1/1)',
      "main.cpp:5:3: error: expected ';' before '}' token",
      '<script>alert(1)</script>',
    ]);
    // Markup in the output stays text.
    expect(document.querySelector('script')).toBeNull();
    expect(screen.getByRole('log', { name: 'Build output' })).toBeDefined();
  });

  it('says when there is nothing yet', () => {
    render(<BuildOutputPanel lines={[]} />);
    expect(screen.getByText(/No build output yet/)).toBeDefined();
    expect(screen.queryByRole('log')).toBeNull();
  });

  it('shows the latest lines and how many earlier ones are hidden', () => {
    const many = Array.from({ length: MAX_BUILD_OUTPUT_LINES + 2 }, (_, i) => ({
      kind: 'raw' as const,
      text: `line ${String(i)}`,
    }));
    render(<BuildOutputPanel lines={many} />);
    const shown = shownLines();
    expect(shown).toHaveLength(MAX_BUILD_OUTPUT_LINES);
    expect(shown[0]).toBe('line 2');
    expect(screen.getByText('… 2 earlier lines not shown')).toBeDefined();
  });

  it('makes hidden characters visible and shortens very long lines', () => {
    render(
      <BuildOutputPanel
        lines={[
          { kind: 'raw', text: 'abc‮def' },
          { kind: 'raw', text: 'y'.repeat(MAX_BUILD_LINE_CHARS + 10) },
        ]}
      />,
    );
    const [hiddenChars, long] = shownLines();
    expect(hiddenChars).toBe('abc⟨U+202E⟩def');
    expect(long).toBe(`${'y'.repeat(MAX_BUILD_LINE_CHARS)} … (10 more characters)`);
  });

  it('follows new output only while scrolled to the end', () => {
    const { rerender } = render(<BuildOutputPanel lines={LINES.slice(0, 2)} />);
    const log = screen.getByRole('log');
    let height = 500;
    Object.defineProperty(log, 'scrollHeight', { configurable: true, get: () => height });
    Object.defineProperty(log, 'clientHeight', { configurable: true, get: () => 100 });

    height = 600;
    rerender(<BuildOutputPanel lines={LINES.slice(0, 3)} />);
    expect(log.scrollTop).toBe(600);

    // Scrolled up: new lines do not move the view.
    log.scrollTop = 100;
    fireEvent.scroll(log);
    height = 700;
    rerender(<BuildOutputPanel lines={LINES} />);
    expect(log.scrollTop).toBe(100);
  });

  it('has no accessibility violations', async () => {
    const { container } = render(<BuildOutputPanel lines={LINES} />);
    await expectNoAxeViolations(container);
  });
});
