/**
 * The dock panels belong to the shell, which never loads Blockly (it is the editor's, loaded with
 * the workspace; see editor/diagnostics/catalog.ts). Here loading Blockly or blockly-ext fails, so
 * an import that would pull either into the shell fails this test.
 */
import { render, screen, within } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

vi.mock('blockly/core', () => {
  throw new Error('the dock panels loaded Blockly');
});
vi.mock('@blocks2cpp/blockly-ext', () => {
  throw new Error('the dock panels loaded blockly-ext');
});

describe('the dock panels', () => {
  it('load and list problems without Blockly, naming blocks by type', async () => {
    const { DockPanels } = await import('./panels');
    const { useAppStore } = await import('./store');
    const { diagnosticFixture, documentFixture, previewFixture, projectFixture } =
      await import('./testing/fixtures');

    const document = documentFixture();
    document.modules = [
      {
        id: 'mod_main',
        name: 'main',
        workspace: {
          blocks: [
            {
              id: 'b001',
              type: 'program.main',
              v: 1,
              statements: { BODY: [{ id: 'b007', type: 'io.print', v: 1 }] },
            },
          ],
        },
      },
    ];
    const { actions } = useAppStore.getState();
    actions.setProject(projectFixture({ document }));
    actions.setAnalysis({ preview: previewFixture([diagnosticFixture()]) });

    const panels = DockPanels();
    render(<div>{panels.problems}</div>);
    const grid = screen.getByRole('grid', { name: 'Problems' });
    const [, row] = within(grid).getAllByRole('row');
    expect(row?.textContent).toContain('main › io.print');
  });
});
