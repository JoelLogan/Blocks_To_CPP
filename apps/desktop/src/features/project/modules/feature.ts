/**
 * The module edits feature (docs/plans/m3-contracts/w3-modules.md §1.2, §6 W3M-D6): publishes the
 * {@link ModuleEdits} the module strip uses (src/editor/modules/), and registers the commands that
 * act on the module shown: `module.add`, `module.rename`, `module.delete`, `module.moveLeft`,
 * `module.moveRight`, and, only while a deleted module is kept, `module.undoDelete` (so `≡` lists
 * *Undo delete module* only then).
 */
import type { Feature, FeatureContext } from '../../../app/features';
import { ModuleEdits, type ModuleEditsDeps, setModuleEdits } from './controller';

/** Installs the feature; see the module comment. Returns the edits and the uninstaller. */
export function installModuleEdits(
  ctx: FeatureContext,
  options: Pick<ModuleEditsDeps, 'newModuleId'> = {},
): { readonly edits: ModuleEdits; readonly uninstall: () => void } {
  const edits = new ModuleEdits({
    store: ctx.store,
    dialogs: ctx.dialogs,
    editor: ctx.editor,
    core: ctx.core,
    events: ctx.events,
    ...options,
  });
  setModuleEdits(edits);
  const shown = (): string | null => ctx.store.getState().project?.activeModuleId ?? null;
  const onShown =
    (action: (id: string) => void | Promise<void>): (() => Promise<void>) =>
    async () => {
      const id = shown();
      if (id !== null) {
        await action(id);
      }
    };
  const removers = [
    ctx.commands.registerCommand('module.add', () => edits.add()),
    ctx.commands.registerCommand(
      'module.rename',
      onShown((id) => edits.rename(id)),
    ),
    ctx.commands.registerCommand(
      'module.delete',
      onShown((id) => edits.remove(id)),
    ),
    ctx.commands.registerCommand(
      'module.moveLeft',
      onShown((id) => {
        edits.move(id, -1);
      }),
    ),
    ctx.commands.registerCommand(
      'module.moveRight',
      onShown((id) => {
        edits.move(id, 1);
      }),
    ),
  ];
  // *Undo delete module* is a command only while there is something to undo.
  let undoRegistration: (() => void) | null = null;
  const followKept = (): void => {
    const kept = edits.state.getState().kept !== null;
    if (kept && undoRegistration === null) {
      undoRegistration = ctx.commands.registerCommand('module.undoDelete', () => {
        edits.undoDelete();
      });
    } else if (!kept && undoRegistration !== null) {
      undoRegistration();
      undoRegistration = null;
    }
  };
  const stopFollowing = edits.state.subscribe(followKept);
  let installed = true;
  return {
    edits,
    uninstall: () => {
      if (!installed) {
        return;
      }
      installed = false;
      stopFollowing();
      undoRegistration?.();
      for (const remove of removers.reverse()) {
        remove();
      }
      edits.dispose();
      setModuleEdits(null);
    },
  };
}

/** The module edits feature, for src/features/index.ts. */
export const moduleEditsFeature: Feature = (ctx) => installModuleEdits(ctx).uninstall;
