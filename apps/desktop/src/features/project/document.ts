/**
 * The document a save writes (docs/spec/05-project-format.md §5.2, §5.3): the editor's document
 * with the viewports captured and `generator` naming the app and catalog that saved it. Pure
 * functions over loaded documents; the compiler core validates the result before it is sent.
 */
import type { BdmDocument, BdmGenerator } from '@blocks2cpp/b2c-core-wasm';

import { isNewerVersion } from './version';

/**
 * The document with `generator` naming this app (`generator.app`) and the newer of its own catalog
 * and this app's (`generator.catalog`, 05 §5.3): a save never lowers the catalog a file names, so
 * the blocks a newer catalog made, which this app keeps unchanged, are still reported as made by a
 * newer version (`B2C-E0602`) after the save, and not as unknown (ADR-0015).
 */
export function withGenerator(doc: BdmDocument, generator: BdmGenerator): BdmDocument {
  const catalog = isNewerVersion(doc.generator.catalog, generator.catalog)
    ? doc.generator.catalog
    : generator.catalog;
  return { ...doc, generator: { app: generator.app, catalog } };
}

/**
 * `current` (the editor's live document) with what a save added to `saved`: its `generator` and
 * the viewport of every module both have. Used when the project changed while it was being
 * saved, so the live document keeps the user's newer edits and the saved layout.
 */
export function withSavedLayout(current: BdmDocument, saved: BdmDocument): BdmDocument {
  const viewports = new Map(saved.modules.map((module) => [module.id, module.workspace.viewport]));
  return {
    ...current,
    generator: { app: saved.generator.app, catalog: saved.generator.catalog },
    modules: current.modules.map((module) => {
      if (!viewports.has(module.id)) {
        return module;
      }
      const viewport = viewports.get(module.id);
      const workspace = { ...module.workspace };
      if (viewport === undefined) {
        delete workspace.viewport;
      } else {
        workspace.viewport = viewport;
      }
      return { ...module, workspace };
    }),
  };
}
