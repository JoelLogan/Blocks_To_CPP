/**
 * How much of the window renders again per preview run (docs/plans/m2-polish-plan.md P-2), counted
 * with React's Profiler: hidden panels (Problems behind the Console tab, the C++ tab in a collapsed
 * dock) do nothing until they are shown and then catch up once, and the status bar, the module
 * switcher and the Restricted Mode banner render only when what they show changes. A hidden panel
 * does not carry what it held of one project into the next.
 */
import type { BdmDocument, PreviewResult } from '@blocks2cpp/b2c-core-wasm';
import { EditorView } from '@codemirror/view';
import { act, render, screen } from '@testing-library/react';
import { Profiler, type ReactNode } from 'react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createStore } from 'zustand/vanilla';

import { buildProblemItems } from '../editor/diagnostics/problemItems';
import { ModuleSwitcher } from '../editor/modules/ModuleSwitcher';
import type { TrustController } from '../features/trust/controller';
import { RestrictedModeBanner } from '../features/trust/RestrictedModeBanner';
import { StatusBar } from './layout/StatusBar';
import { ConnectedCodePanel, ConnectedProblemsPanel } from './panels';
import { resetAppStore, useAppStore } from './store';
import {
  diagnosticFixture,
  documentFixture,
  previewFixture,
  projectFixture,
} from './testing/fixtures';

vi.mock('../editor/diagnostics/problemItems', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../editor/diagnostics/problemItems')>();
  return { ...actual, buildProblemItems: vi.fn(actual.buildProblemItems) };
});

/** The components counted, by Profiler ID. */
type Counted = 'code' | 'problems' | 'status' | 'modules' | 'banner';

/** A document with two modules (so the module switcher shows), new on every call as an edit's is. */
function twoModules(helpers = 'helpers'): BdmDocument {
  const doc = documentFixture();
  doc.modules = [
    { id: 'mod_main', name: 'main', workspace: { blocks: [] } },
    { id: 'mod_helpers', name: helpers, workspace: { blocks: [] } },
  ];
  return doc;
}

/** The preview of edit `n`: one error and the code `// edit n`. */
function previewOf(n: number): PreviewResult {
  return {
    ...previewFixture([diagnosticFixture({ message: `Problem of edit ${String(n)}.` })]),
    files: [{ path: 'main.cpp', kind: 'source', contents: `// edit ${String(n)}\n` }],
  };
}

/** One run of the preview pipeline as the store sees it: the edit, then its preview. */
function edit(n: number, doc: BdmDocument = twoModules()): void {
  act(() => {
    const { actions } = useAppStore.getState();
    actions.updateProjectAndAnalysis(
      { document: doc, canonicalText: `{"edit":${String(n)}}`, contentHash: String(n).repeat(64) },
      { seq: n },
    );
    actions.setAnalysis({ preview: previewOf(n) });
  });
}

/** The trust feature's controller, as far as the banner reads it. */
function bannerController(): TrustController {
  return {
    state: createStore(() => ({ busy: null, message: null })),
  } as unknown as TrustController;
}

/** Renders the counted components, each in its own Profiler, and returns the live counts. */
function renderCounted(): Record<Counted, number> {
  const counts: Record<Counted, number> = {
    code: 0,
    problems: 0,
    status: 0,
    modules: 0,
    banner: 0,
  };
  const counted = (id: Counted, children: ReactNode) => (
    <Profiler
      id={id}
      onRender={() => {
        counts[id] += 1;
      }}
    >
      {children}
    </Profiler>
  );
  render(
    <>
      {counted('code', <ConnectedCodePanel />)}
      {counted('problems', <ConnectedProblemsPanel />)}
      {counted('status', <StatusBar />)}
      {counted('modules', <ModuleSwitcher />)}
      {counted('banner', <RestrictedModeBanner controller={bannerController()} />)}
    </>,
  );
  return counts;
}

function reset(counts: Record<Counted, number>): void {
  for (const key of Object.keys(counts) as Counted[]) {
    counts[key] = 0;
  }
  vi.mocked(buildProblemItems).mockClear();
}

beforeEach(() => {
  resetAppStore();
  const { actions } = useAppStore.getState();
  actions.setProject(projectFixture({ document: twoModules(), dirty: true }));
  actions.setAnalysis({ preview: previewOf(0) });
  // The Console tab is chosen and the C++ dock is collapsed: neither panel is on screen.
  actions.setUi({ bottomTab: 'console', rightCollapsed: true });
});

describe('render counts per preview run', () => {
  it('are zero for hidden panels and for chrome whose text does not change', () => {
    const counts = renderCounted();
    reset(counts);
    for (let n = 1; n <= 10; n += 1) {
      edit(n);
    }
    expect(counts).toEqual({ code: 0, problems: 0, status: 0, modules: 0, banner: 0 });
    expect(buildProblemItems).not.toHaveBeenCalled();
  });

  it('catch up once when Problems is shown, and follow each run while it is', () => {
    const counts = renderCounted();
    for (let n = 1; n <= 10; n += 1) {
      edit(n);
    }
    reset(counts);
    act(() => {
      useAppStore.getState().actions.setUi({ bottomTab: 'problems' });
    });
    expect(counts.problems).toBe(1);
    expect(buildProblemItems).toHaveBeenCalledTimes(1);
    expect(screen.getByText('Problem of edit 10.')).toBeDefined();

    reset(counts);
    edit(11);
    expect(counts.problems).toBe(1);
    expect(buildProblemItems).toHaveBeenCalledTimes(1);
    expect(screen.getByText('Problem of edit 11.')).toBeDefined();

    // A collapsed dock hides it as well.
    act(() => {
      useAppStore.getState().actions.setUi({ bottomCollapsed: true });
    });
    reset(counts);
    edit(12);
    expect(counts.problems).toBe(0);
    expect(buildProblemItems).not.toHaveBeenCalled();
  });

  it('catch up once when the C++ dock opens, with the latest code', () => {
    // Shown once first: the code view's first update after it was created renders it twice.
    act(() => {
      useAppStore.getState().actions.setUi({ rightCollapsed: false });
    });
    const counts = renderCounted();
    edit(1);
    act(() => {
      useAppStore.getState().actions.setUi({ rightCollapsed: true });
    });
    reset(counts);
    for (let n = 2; n <= 10; n += 1) {
      edit(n);
    }
    expect(counts.code).toBe(0);
    expect(screen.getByTestId('code-panel').textContent).toContain('// edit 1');
    reset(counts);
    act(() => {
      useAppStore.getState().actions.setUi({ rightCollapsed: false });
    });
    expect(counts.code).toBe(1);
    expect(screen.getByTestId('code-panel').textContent).toContain('// edit 10');

    reset(counts);
    edit(11);
    expect(counts.code).toBe(1);
    expect(screen.getByTestId('code-panel').textContent).toContain('// edit 11');
  });

  it('follow the chrome’s own texts: a renamed module, a save, a restricted project', () => {
    const counts = renderCounted();
    reset(counts);
    edit(1, twoModules('tools'));
    expect(counts.modules).toBe(1);
    expect(screen.getByRole('button', { name: 'tools.cpp' })).toBeDefined();

    act(() => {
      useAppStore.getState().actions.updateProject({ dirty: false, fileName: 'game.b2c' });
    });
    expect(counts.status).toBe(1);
    expect(screen.getByTestId('status-save').textContent).toMatch(/^Saved/);

    act(() => {
      useAppStore.getState().actions.updateProject({
        trust: {
          state: 'restricted',
          source: null,
          restrictedReason: 'noRecord',
          markOfTheWeb: false,
        },
      });
    });
    expect(counts.banner).toBe(1);
    expect(screen.getByTestId('restricted-banner')).toBeDefined();
  });
});

/** The code view in the C++ tab. */
function codeView(): EditorView {
  const editor = screen.getByTestId('code-panel').querySelector('.cm-editor');
  const view = editor instanceof HTMLElement ? EditorView.findFromDOM(editor) : null;
  if (view === null) {
    throw new Error('The C++ tab shows no code view');
  }
  return view;
}

/** The C++ tab's *Copy selection* button. */
function copySelection(): HTMLButtonElement {
  return screen.getByRole<HTMLButtonElement>('button', { name: 'Copy selection' });
}

/** A project other than the fixture's (another handle), with its own code. */
function openOtherProject(): void {
  const { actions } = useAppStore.getState();
  actions.setProject(projectFixture({ handle: `ph_${'b'.repeat(32)}`, fileName: 'other.b2c' }));
  actions.setAnalysis({
    preview: {
      ...previewFixture([]),
      files: [{ path: 'main.cpp', kind: 'source', contents: '// the other project\n'.repeat(50) }],
    },
  });
}

describe('a project change while a panel is hidden', () => {
  it('gives the C++ tab a new code view, without the old project’s selection', () => {
    act(() => {
      useAppStore.getState().actions.setUi({ rightCollapsed: false });
    });
    renderCounted();
    edit(1);
    const panel = screen.getByTestId('code-panel');
    act(() => {
      codeView().dispatch({ selection: { anchor: 0, head: 8 } });
    });
    expect(copySelection().disabled).toBe(false);
    act(() => {
      useAppStore.getState().actions.setUi({ rightCollapsed: true });
    });

    act(() => {
      openOtherProject();
    });
    act(() => {
      useAppStore.getState().actions.setUi({ rightCollapsed: false });
    });
    expect(screen.getByTestId('code-panel')).not.toBe(panel);
    expect(codeView().state.doc.toString()).toContain('// the other project');
    expect(codeView().state.selection.main.empty).toBe(true);
    expect(copySelection().disabled).toBe(true);
  });

  it('gives the Problems tab new rows when the project is closed and another opened', () => {
    act(() => {
      useAppStore.getState().actions.setUi({ bottomTab: 'problems' });
    });
    renderCounted();
    const panel = screen.getByTestId('problems-panel');
    act(() => {
      useAppStore.getState().actions.setUi({ bottomTab: 'console' });
    });

    act(() => {
      useAppStore.getState().actions.setProject(null);
    });
    act(() => {
      openOtherProject();
    });
    act(() => {
      useAppStore.getState().actions.setUi({ bottomTab: 'problems' });
    });
    expect(screen.getByTestId('problems-panel')).not.toBe(panel);
  });

  it('still keeps what it held while the same project is edited', () => {
    const counts = renderCounted();
    reset(counts);
    act(() => {
      useAppStore.getState().actions.updateProject({ dirty: false, fileName: 'saved.b2c' });
    });
    edit(1);
    expect(counts.code).toBe(0);
    expect(counts.problems).toBe(0);
  });
});
