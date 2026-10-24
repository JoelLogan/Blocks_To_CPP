/**
 * What other features do through the project lifecycle (docs/spec/04-user-interface.md §4.10,
 * 05 §5.10): the lifecycle runs one operation at a time, and an operation of another feature that
 * replaces, re-reads or races the open project must take its place in that queue too.
 *
 * - The recovery feature restores a snapshot with {@link ProjectQueue.replaceWith}, so the start
 *   page's New and Open wait for it (and the reverse), and the open project's unsaved changes are
 *   settled with the same prompt as for New or Open. Its autosave pauses while a save runs
 *   ({@link ProjectQueue.isSaving}) and makes every save wait for a snapshot it is writing
 *   ({@link ProjectQueue.addSaveBarrier}), so a snapshot never reaches the backend after the save
 *   that deleted it.
 * - The external-change feature runs Reload with {@link ProjectQueue.reloadWith}, so a save never
 *   writes the canvas over a file that is being read again. A reload deletes the snapshot too, so
 *   it pauses autosave and waits for a snapshot being written like a save.
 *
 * The app's features are created before they are installed, so `src/features/index.ts` hands the
 * other features a {@link ProjectLink}, which the project feature binds to its lifecycle while it
 * is installed.
 */
import type { Handle } from '@blocks2cpp/ipc-types';

/** The project lifecycle as other features use it; see the module comment. */
export interface ProjectQueue {
  /**
   * Replaces the open project with the one `show` shows, in the queue: the open project's unsaved
   * changes are settled first (*Save*, *Don't save* or *Cancel*), then `show` runs and resolves
   * the handle of the project it showed, or `null` when it showed none (having told the user
   * why). The replaced project is closed only once the new one is shown. Resolves whether it was.
   */
  replaceWith(show: () => Promise<Handle | null>): Promise<boolean>;
  /** Runs `reload` (`project_reload` and showing the file again) in the queue; resolves its result. */
  reloadWith(reload: () => Promise<boolean>): Promise<boolean>;
  /**
   * Whether a save (Save, Save as, or the save of an unsaved-changes prompt) or a reload is under
   * way: both make the backend delete the recovery snapshot.
   */
  isSaving(): boolean;
  /**
   * Adds `wait`, which every save and reload awaits before it sends the project to the backend.
   * Returns the function that removes it again.
   */
  addSaveBarrier(wait: () => Promise<void>): () => void;
}

/**
 * A {@link ProjectQueue} for features created before the project feature is installed: it forwards
 * to the lifecycle bound to it (`createProjectFeature({ link })` binds it while installed). Without
 * one, operations do nothing and resolve `false`, with an error in the log.
 */
export class ProjectLink implements ProjectQueue {
  #lifecycle: ProjectQueue | null = null;
  readonly #barriers = new Set<() => Promise<void>>();

  /** Forwards to `lifecycle` until the returned function is called. */
  bind(lifecycle: ProjectQueue): () => void {
    this.#lifecycle = lifecycle;
    const removeBarrier = lifecycle.addSaveBarrier(() => this.#passBarriers());
    return () => {
      removeBarrier();
      if (this.#lifecycle === lifecycle) {
        this.#lifecycle = null;
      }
    };
  }

  replaceWith(show: () => Promise<Handle | null>): Promise<boolean> {
    return this.#lifecycle?.replaceWith(show) ?? unbound('replace the open project');
  }

  reloadWith(reload: () => Promise<boolean>): Promise<boolean> {
    return this.#lifecycle?.reloadWith(reload) ?? unbound('reload the open project');
  }

  isSaving(): boolean {
    return this.#lifecycle?.isSaving() ?? false;
  }

  /** Kept here, so a barrier added before the lifecycle is bound still applies once it is. */
  addSaveBarrier(wait: () => Promise<void>): () => void {
    const entry = () => wait();
    this.#barriers.add(entry);
    return () => {
      this.#barriers.delete(entry);
    };
  }

  #passBarriers(): Promise<void> {
    return Promise.all([...this.#barriers].map((wait) => wait())).then(() => undefined);
  }
}

/** What an operation without a lifecycle resolves: nothing happened. */
function unbound(what: string): Promise<boolean> {
  console.error(`No project lifecycle is installed, so Blocks2Cpp could not ${what}`);
  return Promise.resolve(false);
}
