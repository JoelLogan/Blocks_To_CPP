/**
 * The toolchain feature's logic, apart from its page: keeping the store's toolchain list current,
 * the setup information, the page's actions (*Rescan*, *Choose g++ manually…*, *Select as
 * default*) and showing the setup page when no usable compiler is found
 * (docs/spec/04-user-interface.md §4.6, 07 §7.2).
 *
 * The bootstrap already reads the list once and applies every `toolchainsUpdated` event to the
 * store. The controller's own reads never overwrite something newer: an answer is applied only
 * when nothing changed the list while it was on its way.
 */
import type { Diagnostic, Toolchain, ToolchainId } from '@blocks2cpp/ipc-types';
import { createStore, type StoreApi } from 'zustand/vanilla';

import type { FeatureContext } from '../../app/features';
import type { AppState } from '../../app/store';
import { hasUsableToolchain } from '../../app/store/selectors';
import { type FailureCode, failureCode, rejectionDiagnostics } from '../settings/page/ipcErrors';

/** An action of the page that talks to the backend. */
export type ToolchainAction = 'rescan' | 'add' | 'select' | 'openLink';

/** What the last action of the page found, shown under the actions. */
export type ToolchainOutcome =
  /** *Rescan* finished: how many compilers were found and how many can build. */
  | { kind: 'rescanned'; found: number; usable: number }
  /** *Choose g++ manually…* added this compiler (usable or not). */
  | { kind: 'added'; toolchain: Toolchain }
  /** *Choose g++ manually…* refused the file; nothing was added. */
  | { kind: 'rejected'; diagnostics: Diagnostic[] }
  /** This compiler is now the default. */
  | { kind: 'selected'; toolchain: Toolchain }
  /** An action failed with `code`; `link` is the page that could not be opened. */
  | { kind: 'failed'; action: ToolchainAction; code: FailureCode; link?: SetupLink };

/** The page's own state, kept while the page is closed. */
export interface ToolchainPageState {
  /** The action in progress, or `null`. One runs at a time. */
  busy: ToolchainAction | null;
  /** The toolchain being selected, while `busy` is `select`. */
  selecting: ToolchainId | null;
  /** What the last action found, or `null`. */
  outcome: ToolchainOutcome | null;
}

/** Whether the setup page is needed: discovery has finished and no compiler can build. */
export function needsSetup(state: Pick<AppState, 'toolchains'>): boolean {
  return !state.toolchains.discovering && !hasUsableToolchain(state);
}

/** The links the setup page opens in the system browser. */
export type SetupLink = 'msys2Install' | 'winlibs';

/** The toolchain feature's controller; one per installation. */
export class ToolchainController {
  /** The page's state, as a store the page subscribes to. */
  readonly page: StoreApi<ToolchainPageState>;

  private readonly ctx: FeatureContext;
  /** Increased by every change of the list; a read applies only if it is unchanged. */
  private listVersion = 0;
  /** Whether the setup page was shown since the last time a usable compiler was known. */
  private setupShown = false;
  /** Whether the first read finished, so the store holds an answer worth judging. */
  private ready = false;
  private readonly disposers: (() => void)[] = [];
  private disposed = false;

  constructor(ctx: FeatureContext) {
    this.ctx = ctx;
    this.page = createStore<ToolchainPageState>()(() => ({
      busy: null,
      selecting: null,
      outcome: null,
    }));
  }

  /**
   * Starts: listens for list updates, reads the list and the setup information, and from then on
   * shows the setup page whenever discovery ends without a usable compiler.
   */
  start(): void {
    this.disposers.push(
      this.ctx.events.on('toolchainsUpdated', () => {
        // The bootstrap has put the event's list into the store already.
        this.listVersion += 1;
      }),
      this.ctx.store.subscribe((state) => {
        this.considerSetupPage(state);
      }),
    );
    void this.loadSetupInfo();
    void this.refreshList().finally(() => {
      this.ready = true;
      this.considerSetupPage(this.ctx.store.getState());
    });
  }

  /** Stops listening; answers that arrive later are ignored. */
  dispose(): void {
    this.disposed = true;
    for (const dispose of this.disposers.splice(0)) {
      dispose();
    }
  }

  /**
   * Reads the list (`toolchain_list`) and puts it into the store, unless the list changed in the
   * meantime (an event, a rescan, a selection), which is newer.
   */
  async refreshList(): Promise<void> {
    const version = this.listVersion;
    try {
      const response = await this.ctx.ipc.toolchainList();
      if (this.disposed || version !== this.listVersion) {
        return;
      }
      this.applyList(response.toolchains, response.discovering);
    } catch (error: unknown) {
      console.error('Could not read the toolchain list', failureCode(error));
    }
  }

  /** Reads what the setup page needs (`toolchain_setup_info`) into the store. */
  async loadSetupInfo(): Promise<void> {
    try {
      const setupInfo = await this.ctx.ipc.toolchainSetupInfo();
      if (!this.disposed) {
        this.ctx.store.getState().actions.setToolchains({ setupInfo });
      }
    } catch (error: unknown) {
      // The page falls back to the platform from app_info and shows every Linux command.
      console.error('Could not read the toolchain setup information', failureCode(error));
    }
  }

  /** *I installed it → Rescan*: discovers and probes again (`toolchain_rescan`). */
  rescan(): Promise<void> {
    return this.run('rescan', async () => {
      const { actions, toolchains } = this.ctx.store.getState();
      const wasDiscovering = toolchains.discovering;
      actions.setToolchains({ discovering: true });
      try {
        const response = await this.ctx.ipc.toolchainRescan();
        this.applyList(response.toolchains, response.discovering);
        return {
          kind: 'rescanned',
          found: response.toolchains.length,
          usable: response.toolchains.filter((toolchain) => toolchain.usable).length,
        };
      } catch (error: unknown) {
        // Put the list (and `discovering`) back to what the backend knows, or else to what it
        // was before.
        const version = this.listVersion;
        await this.refreshList();
        if (version === this.listVersion && !this.disposed) {
          this.ctx.store.getState().actions.setToolchains({ discovering: wasDiscovering });
        }
        throw error;
      }
    });
  }

  /**
   * *Choose g++ manually…*: the backend's native file dialog (`toolchain_add_dialog`). A cancelled
   * dialog changes nothing and leaves the last outcome in place.
   */
  addManually(): Promise<void> {
    return this.run('add', async () => {
      const response = await this.ctx.ipc.toolchainAddDialog();
      if (response.status === 'cancelled') {
        return 'keep';
      }
      await this.refreshList();
      return { kind: 'added', toolchain: response.toolchain };
    });
  }

  /** *Select as default* (`toolchain_select`). */
  select(id: ToolchainId): Promise<void> {
    return this.run(
      'select',
      async () => {
        await this.ctx.ipc.toolchainSelect({ toolchainId: id });
        const { list } = this.ctx.store.getState().toolchains;
        const updated = list.map((toolchain) => ({ ...toolchain, selected: toolchain.id === id }));
        this.applyList(updated, this.ctx.store.getState().toolchains.discovering);
        const chosen = updated.find((toolchain) => toolchain.id === id);
        void this.refreshList();
        return chosen === undefined ? null : { kind: 'selected', toolchain: chosen };
      },
      id,
    );
  }

  /** Opens one of the install pages in the system browser (`open_help_link`). */
  openLink(linkId: SetupLink): Promise<void> {
    return this.run('openLink', async () => {
      try {
        await this.ctx.ipc.openHelpLink({ linkId });
        return 'keep';
      } catch (error: unknown) {
        console.error('Could not open a help link', failureCode(error));
        return { kind: 'failed', action: 'openLink', code: failureCode(error), link: linkId };
      }
    });
  }

  /** Clears the outcome (for example when the page is left). */
  clearOutcome(): void {
    this.page.setState({ outcome: null });
  }

  /** Puts a list into the store and counts it as a change. */
  private applyList(list: Toolchain[], discovering: boolean): void {
    this.listVersion += 1;
    this.ctx.store.getState().actions.setToolchains({ list, discovering });
  }

  /**
   * Runs one action at a time: while one runs, others are ignored. The result becomes the
   * outcome (`keep` leaves the previous one); a failure becomes a `failed` or `rejected` outcome.
   */
  private async run(
    action: ToolchainAction,
    body: () => Promise<ToolchainOutcome | 'keep' | null>,
    selecting: ToolchainId | null = null,
  ): Promise<void> {
    if (this.page.getState().busy !== null || this.disposed) {
      return;
    }
    this.page.setState({ busy: action, selecting });
    let outcome: ToolchainOutcome | 'keep' | null;
    try {
      outcome = await body();
    } catch (error: unknown) {
      const diagnostics = rejectionDiagnostics(error);
      outcome =
        diagnostics === null
          ? { kind: 'failed', action, code: failureCode(error) }
          : { kind: 'rejected', diagnostics };
      if (diagnostics === null) {
        console.error(`The toolchain action ${action} failed`, failureCode(error));
      }
      if (action === 'select' && failureCode(error) === 'unknownToolchain') {
        void this.refreshList();
      }
    }
    if (outcome === 'keep') {
      this.page.setState({ busy: null, selecting: null });
    } else {
      this.page.setState({ busy: null, selecting: null, outcome });
    }
  }

  /**
   * Shows the setup page when discovery has ended without a usable compiler, once per such
   * episode: leaving the page keeps it closed until a usable compiler has been seen again. It only
   * replaces the start page or the editor, never another page the person opened.
   */
  private considerSetupPage(state: AppState): void {
    if (this.disposed || !this.ready) {
      return;
    }
    if (hasUsableToolchain(state)) {
      this.setupShown = false;
      return;
    }
    if (!needsSetup(state) || this.setupShown) {
      return;
    }
    this.setupShown = true;
    const { screen } = state.ui;
    if (
      (screen === 'start' || screen === 'editor') &&
      this.ctx.screens.screen('toolchainSetup') !== null
    ) {
      state.actions.setUi({ screen: 'toolchainSetup' });
    }
  }
}
