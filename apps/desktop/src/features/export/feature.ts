/**
 * Export as a C++ project (docs/spec/04-user-interface.md §4.10, 06 §6.11): the `project.export`
 * command (*Export as C++ project…* in the ≡ menu) sends the project as it is now, never C++, to
 * `project_export_dialog`. The backend asks for the folder in a native dialog, generates the C++
 * itself and writes `src/`, `CMakeLists.txt`, `Makefile` and `README.md` into a folder named after
 * the program; it asks before it replaces an earlier export. A canvas that does not read back as
 * a project is not exported (as it is not built), and the outcome is said in a dialog.
 */
import type { Feature, FeatureContext } from '../../app/features';
import { documentToBuild } from '../build-run/document';
import { failureCode, type FailureCode, type FailureMessage } from '../build-run/messages';
import { problemLines } from '../recovery/text';

/** The text for an export that could not be made. */
export function exportFailureMessage(code: FailureCode): FailureMessage {
  const title = 'The project could not be exported';
  switch (code) {
    case 'projectErrors':
      return { title, message: 'The blocks have errors. Fix them, then export again.' };
    case 'defineNotExportable':
      return {
        title,
        message:
          'A text define holds #, ; or $, which CMake and make cannot pass to the compiler. Change it on the Project settings page (≡ → Project settings…), then export again.',
      };
    case 'io':
      return {
        title,
        message:
          'The folder could not be written. A folder of that name that holds other files is never written into: choose another place, or move those files.',
      };
    case 'busy':
      return { title, message: 'Another dialog is open. Close it, then export again.' };
    case 'unknownHandle':
      return { title, message: 'The project is no longer open.' };
    case 'payloadTooLarge':
    case 'invalidDocument':
    case 'newerFormat':
      return { title, message: 'The project could not be read for the export.' };
    default:
      return { title, message: 'Something went wrong. The details are in the log file.' };
  }
}

/** Exports the open project; see the module documentation. */
export async function exportProject(ctx: FeatureContext): Promise<void> {
  const project = ctx.store.getState().project;
  const found = documentToBuild({ store: ctx.store, editor: ctx.editor, core: ctx.core });
  if (project === null || found === null) {
    return;
  }
  if (found.kind === 'unreadable') {
    await ctx.dialogs.alert({
      title: 'The project could not be exported',
      message: `Blocks2Cpp cannot read your latest change back as a project, so nothing was exported:\n${problemLines(found.diagnostics).join('\n')}\nUndo the last change (Ctrl+Z), then try again.`,
    });
    return;
  }
  try {
    const response = await ctx.ipc.projectExportDialog({
      handle: project.handle,
      document: found.document.text,
    });
    if (response.cancelled) {
      return;
    }
    await ctx.dialogs.alert({
      title: 'Project exported',
      message: `The C++ project is in the folder “${response.folderName ?? ''}”, with ${String(response.files)} files: the code in src, and a CMakeLists.txt, a Makefile and a README that say how to build it.`,
    });
  } catch (error: unknown) {
    const code = failureCode(error);
    console.warn('project_export_dialog failed', code);
    await ctx.dialogs.alert(exportFailureMessage(code));
  }
}

/** The export feature: registers `project.export`. */
export const exportFeature: Feature = (ctx) =>
  ctx.commands.registerCommand('project.export', () => exportProject(ctx));
