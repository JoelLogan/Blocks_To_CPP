import * as Tabs from '@radix-ui/react-tabs';
import { type ReactNode, useId, useState } from 'react';
import { useShallow } from 'zustand/react/shallow';

import { PROBLEMS_PANEL } from '../actions';
import { DockPanels } from '../panels';
import { type BottomTab, useAppStore } from '../store';
import { problemCount } from '../store/selectors';
import { ChevronIcon } from '../ui/icons';
import { Splitter } from './Splitter';

/** The right dock's width: initial, smallest and largest, in CSS pixels. */
export const RIGHT_DOCK = { initial: 380, min: 200, max: 960 } as const;
/** The bottom dock's height: initial, smallest and largest, in CSS pixels. */
export const BOTTOM_DOCK = { initial: 220, min: 96, max: 720 } as const;

/** The largest share of the window a dock may take, so the workspace always stays visible. */
const MAX_DOCK_SHARE = '65%';

const BOTTOM_TABS: readonly { value: BottomTab; label: string }[] = [
  { value: 'console', label: 'Console' },
  { value: 'problems', label: 'Problems' },
  { value: 'buildOutput', label: 'Build output' },
];

function isBottomTab(value: string): value is BottomTab {
  return BOTTOM_TABS.some((tab) => tab.value === value);
}

/**
 * The editor's part of the window (docs/spec/04-user-interface.md §4.1): the workspace (with
 * Blockly's toolbox on its left) in the centre, the C++ dock on the right and the dock with the
 * console, Problems and the build output at the bottom. Both docks can be resized (by pointer or
 * keyboard) and collapsed; their sizes last for the session (remembering them is M5).
 *
 * The docks' panels stay mounted while they are hidden, so the console keeps its content.
 */
export function EditorLayout({ workspace, hidden }: { workspace: ReactNode; hidden: boolean }) {
  const [rightWidth, setRightWidth] = useState<number>(RIGHT_DOCK.initial);
  const [bottomHeight, setBottomHeight] = useState<number>(BOTTOM_DOCK.initial);
  const { rightCollapsed, bottomCollapsed } = useAppStore(
    useShallow((state) => ({
      rightCollapsed: state.ui.rightCollapsed,
      bottomCollapsed: state.ui.bottomCollapsed,
    })),
  );
  const rightId = useId();
  const bottomId = useId();
  const panels = DockPanels();
  const { setUi } = useAppStore.getState().actions;

  const columns = rightCollapsed
    ? 'minmax(0, 1fr) 0 auto'
    : `minmax(0, 1fr) auto min(${String(rightWidth)}px, ${MAX_DOCK_SHARE})`;
  const rows = bottomCollapsed
    ? 'minmax(0, 1fr) 0 auto'
    : `minmax(0, 1fr) auto min(${String(bottomHeight)}px, ${MAX_DOCK_SHARE})`;

  return (
    <div
      className="editor-layout"
      hidden={hidden}
      style={{ gridTemplateColumns: columns, gridTemplateRows: rows }}
    >
      <main className="workspace" aria-label="Block workspace">
        {workspace}
      </main>

      {!rightCollapsed && (
        <Splitter
          orientation="vertical"
          value={rightWidth}
          min={RIGHT_DOCK.min}
          max={RIGHT_DOCK.max}
          onChange={setRightWidth}
          onCollapse={() => {
            setUi({ rightCollapsed: true });
          }}
          label="Resize the C++ panel"
          controls={rightId}
        />
      )}

      <aside
        id={rightId}
        className="right-dock"
        aria-label="Generated C++"
        data-collapsed={rightCollapsed}
      >
        <div className="dock-header">
          <h2 className="dock-title">C++</h2>
          <button
            type="button"
            className="icon-button"
            aria-expanded={!rightCollapsed}
            aria-controls={`${rightId}-content`}
            aria-label={rightCollapsed ? 'Show the C++ panel' : 'Hide the C++ panel'}
            onClick={() => {
              setUi({ rightCollapsed: !rightCollapsed });
            }}
          >
            <ChevronIcon direction={rightCollapsed ? 'left' : 'right'} />
          </button>
        </div>
        <div id={`${rightId}-content`} className="dock-content" hidden={rightCollapsed}>
          {panels.code}
        </div>
      </aside>

      {!bottomCollapsed && (
        <Splitter
          orientation="horizontal"
          value={bottomHeight}
          min={BOTTOM_DOCK.min}
          max={BOTTOM_DOCK.max}
          onChange={setBottomHeight}
          onCollapse={() => {
            setUi({ bottomCollapsed: true });
          }}
          label="Resize the bottom panel"
          controls={bottomId}
        />
      )}

      <BottomDock id={bottomId} collapsed={bottomCollapsed} panels={panels} />
    </div>
  );
}

/** The bottom dock's tabs: Console, Problems (n) and Build output. */
function BottomDock({
  id,
  collapsed,
  panels,
}: {
  id: string;
  collapsed: boolean;
  panels: ReturnType<typeof DockPanels>;
}) {
  const { tab, problems } = useAppStore(
    useShallow((state) => ({ tab: state.ui.bottomTab, problems: problemCount(state) })),
  );
  const { setUi } = useAppStore.getState().actions;
  const contents: Record<BottomTab, ReactNode> = {
    console: panels.console,
    problems: panels.problems,
    buildOutput: panels.buildOutput,
  };

  return (
    <section
      id={id}
      className="bottom-dock"
      aria-label="Console, problems and build output"
      data-collapsed={collapsed}
    >
      <Tabs.Root
        className="bottom-tabs"
        value={tab}
        onValueChange={(value) => {
          if (isBottomTab(value)) {
            setUi({ bottomTab: value, bottomCollapsed: false });
          }
        }}
      >
        <div className="dock-header">
          <Tabs.List className="dock-tabs" aria-label="Bottom panel">
            {BOTTOM_TABS.map(({ value, label }) => (
              <Tabs.Trigger
                key={value}
                value={value}
                className="dock-tab"
                onClick={() => {
                  // Choosing the shown tab again opens a collapsed dock.
                  if (collapsed) {
                    setUi({ bottomCollapsed: false });
                  }
                }}
              >
                {value === 'problems' ? `${label} (${String(problems)})` : label}
              </Tabs.Trigger>
            ))}
          </Tabs.List>
          <button
            type="button"
            className="icon-button"
            aria-expanded={!collapsed}
            aria-controls={`${id}-content`}
            aria-label={collapsed ? 'Show the bottom panel' : 'Hide the bottom panel'}
            onClick={() => {
              setUi({ bottomCollapsed: !collapsed });
            }}
          >
            <ChevronIcon direction={collapsed ? 'up' : 'down'} />
          </button>
        </div>
        <div id={`${id}-content`} className="dock-content" hidden={collapsed}>
          {BOTTOM_TABS.map(({ value }) => (
            <Tabs.Content
              key={value}
              value={value}
              forceMount
              hidden={tab !== value}
              className="dock-panel"
              data-dock-panel={value === 'problems' ? PROBLEMS_PANEL : value}
            >
              {contents[value]}
            </Tabs.Content>
          ))}
        </div>
      </Tabs.Root>
    </section>
  );
}
