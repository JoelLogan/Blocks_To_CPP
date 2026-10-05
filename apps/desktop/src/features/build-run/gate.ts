/**
 * The preconditions of Build and Run in the frontend (docs/spec/04-user-interface.md §4.4,
 * 07 §7.6.1), checked again by the commands themselves: the toolbar and the shortcuts check them
 * before running a command, but ⟲ Run again in the console and anything else that runs a command
 * directly do not. The backend checks trust and errors once more on its own.
 */
import type { FeatureContext } from '../../app/features';
import { type GatedAction, runGate, runGateMessage } from '../../app/runGate';
import { firstError } from '../../app/store/selectors';

/** The `data-dock-panel` value of the Problems tab (the shell's PROBLEMS_PANEL). */
const PROBLEMS_PANEL = 'problems';

/** What the gate needs. */
export type GateContext = Pick<FeatureContext, 'store' | 'commands' | 'screens' | 'dialogs'>;

/**
 * Whether `action` may go ahead now. When it may not, this does what the toolbar would: with
 * errors it shows the first one (`problems.focusFirstError`, in both *Run on errors* modes);
 * without a compiler it opens the toolchain setup page (or says why); in Restricted Mode it says
 * how to leave it. Without a project it does nothing.
 */
export function gateAllows(action: GatedAction, ctx: GateContext): boolean {
  const state = ctx.store.getState();
  const gate = runGate(state);
  if (gate.reason === 'errors') {
    ctx.commands.runCommand('problems.focusFirstError').catch((error: unknown) => {
      console.error('Command problems.focusFirstError failed', error);
    });
    return false;
  }
  if (gate.enabled) {
    return true;
  }
  const discovering = state.toolchains.discovering;
  switch (gate.reason) {
    case 'noToolchain':
      if (ctx.screens.screen('toolchainSetup') !== null) {
        state.actions.setUi({ screen: 'toolchainSetup' });
      } else {
        void ctx.dialogs.alert({
          title: 'No C++ compiler',
          message: discovering
            ? 'Blocks2Cpp is still looking for a C++ compiler (g++). Try again in a moment.'
            : 'Blocks2Cpp found no C++ compiler (g++) it can use.',
        });
      }
      return false;
    case 'restricted':
      void ctx.dialogs.alert({
        title: 'Restricted Mode',
        message: runGateMessage(gate, action, { discovering }),
      });
      return false;
    default:
      return false;
  }
}

/** Moves the keyboard focus into the Problems tab once it is shown: to its grid, or the panel. */
function focusProblemsPanel(): void {
  globalThis.setTimeout(() => {
    const panel = document.querySelector<HTMLElement>(`[data-dock-panel="${PROBLEMS_PANEL}"]`);
    const cell = panel?.querySelector<HTMLElement>('[role="grid"] [tabindex="0"]');
    (cell ?? panel)?.focus();
  }, 0);
}

/**
 * `problems.focusFirstError`: shows the editor with the Problems tab open, selects and centres
 * the block of the first error (a live error first, then one of the last build), and moves the
 * keyboard focus into the Problems grid, onto its first row, which lists the most serious problem.
 */
export function focusFirstError(ctx: Pick<FeatureContext, 'store' | 'editor'>): void {
  const state = ctx.store.getState();
  const block = firstError(state)?.primary.block;
  state.actions.setUi({
    screen: 'editor',
    bottomTab: 'problems',
    bottomCollapsed: false,
    ...(block === undefined ? {} : { selection: block }),
  });
  if (block !== undefined) {
    ctx.editor()?.selectBlock(block, { center: true });
  }
  focusProblemsPanel();
}
