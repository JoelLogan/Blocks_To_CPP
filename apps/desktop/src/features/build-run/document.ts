/**
 * The document a build sends (docs/spec/02-architecture.md §2.4.2): the project as canonical BDM
 * text, never C++ (the backend generates the C++ itself), and its content hash, which says whether
 * a finished build still matches the project.
 */
import type { CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import type { Diagnostic } from '@blocks2cpp/ipc-types';

import type { EditorHandle } from '../../app/editor-types';
import type { ProjectState, useAppStore } from '../../app/store';

/** What a build is asked to build. */
export interface BuildDocument {
  /** The canonical BDM text (`build_start`'s `document`). */
  readonly text: string;
  /** Its content hash (64 lower-case hex digits, 05 §5.11). */
  readonly hash: string;
}

/**
 * What {@link documentToBuild} found: the document to build, or the loader's problems with the
 * canvas, which must not be built at all.
 */
export type DocumentToBuild =
  | { readonly kind: 'document'; readonly document: BuildDocument }
  | { readonly kind: 'unreadable'; readonly diagnostics: readonly Diagnostic[] };

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
 * started right after an edit would still see the old document. When the editor and the compiler
 * core are there, the canvas is therefore read now and passed through the core's `canonical()`
 * (the same loader and serialisation as the pipeline, so the hash is what the pipeline will
 * store). Without the editor, or when reading the canvas fails (the core stopped), the store's
 * document, the last one synchronised, is built: its canonical text from the core when it
 * answers, and otherwise its JSON text, which the backend loads into the same document (the store
 * keeps only the loader's output, whose numbers JavaScript holds exactly, 05 §5.6).
 *
 * When the loader refuses what the canvas reads back as (for example blocks nested deeper than a
 * project may be, B2C-E0104), the result is `unreadable` with the loader's problems: the store
 * still holds an older version, and building that instead would silently ignore the newest blocks.
 */
export function documentToBuild(sources: DocumentSources): DocumentToBuild | null {
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
        return { kind: 'document', document: { text: result.text, hash: result.hash } };
      }
      console.warn(
        'The canvas does not load; nothing is built',
        result.diagnostics.map((diagnostic) => diagnostic.code).join(', '),
      );
      return { kind: 'unreadable', diagnostics: result.diagnostics };
    } catch (error: unknown) {
      console.warn('Could not read the canvas for the build; building the last synchronised one', {
        name: error instanceof Error ? error.name : typeof error,
      });
    }
  }
  return { kind: 'document', document: storedDocument(project, core) };
}

/**
 * The store's document as the build sends it: its canonical text and content hash from the core,
 * or, when the core cannot answer (it stopped, or has not started), its JSON text with the hash
 * the pipeline stored for it.
 */
function storedDocument(project: ProjectState, core: CoreWasm | null): BuildDocument {
  const json = JSON.stringify(project.document);
  if (core !== null) {
    try {
      const result = core.canonical(json);
      if (result.ok) {
        return { text: result.text, hash: result.hash };
      }
    } catch (error: unknown) {
      console.warn('The compiler core could not write the last synchronised document', {
        name: error instanceof Error ? error.name : typeof error,
      });
    }
  }
  return { text: json, hash: project.contentHash };
}
