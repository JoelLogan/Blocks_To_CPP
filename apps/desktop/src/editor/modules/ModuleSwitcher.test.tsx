/** The module switcher: shown for several modules, it only changes the active module. */
import { act, fireEvent, render, screen } from '@testing-library/react';
import { beforeEach, describe, expect, it } from 'vitest';

import { resetAppStore, useAppStore } from '../../app/store';
import { documentFixture, projectFixture } from '../../app/testing/fixtures';
import { expectNoAxeViolations } from '../../test/axe';
import { ModuleSwitcher } from './ModuleSwitcher';

function openWith(names: string[]): void {
  const doc = documentFixture();
  doc.modules = names.map((name) => ({ id: `mod_${name}`, name, workspace: { blocks: [] } }));
  useAppStore
    .getState()
    .actions.setProject(projectFixture({ document: doc, activeModuleId: `mod_${names[0] ?? ''}` }));
}

beforeEach(() => {
  resetAppStore();
});

describe('the module switcher', () => {
  it('is not shown without a project or with a single module', () => {
    const { container, rerender } = render(<ModuleSwitcher />);
    expect(container.childElementCount).toBe(0);
    act(() => {
      openWith(['main']);
    });
    rerender(<ModuleSwitcher />);
    expect(container.childElementCount).toBe(0);
  });

  it('lists the modules as files and switches the active one', async () => {
    openWith(['main', 'shapes', 'io']);
    render(<ModuleSwitcher />);
    const nav = screen.getByRole('navigation', { name: 'Modules' });
    const buttons = screen.getAllByRole('button');
    expect(buttons.map((button) => button.textContent)).toEqual([
      'main.cpp',
      'shapes.cpp',
      'io.cpp',
    ]);
    expect(buttons.map((button) => button.getAttribute('aria-pressed'))).toEqual([
      'true',
      'false',
      'false',
    ]);
    await expectNoAxeViolations(nav);

    fireEvent.click(screen.getByRole('button', { name: 'shapes.cpp' }));
    expect(useAppStore.getState().project?.activeModuleId).toBe('mod_shapes');
    expect(screen.getByRole('button', { name: 'shapes.cpp' }).getAttribute('aria-pressed')).toBe(
      'true',
    );
    // Pressing the shown module changes nothing.
    const before = useAppStore.getState().project;
    fireEvent.click(screen.getByRole('button', { name: 'shapes.cpp' }));
    expect(useAppStore.getState().project).toBe(before);
  });
});
