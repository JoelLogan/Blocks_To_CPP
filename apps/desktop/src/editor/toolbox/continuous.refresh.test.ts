/**
 * When the continuous toolbox shows its flyout again (docs/spec/04-user-interface.md §4.2): only
 * when what its dynamic categories depend on changed (their contents key), never while a block is
 * dragged, a field editor or a menu is open or the flyout scrolls to a chosen category, and then
 * with the flyout where it was. The toolbox plugin's use of it is in ./plugin.test.ts.
 */
import * as Blockly from 'blockly/core';
import { afterEach, beforeAll, describe, expect, it, type MockInstance, vi } from 'vitest';

import {
  B2cContinuousFlyout,
  B2cContinuousToolbox,
  CATEGORY_SCROLL_STILL_MS,
  CONTINUOUS_REFRESH_DELAY_MS,
  MAX_CATEGORY_SCROLL_MS,
} from './continuous';
import { disposeTestWorkspaces, injectedWorkspace, setUpToolboxBlocks } from './testing';

beforeAll(() => {
  setUpToolboxBlocks();
});

afterEach(() => {
  vi.restoreAllMocks();
  Blockly.WidgetDiv.hide();
  disposeTestWorkspaces();
});

/** Waits for the toolbox's delayed rebuild. */
function rebuildDelay(): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, CONTINUOUS_REFRESH_DELAY_MS + 50));
}

/** An injected continuous toolbox with its flyout shown once, and a contents key it reads. */
function shownToolbox(key: { value: string }): {
  workspace: Blockly.WorkspaceSvg;
  toolbox: B2cContinuousToolbox;
  flyout: B2cContinuousFlyout;
} {
  const workspace = injectedWorkspace('continuous');
  workspace.setCachedParentSvgSize(1000, 500);
  workspace.resize();
  const toolbox = workspace.getToolbox();
  const flyout = workspace.getFlyout();
  if (!(toolbox instanceof B2cContinuousToolbox) || !(flyout instanceof B2cContinuousFlyout)) {
    throw new Error('not the continuous toolbox');
  }
  toolbox.setContentsKey(() => key.value);
  toolbox.showAllCategories();
  return { workspace, toolbox, flyout };
}

describe('the contents key', () => {
  it('lets a refresh build nothing while it is unchanged', async () => {
    const key = { value: 'a' };
    const { toolbox } = shownToolbox(key);
    const build = vi.spyOn(toolbox, 'flyoutContents');
    toolbox.refreshSelection();
    toolbox.refreshSelection();
    await rebuildDelay();
    expect(build).not.toHaveBeenCalled();
    key.value = 'b';
    toolbox.refreshSelection();
    await rebuildDelay();
    expect(build).toHaveBeenCalledTimes(1);
  });

  it('builds at every refresh without a key, or when the key fails', async () => {
    const key = { value: 'a' };
    const { toolbox } = shownToolbox(key);
    const build = vi.spyOn(toolbox, 'flyoutContents');
    toolbox.setContentsKey(null);
    toolbox.showAllCategories();
    toolbox.refreshSelection();
    await rebuildDelay();
    expect(build).toHaveBeenCalledTimes(2);

    const error = vi.spyOn(console, 'error').mockImplementation(() => undefined);
    toolbox.setContentsKey(() => {
      throw new Error('broken');
    });
    toolbox.refreshSelection();
    await rebuildDelay();
    expect(build).toHaveBeenCalledTimes(3);
    expect(error).toHaveBeenCalled();
  });

  it('is forgotten when new categories are drawn', async () => {
    const key = { value: 'a' };
    const { workspace, toolbox } = shownToolbox(key);
    const build = vi.spyOn(toolbox, 'flyoutContents');
    workspace.updateToolbox(
      workspace.options.languageTree ?? { kind: 'categoryToolbox', contents: [] },
    );
    toolbox.refreshSelection();
    await rebuildDelay();
    expect(build).toHaveBeenCalled();
  });
});

describe('a refresh', () => {
  /** A toolbox whose key has changed, so that its next refresh would show the flyout again. */
  function changed(): {
    workspace: Blockly.WorkspaceSvg;
    toolbox: B2cContinuousToolbox;
    flyout: B2cContinuousFlyout;
    show: MockInstance<B2cContinuousFlyout['show']>;
  } {
    const key = { value: 'a' };
    const shown = shownToolbox(key);
    key.value = 'b';
    // A new showing whatever the contents: the test sees each one.
    vi.spyOn(shown.toolbox, 'flyoutContents').mockImplementation(() => [
      { kind: 'label', text: `contents ${String(Math.random())}` },
    ]);
    return { ...shown, show: vi.spyOn(shown.flyout, 'show') };
  }

  it('waits while a block is dragged', async () => {
    const { workspace, toolbox, show } = changed();
    const dragging = vi.spyOn(workspace, 'isDragging').mockReturnValue(true);
    toolbox.refreshSelection();
    await rebuildDelay();
    await rebuildDelay();
    expect(show).not.toHaveBeenCalled();
    dragging.mockReturnValue(false);
    await rebuildDelay();
    expect(show).toHaveBeenCalledTimes(1);
  });

  it('waits while a field editor is open', async () => {
    const { workspace, toolbox, show } = changed();
    Blockly.WidgetDiv.show({}, false, () => undefined, workspace);
    toolbox.refreshSelection();
    await rebuildDelay();
    await rebuildDelay();
    expect(show).not.toHaveBeenCalled();
    Blockly.WidgetDiv.hide();
    await rebuildDelay();
    expect(show).toHaveBeenCalledTimes(1);
  });

  it('waits while a menu is open', async () => {
    const { toolbox, show } = changed();
    const open = vi.spyOn(Blockly.DropDownDiv, 'isVisible').mockReturnValue(true);
    toolbox.refreshSelection();
    await rebuildDelay();
    await rebuildDelay();
    expect(show).not.toHaveBeenCalled();
    open.mockReturnValue(false);
    await rebuildDelay();
    expect(show).toHaveBeenCalledTimes(1);
  });

  it('waits while the flyout scrolls to a chosen category, but not for ever', async () => {
    const { toolbox, flyout, show } = changed();
    const scrolling = vi.spyOn(flyout, 'isScrollingToCategory').mockReturnValue(true);
    toolbox.refreshSelection();
    await rebuildDelay();
    expect(show).not.toHaveBeenCalled();
    scrolling.mockRestore();
    await rebuildDelay();
    expect(show).toHaveBeenCalledTimes(1);

    // A scroll that never reaches its target counts for at most MAX_CATEGORY_SCROLL_MS.
    const now = vi.spyOn(performance, 'now').mockReturnValue(1_000);
    flyout.scrollTo(flyout.getCategoryScrollPosition('Functions') ?? 0);
    vi.spyOn(flyout.getWorkspace(), 'scrollY', 'get').mockReturnValue(0);
    expect(flyout.isScrollingToCategory()).toBe(true);
    now.mockReturnValue(1_000 + MAX_CATEGORY_SCROLL_MS + 1);
    expect(flyout.isScrollingToCategory()).toBe(false);
  });

  it('does not wait for a scroll to a category that has ended or stands still', () => {
    const { flyout } = changed();
    const workspace = flyout.getWorkspace();
    const scrollbar = workspace.scrollbar;
    if (scrollbar === null) {
      throw new Error('the flyout has no scrollbar');
    }
    const target = flyout.getCategoryScrollPosition('Functions') ?? 0;
    // The flyout goes where it is told, whatever room happy-dom's layout gives it.
    const setY = vi.spyOn(scrollbar, 'setY').mockImplementation((y: number) => {
      workspace.scrollY = -y;
    });
    const reducedMotion = (matches: boolean) =>
      vi.spyOn(window, 'matchMedia').mockReturnValue({ matches } as MediaQueryList);

    // With reduced motion the first step ends the scroll, also when the flyout then scrolls on
    // (the keyboard shows the block it moves to).
    reducedMotion(true);
    flyout.scrollTo(target);
    workspace.scrollY -= 100;
    expect(flyout.isScrollingToCategory()).toBe(false);

    // A scroll that moves, then stands still short of its target (held back, or moved elsewhere).
    reducedMotion(false);
    workspace.scrollY = 0;
    const now = vi.spyOn(performance, 'now').mockReturnValue(1_000);
    flyout.scrollTo(target);
    setY.mockImplementation(() => undefined);
    now.mockReturnValue(1_020);
    expect(flyout.isScrollingToCategory()).toBe(true);
    workspace.scrollY -= 10;
    now.mockReturnValue(1_040);
    expect(flyout.isScrollingToCategory()).toBe(true);
    now.mockReturnValue(1_040 + CATEGORY_SCROLL_STILL_MS - 1);
    expect(flyout.isScrollingToCategory()).toBe(true);
    now.mockReturnValue(1_040 + CATEGORY_SCROLL_STILL_MS);
    expect(flyout.isScrollingToCategory()).toBe(false);

    // Ends the plugin's animation, which would otherwise step on after the test.
    setY.mockImplementation((y: number) => {
      workspace.scrollY = -y;
    });
    reducedMotion(true);
    flyout.scrollTo(target);
  });
});
