/**
 * The document a save writes (docs/spec/05-project-format.md §5.2, §5.3): the editor's document
 * with the viewports captured and `generator` naming the app and catalog that saved it. Pure
 * functions over loaded documents; the compiler core validates the result before it is sent.
 */
import type { BdmDocument, BdmGenerator } from '@blocks2cpp/b2c-core-wasm';

/** The document with `generator` replaced. */
export function withGenerator(doc: BdmDocument, generator: BdmGenerator): BdmDocument {
  return { ...doc, generator: { app: generator.app, catalog: generator.catalog } };
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
