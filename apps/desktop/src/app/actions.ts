/**
 * What the shell's own buttons and shortcuts do: Run and Build go through the run gate, and the
 * shell registers default handlers for the commands that only change what is shown.
 */
import { type CommandId, type CommandRegistry, commands, triggerCommand } from './commands';
import { type EditorHandle, getEditorHandle } from './editor-types';
import { type GatedAction, runGate } from './runGate';
import { type ScreenRegistry, screens } from './screens';
import { useAppStore } from './store';
import { firstError } from './store/selectors';

/** What {@link activateGated} and {@link installShellCommands} need. */
export interface ShellActionContext {
  store: typeof useAppStore;
  commands: CommandRegistry;
  screens: ScreenRegistry;
  editor: () => EditorHandle | null;
}

/** The app's own registries and store: what the window's buttons and shortcuts act on. */
export const SHELL: ShellActionContext = {
  store: useAppStore,
  commands,
  screens,
  editor: getEditorHandle,
};

const GATED_COMMANDS: Record<GatedAction, CommandId> = {
  run: 'run.start',
  build: 'build.start',
};

/**
 * Presses Run or Build (the toolbar button, `F5` or `Ctrl+B`). When the gate is open, it runs the
 * command. While the project has errors it shows the first one instead, whether the *Run on
 * errors* setting disables the button or not (04 §4.4). Without a usable compiler it opens the
 * toolchain setup page. In Restricted Mode or without a project it does nothing: the banner and
 * the start page explain those.
 */
export function activateGated(action: GatedAction, ctx: ShellActionContext): void {
  const state = ctx.store.getState();
  const gate = runGate(state);
  if (gate.reason === 'errors') {
    triggerCommand('problems.focusFirstError', ctx.commands);
    return;
  }
  if (gate.enabled) {
    triggerCommand(GATED_COMMANDS[action], ctx.commands);
    return;
  }
  if (gate.reason === 'noToolchain' && ctx.screens.screen('toolchainSetup') !== null) {
    state.actions.setUi({ screen: 'toolchainSetup' });
  }
}

/** The `data-dock-panel` value of the Problems tab's panel. */
export const PROBLEMS_PANEL = 'problems';

/**
 * Moves the keyboard focus to a bottom-dock panel once it is shown (after the state change that
 * shows it has rendered).
 */
function focusDockPanel(panel: string): void {
  window.setTimeout(() => {
    const element = document.querySelector<HTMLElement>(`[data-dock-panel="${panel}"]`);
    element?.focus();
  }, 0);
}

/**
 * Registers the shell's default handlers, which features may override:
 *
 * - `problems.focusFirstError` opens the Problems tab, selects the block of the first error (live
 *   errors first, then build errors) and moves the focus to the Problems panel;
 * - `settings.open` shows the Settings page, once a feature provides it.
 *
 * Returns the function that removes them.
 */
export function installShellCommands(ctx: ShellActionContext): () => void {
  const removers = [
    ctx.commands.registerCommand('problems.focusFirstError', () => {
      const state = ctx.store.getState();
      const error = firstError(state);
      const block = error?.primary.block;
      state.actions.setUi({
        screen: 'editor',
        bottomTab: 'problems',
        bottomCollapsed: false,
        ...(block === undefined ? {} : { selection: block }),
      });
      if (block !== undefined) {
        ctx.editor()?.selectBlock(block, { center: true });
      }
      focusDockPanel(PROBLEMS_PANEL);
    }),
    ctx.commands.registerCommand('settings.open', () => {
      if (ctx.screens.screen('settings') !== null) {
        ctx.store.getState().actions.setUi({ screen: 'settings' });
      }
    }),
  ];
  return () => {
    for (const remove of removers) {
      remove();
    }
  };
}
