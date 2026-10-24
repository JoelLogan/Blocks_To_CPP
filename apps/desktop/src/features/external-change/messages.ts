/**
 * What the external-change dialog says (docs/spec/04-user-interface.md §4.10): the question when
 * the open project's file changed on disk or disappeared, and why a reload failed.
 */
import type { IpcError, TransportError } from '@blocks2cpp/ipc-types';

import type { DialogChoice } from '../../app/dialogs';
import type { ProjectState } from '../../app/store';
import { documentProblem, shownName, somethingWentWrong } from '../recovery/text';

/** The dialog's answers. */
export type ExternalChangeChoice = 'reload' | 'keepMine' | 'later';

/** The question to ask, as `DialogService.choose` takes it. */
export interface ExternalChangeQuestion {
  readonly title: string;
  readonly message: string;
  readonly choices: readonly DialogChoice<ExternalChangeChoice>[];
}

/** The label of the answer that keeps the editor's version (04 §4.10). */
export const KEEP_MINE_LABEL = 'Keep mine (save as…)';

/** The label of the answer that reloads the file. */
export const RELOAD_LABEL = 'Reload';

/** The label of the answer that decides later (also what dismissing the dialog means). */
export const LATER_LABEL = 'Not now';

/**
 * The question about `project`'s file. `deleted`: it was deleted, renamed or moved, so *Reload*
 * is not offered (05 §5.10) and only *Keep mine (save as…)* is. Otherwise *Reload* is offered
 * too, as the suggested answer when there are no unsaved changes to lose, and marked as losing
 * data when there are.
 */
export function externalChangeQuestion(
  project: ProjectState,
  deleted: boolean,
): ExternalChangeQuestion {
  const name = shownName(project.document.project.name);
  const file = shownName(project.fileName ?? 'The project file');
  const keepMine: DialogChoice<ExternalChangeChoice> = { id: 'keepMine', label: KEEP_MINE_LABEL };
  const later: DialogChoice<ExternalChangeChoice> = { id: 'later', label: LATER_LABEL };
  if (deleted) {
    return {
      title: `“${name}” was deleted or moved`,
      message: `${file} is no longer where Blocks2Cpp opened it: it was deleted, renamed or moved by another program. It cannot be reloaded. Keep your version by saving it as a new file.`,
      choices: [{ ...keepMine, primary: true }, later],
    };
  }
  if (project.dirty) {
    return {
      title: `“${name}” was changed outside Blocks2Cpp`,
      message: `Another program changed ${file} on disk. Reload it to see those changes, or keep your version and save it as a new file. Reloading discards the changes you have not saved.`,
      choices: [
        { id: 'reload', label: RELOAD_LABEL, destructive: true },
        { ...keepMine, primary: true },
        later,
      ],
    };
  }
  return {
    title: `“${name}” was changed outside Blocks2Cpp`,
    message: `Another program changed ${file} on disk. Reload it to see those changes, or keep the version shown here and save it as a new file.`,
    choices: [{ id: 'reload', label: RELOAD_LABEL, primary: true }, keepMine, later],
  };
}

/** Why `project_reload` failed, for the user (the editor's blocks are kept in every case). */
export function describeReloadError(error: IpcError | TransportError | null, code: string): string {
  const problem = error === null ? null : documentProblem(error);
  if (problem !== null) {
    return `The file on disk is not a project Blocks2Cpp can open:\n${problem}\nYour blocks were not changed. You can keep them by saving them as a new file.`;
  }
  if (error?.code === 'io') {
    return error.kind === 'permissionDenied'
      ? 'Blocks2Cpp is not allowed to read the file. Your blocks were not changed.'
      : 'The file could not be read. Your blocks were not changed. Try again later.';
  }
  return somethingWentWrong('the file was not reloaded', code);
}
