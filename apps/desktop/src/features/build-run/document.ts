/**
 * The document a build sends (docs/spec/02-architecture.md §2.4.2): the project as canonical BDM
 * text, never C++ (the backend generates the C++ itself), and its content hash, which says whether
 * a finished build still matches the project.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';

import type { EditorHandle } from '../../app/editor-types';
import type { useAppStore } from '../../app/store';

/** What a build is asked to build. */
export interface BuildDocument {
  /** The canonical BDM text (`build_start`'s `document`). */
  readonly text: string;
  /** Its content hash (64 lower-case hex digits, 05 §5.11). */
  readonly hash: string;
}

/** Where the document comes from. */
export interface DocumentSources {
  readonly store: typeof useAppStore;
  readonly editor: () => EditorHandle | null;
  readonly core: () => CoreWasm | null;
}

/**
 * The open project's document as it is now, or `null` when no project is open.
 *
 * The preview pipeline puts the canvas into the store 50 ms after the last change, so a build
 * started right after an edit would still see the old text. When the editor and the compiler core
 * are there, the canvas is therefore read now and passed through the core's `canonical()` (the
 * same loader and serialisation as the pipeline, so the text and hash are what the pipeline will
 * store). Otherwise, or when that fails, the store's canonical text is used.
 */
export function documentToBuild(sources: DocumentSources): BuildDocument | null {
  const project = sources.store.getState().project;
  if (project === null) {
    return null;
  }
  const editor = sources.editor();
  const core = sources.core();
  if (editor !== null && core !== null) {
    try {
      const result = core.canonical(JSON.stringify(editor.currentDocument()));
      if (result.ok) {
        return { text: result.text, hash: result.hash };
      }
      console.warn(
        'The canvas did not load for the build; building the last synchronised document',
        result.diagnostics.map((diagnostic) => diagnostic.code).join(', '),
      );
    } catch (error: unknown) {
      console.warn('Could not read the canvas for the build; building the last synchronised one', {
        name: error instanceof Error ? error.name : typeof error,
      });
    }
  }
  return { text: project.canonicalText, hash: project.contentHash };
}
