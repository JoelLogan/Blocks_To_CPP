/**
 * Module edits as the student makes them (docs/plans/m3-contracts/w3-modules.md §1.2, §2.11, §2.13,
 * §6 W3M-D6): *Add module* and *Rename…* ask for a name in the app's dialog, checked as the loader
 * checks it (./names.ts); *Delete…* asks first when the module holds blocks, naming how many and
 * whether it holds *when program starts*; *Move left* and *Move right* move at once. The editor
 * makes each edit (`EditorHandle`), so the canvas's latest change is part of it.
 *
 * Every edit is refused, with the text of the banner, while the canvas holds a change that cannot
 * be read back. A deleted module is kept, with its IDs, until the next module edit or until the
 * project closes, so *Undo delete* (in the status message and in `≡`) puts it back where it was and
 * the other modules' references to it bind again. Each edit ends with a status message, read out
 * by screen readers; the one for the second module says that the code of the other modules
 * changed (§2.13).
 */
import type { BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import { newId } from '@blocks2cpp/blockly-ext';
import { createStore, type StoreApi } from 'zustand/vanilla';

import type { DialogService } from '../../../app/dialogs';
import type { EditorHandle, ModuleEditOutcome } from '../../../app/editor-types';
import type { useAppStore } from '../../../app/store';
import { syncFailedText } from '../../analysis/AnalysisNoticeBanner';
import { checkModuleName } from './names';
import {
  canAddModule,
  canDeleteModule,
  holdsProgramStart,
  type KeptModule,
  MODULE_EDIT_PROBLEMS,
  nextModuleName,
  topLevelBlockCount,
} from './operations';

/** A deleted module that *Undo delete* can put back. */
export interface KeptDeletion {
  /** The project it was deleted from (its handle): it is dropped when another one opens. */
  readonly handle: string;
  readonly kept: KeptModule;
}

/** The last module edit's message, for the status region. */
export interface ModuleStatus {
  readonly text: string;
  /** Counts the messages, so that the same text said twice is read out twice. */
  readonly serial: number;
}

/** What the module strip shows of the module edits. */
export interface ModuleEditsState {
  /** The deleted module *Undo delete* puts back, or `null`. */
  readonly kept: KeptDeletion | null;
  /** The last edit's message, or `null`. */
  readonly status: ModuleStatus | null;
}

/** What the module edits work with (the app's by default; tests pass their own). */
export interface ModuleEditsDeps {
  readonly store: typeof useAppStore;
  readonly dialogs: DialogService;
  readonly editor: () => EditorHandle | null;
  readonly core: () => CoreWasm | null;
  /** A new module ID (`mod_<random>`); the editor's ID source by default. */
  readonly newModuleId?: () => string;
}

/** The title of the dialog that says why the modules cannot be changed now. */
export const REFUSED_TITLE = 'The modules cannot be changed now';

/** `name` in quotation marks, as messages name a module. */
function quoted(name: string): string {
  return `“${name}”`;
}

/** The module edits of the open project; see the module comment. */
export class ModuleEdits {
  /** What the strip shows; read it with `useModuleEditsState`. */
  readonly state: StoreApi<ModuleEditsState> = createStore<ModuleEditsState>(() => ({
    kept: null,
    status: null,
  }));
  private readonly deps: ModuleEditsDeps;
  private readonly unsubscribe: () => void;
  private serial = 0;

  constructor(deps: ModuleEditsDeps) {
    this.deps = deps;
    // Another project (or none): its kept module and messages go.
    this.unsubscribe = deps.store.subscribe((state, previous) => {
      if (state.project?.handle !== previous.project?.handle) {
        this.state.setState({ kept: null, status: null });
      }
    });
  }

  /** Stops following the store. */
  dispose(): void {
    this.unsubscribe();
  }

  /** Asks for a name and adds an empty module with it, which is then shown. */
  async add(): Promise<void> {
    const doc = this.document();
    if (doc === null || this.refusedNow()) {
      return;
    }
    if (!canAddModule(doc)) {
      this.say(MODULE_EDIT_PROBLEMS.tooMany);
      return;
    }
    const name = await this.askName({
      title: 'Add module',
      message: 'Name of the new module',
      okLabel: 'Add',
      defaultValue: nextModuleName(doc),
      moduleId: null,
    });
    if (name === null) {
      return;
    }
    const second = doc.modules.length === 1;
    const id = (this.deps.newModuleId ?? (() => newId('mod')))();
    if (this.apply((editor) => editor.addModule(id, name))) {
      this.say(
        second
          ? `Module ${quoted(name)} added; the C++ code of the other module changed too, as a project with several modules needs.`
          : `Module ${quoted(name)} added.`,
      );
    }
  }

  /** Asks for a new name for module `id` and renames it; no block changes. */
  async rename(id: string): Promise<void> {
    const doc = this.document();
    const module = doc?.modules.find((candidate) => candidate.id === id);
    if (module === undefined || this.refusedNow()) {
      return;
    }
    const name = await this.askName({
      title: `Rename module ${quoted(module.name)}`,
      message: 'New name of the module',
      okLabel: 'Rename',
      defaultValue: module.name,
      moduleId: id,
    });
    if (name === null || name === module.name) {
      return;
    }
    if (this.apply((editor) => editor.renameModule(id, name))) {
      this.say(`Module ${quoted(module.name)} renamed to ${quoted(name)}.`);
    }
  }

  /**
   * Deletes module `id`, after asking when it holds blocks; it is kept for *Undo delete*. The only
   * module cannot be deleted.
   */
  async remove(id: string): Promise<void> {
    if (this.refusedNow()) {
      return;
    }
    const doc = this.currentDocument();
    const module = doc?.modules.find((candidate) => candidate.id === id);
    if (doc === null || module === undefined) {
      return;
    }
    if (!canDeleteModule(doc)) {
      this.say(MODULE_EDIT_PROBLEMS.onlyModule);
      return;
    }
    const count = topLevelBlockCount(module);
    if (count > 0) {
      const blocks = count === 1 ? 'one block' : `${String(count)} blocks`;
      const start = holdsProgramStart(module)
        ? ' It holds “when program starts”, so the program will have no start until you add one.'
        : '';
      const confirmed = await this.deps.dialogs.confirm({
        title: `Delete module ${quoted(module.name)}?`,
        message: `It holds ${blocks}.${start} You can bring it back with Undo delete until you change the modules again.`,
        confirmLabel: 'Delete module',
        cancelLabel: 'Cancel',
        destructive: true,
      });
      if (!confirmed) {
        return;
      }
    }
    const deleted: { kept: KeptModule | null } = { kept: null };
    const handle = this.deps.store.getState().project?.handle ?? null;
    if (
      this.apply((editor) => {
        const result = editor.deleteModule(id);
        deleted.kept = result.kept;
        return result.outcome;
      }) &&
      deleted.kept !== null &&
      handle !== null
    ) {
      this.state.setState({ kept: { handle, kept: deleted.kept } });
      this.say(`Module ${quoted(module.name)} deleted.`);
    }
  }

  /** Puts the last deleted module back at its place (*Undo delete*). */
  undoDelete(): void {
    const deletion = this.state.getState().kept;
    const project = this.deps.store.getState().project;
    if (deletion === null || project?.handle !== deletion.handle || this.refusedNow()) {
      return;
    }
    if (this.apply((editor) => editor.restoreModule(deletion.kept))) {
      this.say(`Module ${quoted(deletion.kept.module.name)} is back.`);
    }
  }

  /** Moves module `id` one place left (`-1`) or right (`1`). */
  move(id: string, delta: -1 | 1): void {
    const doc = this.document();
    const index = doc?.modules.findIndex((module) => module.id === id) ?? -1;
    const module = doc?.modules[index];
    if (doc === null || module === undefined || this.refusedNow()) {
      return;
    }
    const target = index + delta;
    if (target < 0 || target >= doc.modules.length) {
      return;
    }
    if (this.apply((editor) => editor.moveModule(id, delta))) {
      const place = target === 0 ? ' It is now first, so the program is named after it.' : '';
      this.say(`Module ${quoted(module.name)} moved ${delta < 0 ? 'left' : 'right'}.${place}`);
    }
  }

  /** The open project's document, or `null`. */
  private document(): BdmDocument | null {
    return this.deps.store.getState().project?.document ?? null;
  }

  /** The document as the canvas has it now (its block counts), or the project's. */
  private currentDocument(): BdmDocument | null {
    try {
      return this.deps.editor()?.currentDocument() ?? this.document();
    } catch {
      return this.document();
    }
  }

  /**
   * Whether the canvas holds a change that cannot be read back (the banner, `syncFailed`): the
   * edit is refused, and the banner's text says why.
   */
  private refusedNow(): boolean {
    const { analysis } = this.deps.store.getState();
    if (analysis.notice !== 'syncFailed') {
      return false;
    }
    this.refuse();
    return true;
  }

  /** Says why the modules cannot be changed now, with the banner's text. */
  private refuse(): void {
    const { analysis } = this.deps.store.getState();
    void this.deps.dialogs.alert({
      title: REFUSED_TITLE,
      message: syncFailedText(analysis.syncCodes),
    });
  }

  /**
   * Makes an edit with the editor; a new edit drops the kept deleted module. Returns whether it was
   * made.
   */
  private apply(edit: (editor: EditorHandle) => ModuleEditOutcome): boolean {
    const editor = this.deps.editor();
    if (editor === null) {
      return false;
    }
    const before = this.state.getState().kept;
    const outcome = edit(editor);
    switch (outcome) {
      case 'done':
        if (this.state.getState().kept === before) {
          this.state.setState({ kept: null });
        }
        return true;
      case 'unchecked':
        this.refuse();
        return false;
      case 'busy':
      case 'refused':
        return false;
    }
  }

  /** Asks for a module name, checked as the loader checks it; `null` when cancelled. */
  private async askName(options: {
    readonly title: string;
    readonly message: string;
    readonly okLabel: string;
    readonly defaultValue: string;
    readonly moduleId: string | null;
  }): Promise<string | null> {
    const check = (text: string) => {
      const doc = this.document();
      return doc === null
        ? ({ ok: false, message: 'No project is open.' } as const)
        : checkModuleName(text, doc, options.moduleId, this.deps.core());
    };
    const text = await this.deps.dialogs.prompt({
      title: options.title,
      message: options.message,
      okLabel: options.okLabel,
      defaultValue: options.defaultValue,
      maxLength: 100,
      validate: (value) => {
        const result = check(value);
        return result.ok ? null : result.message;
      },
    });
    if (text === null) {
      return null;
    }
    const result = check(text);
    return result.ok ? result.name : null;
  }

  /** Puts `text` in the status region. */
  private say(text: string): void {
    this.serial += 1;
    this.state.setState({ status: { text, serial: this.serial } });
  }
}

let current: ModuleEdits | null = null;
const listeners = new Set<() => void>();

/** Publishes the module edits (or, with `null`, withdraws them). */
export function setModuleEdits(edits: ModuleEdits | null): void {
  current = edits;
  for (const listener of [...listeners]) {
    listener();
  }
}

/** The module edits, or `null` while the feature is not installed. */
export function getModuleEdits(): ModuleEdits | null {
  return current;
}

/** Calls `listener` when the module edits are published or withdrawn; returns the unsubscriber. */
export function subscribeModuleEdits(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
