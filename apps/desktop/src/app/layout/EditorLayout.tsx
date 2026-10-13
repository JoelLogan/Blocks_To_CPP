import type { LayoutPatch, LayoutSettings } from '@blocks2cpp/ipc-types';
import * as Tabs from '@radix-ui/react-tabs';
import {
  type ReactNode,
  type Ref,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
} from 'react';
import { flushSync } from 'react-dom';
import { useShallow } from 'zustand/react/shallow';

import { PROBLEMS_PANEL } from '../actions';
import { DockPanels } from '../panels';
import { type BottomTab, useAppStore } from '../store';
import { problemCount } from '../store/selectors';
import { ChevronIcon } from '../ui/icons';
import { type FlyoutLayout, listenToFlyoutLayout } from './flyoutLayout';
import { focusIfLost } from './focus';
import { rememberLayout } from './rememberedLayout';
import {
  BOTTOM_DOCK,
  clampSize,
  dockMaximum,
  MAX_DOCK_SHARE,
  RIGHT_DOCK,
  rightDockMaximum,
  startSize,
} from './sizes';
import { Splitter } from './Splitter';
import './EditorLayout.css';

/** A width and a height, in CSS pixels. */
interface Size {
  readonly width: number;
  readonly height: number;
}

/** The window's inner size. */
function windowSize(): Size {
  return { width: window.innerWidth, height: window.innerHeight };
}

/**
 * The toolbox's flyout as it last announced itself, with where the *Resize the toolbox* splitter
 * goes in the workspace region: at the flyout's right edge, as high as the canvas.
 */
interface ToolboxEdge {
  readonly layout: FlyoutLayout;
  readonly left: number;
  readonly top: number;
  readonly bottom: number;
}

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
 * keyboard) and collapsed, and so can the toolbox's flyout be resized, with the *Resize the
 * toolbox* splitter at its right edge.
 *
 * The sizes the user gives the toolbox and the docks, and whether each dock is hidden, are
 * remembered on this computer (05 §5.9 `ui.layout`, through ./rememberedLayout.ts): they are taken
 * from the settings once, when the settings are first in the store, and each change is handed on.
 *
 * The docks start at a share of the window and follow it until the user resizes them; each is
 * kept within the limits of ./sizes.ts as the window changes, and the C++ dock is hidden when even
 * its narrowest width would leave the canvas less than 200 px beside the narrowest flyout (the
 * flyout narrows first; it is shown again when the window has room for it, unless the user showed
 * or hid it meanwhile).
 *
 * The docks' panels stay mounted while they are hidden, so the console keeps its content.
 * Collapsing a dock with Enter on its splitter removes the splitter, so the keyboard focus moves to
 * the dock's show/hide button, which stays and expands it again.
 */
export function EditorLayout({ workspace, hidden }: { workspace: ReactNode; hidden: boolean }) {
  const layoutElement = useRef<HTMLDivElement>(null);
  const workspaceRegion = useRef<HTMLElement>(null);
  const wasHidden = useRef(hidden);
  // Shown again (a page closed, such as the start page's "Back to the project"): the control that
  // had the focus is gone with the page, so the focus goes to the workspace region rather than the
  // top of the window.
  useEffect(() => {
    if (wasHidden.current && !hidden) {
      focusIfLost(workspaceRegion.current);
    }
    wasHidden.current = hidden;
  }, [hidden]);
  /** The editor area's size as last measured, or `null` before it was laid out. */
  const [area, setArea] = useState<Size | null>(null);
  /** The window's size when the editor area was last measured (the docks' start sizes). */
  const [windowExtent, setWindowExtent] = useState<Size>(windowSize);
  /** The sizes the user gave the docks, or `null` while they keep their start sizes. */
  const [rightChosen, setRightChosen] = useState<number | null>(null);
  const [bottomChosen, setBottomChosen] = useState<number | null>(null);
  const [toolbox, setToolbox] = useState<ToolboxEdge | null>(null);
  /** The width the user gave the toolbox's flyout, or `null` while it keeps its default width. */
  const [toolboxChosen, setToolboxChosen] = useState<number | null>(null);
  /** The flyout and the width it was last given, so that each flyout is given a width once. */
  const toolboxGiven = useRef<{ flyout: FlyoutLayout['setWidth']; width: number } | null>(null);
  /** Whether the C++ dock was hidden because it did not fit (and the user has not touched it). */
  const rightHiddenToFit = useRef(false);
  /**
   * Whether each dock is hidden as last remembered (`settings.json`, or the change last handed on),
   * or `null` before the remembered layout was taken from the settings: until then nothing is
   * handed on, so the defaults never replace what was remembered.
   */
  const rememberedHidden = useRef<{ right: boolean; bottom: boolean } | null>(null);

  // Measured before the first paint, and before the editor's own effects lay the canvas out.
  useLayoutEffect(() => {
    const element = layoutElement.current;
    if (element === null) {
      return;
    }
    const observer = new ResizeObserver((entries) => {
      const box = entries.at(-1)?.contentRect;
      if (box === undefined || box.width <= 0 || box.height <= 0) {
        // Hidden (a page replaces the editor): the sizes it had still hold.
        return;
      }
      setArea({ width: box.width, height: box.height });
      setWindowExtent(windowSize());
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, []);

  // The flyout announces itself from inside the workspace region, also while the editor attaches
  // to the canvas (after this effect, which is why it is a layout effect).
  useLayoutEffect(() => {
    const region = workspaceRegion.current;
    if (region === null) {
      return;
    }
    return listenToFlyoutLayout(region, (layout, from) => {
      if (layout.width <= 0) {
        setToolbox(null);
        return;
      }
      const outer = region.getBoundingClientRect();
      const inner = from.getBoundingClientRect();
      setToolbox({
        layout,
        left: inner.left - outer.left + layout.left + layout.width,
        top: inner.top - outer.top,
        bottom: outer.bottom - inner.bottom,
      });
    });
  }, []);

  // The layout the user left (05 §5.9): taken from the settings once, before the first paint when
  // the start of the window read them already, or else when they arrive.
  useLayoutEffect(() => {
    const restore = (layout: LayoutSettings): void => {
      if (layout.toolboxWidth !== undefined) {
        setToolboxChosen(layout.toolboxWidth);
      }
      if (layout.rightDockWidth !== undefined) {
        setRightChosen(layout.rightDockWidth);
      }
      if (layout.bottomDockHeight !== undefined) {
        setBottomChosen(layout.bottomDockHeight);
      }
      // Absent means shown, as on the first start.
      const hidden = {
        right: layout.rightCollapsed ?? false,
        bottom: layout.bottomCollapsed ?? false,
      };
      rememberedHidden.current = hidden;
      const { ui, actions } = useAppStore.getState();
      const patch: { rightCollapsed?: boolean; bottomCollapsed?: boolean } = {};
      if (hidden.bottom !== ui.bottomCollapsed) {
        patch.bottomCollapsed = hidden.bottom;
      }
      if (hidden.right) {
        // Hidden by the user: it stays hidden when the window grows.
        rightHiddenToFit.current = false;
        if (!ui.rightCollapsed) {
          patch.rightCollapsed = true;
        }
      } else if (ui.rightCollapsed && !rightHiddenToFit.current) {
        // Shown, unless the window is too small for it now.
        patch.rightCollapsed = false;
      }
      if (patch.rightCollapsed !== undefined || patch.bottomCollapsed !== undefined) {
        actions.setUi(patch);
      }
    };
    const now = useAppStore.getState().settings.value;
    if (now !== null) {
      restore(now.ui.layout);
      return;
    }
    let restored = false;
    const unsubscribe = useAppStore.subscribe((state) => {
      const settings = state.settings.value;
      if (restored || settings === null) {
        return;
      }
      restored = true;
      unsubscribe();
      restore(settings.ui.layout);
    });
    return unsubscribe;
  }, []);

  // Hands on each change of whether a dock is hidden, whoever made it (a dock's button or
  // splitter, or a command that shows the console), except the C++ dock's hiding while it does not
  // fit: what the user chose stays remembered.
  useEffect(
    () =>
      useAppStore.subscribe((state) => {
        const remembered = rememberedHidden.current;
        if (remembered === null) {
          return;
        }
        const change: LayoutPatch = {};
        if (state.ui.bottomCollapsed !== remembered.bottom) {
          remembered.bottom = state.ui.bottomCollapsed;
          change.bottomCollapsed = remembered.bottom;
        }
        if (!rightHiddenToFit.current && state.ui.rightCollapsed !== remembered.right) {
          remembered.right = state.ui.rightCollapsed;
          change.rightCollapsed = remembered.right;
        }
        if (change.bottomCollapsed !== undefined || change.rightCollapsed !== undefined) {
          rememberLayout(change);
        }
      }),
    [],
  );

  // Gives the toolbox's flyout the width the user chose: a remembered one once the flyout is there,
  // and again to a new flyout. Without a choice, the flyout keeps its default width.
  const toolboxSetWidth = toolbox?.layout.setWidth;
  useEffect(() => {
    const given = toolboxGiven.current;
    if (
      toolboxSetWidth === undefined ||
      toolboxChosen === null ||
      (given?.flyout === toolboxSetWidth && given.width === toolboxChosen)
    ) {
      return;
    }
    toolboxGiven.current = { flyout: toolboxSetWidth, width: toolboxChosen };
    toolboxSetWidth(toolboxChosen);
  }, [toolboxSetWidth, toolboxChosen]);

  const { rightCollapsed, bottomCollapsed } = useAppStore(
    useShallow((state) => ({
      rightCollapsed: state.ui.rightCollapsed,
      bottomCollapsed: state.ui.bottomCollapsed,
    })),
  );
  const rightId = useId();
  const bottomId = useId();
  const rightToggle = useRef<HTMLButtonElement>(null);
  const bottomToggle = useRef<HTMLButtonElement>(null);
  const panels = DockPanels();
  const { setUi } = useAppStore.getState().actions;

  /**
   * Collapses a dock from its focused splitter. The splitter is removed, so the focus goes to the
   * dock's toggle button once it shows its new state (rendered at once, so the focus is never left
   * on the page's body and the button is announced as "Show …").
   */
  const collapseFromSplitter = (
    patch: { rightCollapsed: true } | { bottomCollapsed: true },
    toggle: { readonly current: HTMLButtonElement | null },
  ) => {
    if ('rightCollapsed' in patch) {
      rightHiddenToFit.current = false;
    }
    flushSync(() => {
      setUi(patch);
    });
    toggle.current?.focus();
  };

  // The largest sizes now (the splitters' `aria-valuemax`), and the sizes shown within them.
  const rightMaximum =
    area === null
      ? RIGHT_DOCK.max
      : rightDockMaximum(
          area.width,
          toolbox === null ? null : { left: toolbox.layout.left, min: toolbox.layout.min },
        );
  const rightLimit = Math.max(RIGHT_DOCK.min, rightMaximum);
  const rightWidth = clampSize(
    rightChosen ?? startSize(RIGHT_DOCK, windowExtent.width),
    RIGHT_DOCK.min,
    rightLimit,
  );
  const bottomLimit = area === null ? BOTTOM_DOCK.max : dockMaximum(BOTTOM_DOCK, area.height);
  const bottomHeight = clampSize(
    bottomChosen ?? startSize(BOTTOM_DOCK, windowExtent.height),
    BOTTOM_DOCK.min,
    bottomLimit,
  );
  const rightDoesNotFit = rightMaximum < RIGHT_DOCK.min;

  // Hides the C++ dock while it does not fit, and shows it again once it does, unless the user
  // showed or hid it in the meantime.
  useEffect(() => {
    const { ui, actions } = useAppStore.getState();
    if (rightDoesNotFit) {
      if (!ui.rightCollapsed) {
        rightHiddenToFit.current = true;
        actions.setUi({ rightCollapsed: true });
      }
    } else if (rightHiddenToFit.current) {
      rightHiddenToFit.current = false;
      if (ui.rightCollapsed) {
        actions.setUi({ rightCollapsed: false });
      }
    }
  }, [rightDoesNotFit]);

  // `min()` keeps a dock within its share of the area before it was first measured.
  const share = `${String(MAX_DOCK_SHARE * 100)}%`;
  const columns = rightCollapsed
    ? 'minmax(0, 1fr) 0 auto'
    : `minmax(0, 1fr) auto min(${String(rightWidth)}px, ${share})`;
  const rows = bottomCollapsed
    ? 'minmax(0, 1fr) 0 auto'
    : `minmax(0, 1fr) auto min(${String(bottomHeight)}px, ${share})`;

  return (
    <div
      ref={layoutElement}
      className="editor-layout"
      hidden={hidden}
      style={{ gridTemplateColumns: columns, gridTemplateRows: rows }}
    >
      {/* Focusable from code only (tabIndex -1): see the focus effect above. */}
      <main
        ref={workspaceRegion}
        className="workspace"
        aria-label="Block workspace"
        tabIndex={-1}
        data-testid="workspace"
      >
        {workspace}
        {toolbox !== null && (
          <Splitter
            orientation="vertical"
            side="before"
            value={Math.round(toolbox.layout.width)}
            min={toolbox.layout.min}
            max={toolbox.layout.max}
            onChange={(width) => {
              toolboxGiven.current = { flyout: toolbox.layout.setWidth, width };
              toolbox.layout.setWidth(width);
              setToolboxChosen(width);
              rememberLayout({ toolboxWidth: Math.round(width) });
            }}
            label="Resize the toolbox"
            className="splitter-toolbox"
            style={{ left: toolbox.left, top: toolbox.top, bottom: toolbox.bottom }}
          />
        )}
      </main>

      {!rightCollapsed && (
        <Splitter
          orientation="vertical"
          value={rightWidth}
          min={RIGHT_DOCK.min}
          max={rightLimit}
          onChange={(width) => {
            setRightChosen(width);
            rememberLayout({ rightDockWidth: width });
          }}
          onCollapse={() => {
            collapseFromSplitter({ rightCollapsed: true }, rightToggle);
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
            ref={rightToggle}
            type="button"
            className="icon-button"
            data-testid="right-dock-toggle"
            aria-expanded={!rightCollapsed}
            aria-controls={`${rightId}-content`}
            aria-label={rightCollapsed ? 'Show the C++ panel' : 'Hide the C++ panel'}
            onClick={() => {
              rightHiddenToFit.current = false;
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
          max={bottomLimit}
          onChange={(height) => {
            setBottomChosen(height);
            rememberLayout({ bottomDockHeight: height });
          }}
          onCollapse={() => {
            collapseFromSplitter({ bottomCollapsed: true }, bottomToggle);
          }}
          label="Resize the bottom panel"
          controls={bottomId}
        />
      )}

      <BottomDock
        id={bottomId}
        collapsed={bottomCollapsed}
        panels={panels}
        toggleRef={bottomToggle}
      />
    </div>
  );
}

/** The bottom dock's tabs: Console, Problems (n) and Build output. */
function BottomDock({
  id,
  collapsed,
  panels,
  toggleRef,
}: {
  id: string;
  collapsed: boolean;
  panels: ReturnType<typeof DockPanels>;
  /** The show/hide button, which gets the focus when the splitter collapses the dock. */
  toggleRef: Ref<HTMLButtonElement>;
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
                data-testid={`dock-tab-${value}`}
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
            ref={toggleRef}
            type="button"
            className="icon-button"
            data-testid="bottom-dock-toggle"
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
