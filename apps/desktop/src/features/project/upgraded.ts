/**
 * The notice for a project whose blocks were upgraded on load (docs/spec/05-project-format.md §5.7,
 * ADR-0015): the backend upgraded blocks of an older version, or of a block type that was retired,
 * when it opened or reloaded the file (`blocksUpgraded`). The project then has unsaved changes,
 * and the file keeps its old form until the user saves; the save keeps it as `<name>.b2c.bak`.
 */
import type { AlertOptions } from '../../app/dialogs';
import { shownName } from './text';

/** The alert that says a project's blocks were upgraded, for the file `fileName`. */
export function blocksUpgradedAlert(fileName: string | null): AlertOptions {
  const upgraded = 'were made by an older version of Blocks2Cpp and have been upgraded';
  if (fileName === null) {
    return {
      title: 'Blocks were upgraded',
      message: `Some blocks of this project ${upgraded}, so it now has unsaved changes.`,
    };
  }
  const file = shownName(fileName);
  return {
    title: 'Blocks were upgraded',
    message: `Some blocks in “${file}” ${upgraded}, so the project now has unsaved changes. When you save it, the file as it was is kept as “${file}.bak”.`,
  };
}
