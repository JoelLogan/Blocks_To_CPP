/**
 * Opening a document in the editor (docs/spec/04-user-interface.md §4.10, 08 §8.3): the one way a
 * project's text becomes editor state. The text comes from the backend (open, new, reload,
 * recovery) and is untrusted, so it goes through the compiler core's loader, with every limit of
 * 05 §5.6, and is never `JSON.parse`d here.
 */
import {
  type CoreWasm,
  CoreTrap,
  type Diagnostic,
  MAX_DOCUMENT_BYTES,
} from '@blocks2cpp/b2c-core-wasm';
import type { Handle, Trust } from '@blocks2cpp/ipc-types';

import type { FeatureContext } from '../app/features';
import { appCoreHost, type CoreHost } from './preview/coreHost';

/** What {@link openDocumentInEditor} shows. */
export interface OpenDocumentArgs {
  /** The backend's handle for the project. */
  handle: Handle;
  /** The project file's text, as the backend sent it. */
  documentText: string;
  /** The trust state (08 §8.3). */
  trust: Trust;
  /** The file's name for display, or `null` for a project that was never saved. */
  fileName: string | null;
  /** The format version the file was migrated from in memory, or `null`. */
  migratedFrom: number | null;
  /**
   * The text of the project as last saved, which decides whether it has unsaved changes: the same
   * as `documentText` for a project opened from its file, the file's text for a restored
   * recovery snapshot, `null` for a project that was never saved.
   */
  savedText: string | null;
}

/** The outcome: shown, or the loader's problems (the editor is left as it was). */
export type OpenDocumentResult = { ok: true } | { ok: false; diagnostics: Diagnostic[] };

/** Text cut to one character over the size limit: anything longer is refused just the same. */
function bounded(text: string): string {
  return text.length > MAX_DOCUMENT_BYTES ? text.slice(0, MAX_DOCUMENT_BYTES + 1) : text;
}

/** Runs `use` on the core, once more on a fresh instance when the first one traps. */
async function withCore<T>(
  ctx: FeatureContext,
  host: CoreHost,
  action: (core: CoreWasm) => T,
): Promise<T> {
  const core = ctx.core() ?? (await host.start());
  try {
    return action(core);
  } catch (error: unknown) {
    if (!(error instanceof CoreTrap)) {
      throw error;
    }
    console.error('The compiler core stopped while opening a project; starting a new one', error);
    return action(await host.restart());
  }
}

/**
 * Loads `documentText` with the compiler core, makes it the open project in the store, shows it in
 * the editor (with an empty undo history) and switches to the editor screen.
 *
 * Its canonical text becomes the project's; it has unsaved changes when that differs from the
 * canonical form of `savedText`. Reopening the project that is open (same handle) keeps the shown
 * module and the time of the last save.
 *
 * @throws CoreError (`init`) when the compiler core cannot be started, and `CoreTrap` when it
 *   stops twice in a row. A document the loader refuses is not an error: the result carries its
 *   diagnostics.
 */
export async function openDocumentInEditor(
  ctx: FeatureContext,
  args: OpenDocumentArgs,
): Promise<OpenDocumentResult> {
  const host = appCoreHost();
  const bytes = new TextEncoder().encode(bounded(args.documentText));
  const outcome = await withCore(ctx, host, (core) => {
    const loaded = core.load(bytes);
    if (!loaded.ok) {
      return { ok: false as const, diagnostics: loaded.diagnostics };
    }
    const canonical = core.canonical(JSON.stringify(loaded.document));
    if (!canonical.ok) {
      return { ok: false as const, diagnostics: canonical.diagnostics };
    }
    let savedCanonicalText: string | null = null;
    if (args.savedText === args.documentText) {
      savedCanonicalText = canonical.text;
    } else if (args.savedText !== null) {
      const saved = core.canonical(bounded(args.savedText));
      savedCanonicalText = saved.ok ? saved.text : null;
    }
    return { ok: true as const, document: loaded.document, canonical, savedCanonicalText };
  });
  if (!outcome.ok) {
    return outcome;
  }

  const { document, canonical, savedCanonicalText } = outcome;
  const { project: previous, actions } = ctx.store.getState();
  const sameProject = previous?.handle === args.handle;
  const keptModule =
    sameProject && document.modules.some((module) => module.id === previous.activeModuleId)
      ? previous.activeModuleId
      : undefined;
  actions.setProject({
    handle: args.handle,
    fileName: args.fileName,
    document,
    canonicalText: canonical.text,
    contentHash: canonical.hash,
    savedCanonicalText,
    savedAt: sameProject ? previous.savedAt : null,
    dirty: canonical.text !== savedCanonicalText,
    trust: args.trust,
    activeModuleId: keptModule ?? document.modules[0]?.id ?? '',
    migratedFrom: args.migratedFrom,
  });
  // Without a mounted editor, the editor shows the store's project when it mounts.
  ctx.editor()?.loadDocument(document, { clearUndo: true });
  actions.setUi({ screen: 'editor' });
  return { ok: true };
}
