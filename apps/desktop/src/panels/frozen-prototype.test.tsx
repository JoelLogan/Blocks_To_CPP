/**
 * The app runs with Tauri's `freezePrototype: true` (docs/spec/08-security.md §8.8): every library
 * must work with a frozen `Object.prototype`. This file freezes it before loading the panels (and
 * CodeMirror, xterm.js and Radix with them) and renders each panel. Vitest runs every test file
 * in its own module graph, so the freeze stays in this file.
 */
import { act, fireEvent, render, screen } from '@testing-library/react';
import { createRef } from 'react';
import { describe, expect, it } from 'vitest';

import type { ConsoleHandle } from './index';

Object.freeze(Object.prototype);

const panels = await import('./index');
const { LinkDialog } = await import('./console/LinkDialog');

describe('panels with a frozen Object.prototype', () => {
  it('still has the prototype frozen', () => {
    expect(Object.isFrozen(Object.prototype)).toBe(true);
  });

  it('renders the code panel with highlighting, markers and placeholders', () => {
    const { container } = render(
      <panels.CodePanel
        files={[
          { path: 'main.cpp', kind: 'source', contents: 'int main() {\n    return 0;​\n}\n' },
        ]}
        activePath={null}
        onActivePathChange={() => undefined}
        sourceMap={{
          version: 1,
          files: [
            {
              path: 'main.cpp',
              ranges: [
                {
                  start: { line: 2, column: 5 },
                  end: { line: 2, column: 14 },
                  module: 'mod_main',
                  block: 'b1',
                  part: { kind: 'whole' },
                },
              ],
            },
          ],
        }}
        buildable={false}
        diagnostics={[
          {
            code: 'B2C-E0201',
            severity: 'error',
            message: 'Problem',
            primary: { block: 'b1', part: { kind: 'whole' } },
            source: 'analyser',
          },
        ]}
        highlightBlockId="b1"
        onSelectBlock={() => undefined}
      />,
    );
    expect(container.querySelector('.cm-b2c-gutter-marker')).not.toBeNull();
    expect(container.querySelector('.cm-b2c-hidden-char')?.textContent).toBe('⟨U+200B⟩');
  });

  it('runs the console', async () => {
    const ref = createRef<ConsoleHandle>();
    render(
      <panels.ConsolePanel
        ref={ref}
        header={{ state: 'running', exit: null, elapsedMs: 0, notices: [] }}
        scrollbackLines={1000}
        onStop={() => undefined}
        onRunAgain={() => undefined}
        onClear={() => undefined}
      />,
    );
    await act(() => ref.current?.write(new TextEncoder().encode('hello\r\n')) ?? Promise.resolve());
    expect(screen.getByTestId('console-state').textContent).toBe('▶ Running');
  });

  it('opens the link dialog (Radix)', () => {
    render(<LinkDialog url="https://example.com/" onClose={() => undefined} />);
    expect(screen.getByRole('dialog', { name: 'Link from your program' })).toBeDefined();
  });

  it('renders Problems and Build output', () => {
    render(
      <panels.ProblemsPanel
        items={[
          {
            key: 'k',
            diagnostic: {
              code: 'B2C-C1001',
              severity: 'error',
              message: 'expected ;',
              primary: { part: { kind: 'whole' } },
              source: 'compiler',
              raw: 'main.cpp:1:1: error',
            },
            origin: 'build',
            stale: false,
            modulePath: 'main',
            blockPath: 'main',
          },
        ]}
        onActivate={() => undefined}
        onShowRaw={() => undefined}
        onLearnMore={() => undefined}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Show C++ compiler message' }));
    expect(screen.getByText('main.cpp:1:1: error')).toBeDefined();
    render(<panels.BuildOutputPanel lines={[{ kind: 'raw', text: 'done' }]} />);
    expect(screen.getByText('done')).toBeDefined();
  });
});
