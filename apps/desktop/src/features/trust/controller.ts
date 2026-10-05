/**
 * The trust feature's logic (docs/spec/08-security.md §8.3, 04 §4.10): *Trust…* asks the backend
 * to raise its native trust dialog (`trust_grant`), and *Revoke trust* forgets the project's own
 * trust record (`trust_revoke`). The webview never decides trust; it only shows the state the
 * backend answers with, and the backend checks trust again before every build and run.
 */
import type { Handle, Trust } from '@blocks2cpp/ipc-types';
import { createStore, type StoreApi } from 'zustand/vanilla';

import type { FeatureContext } from '../../app/features';
import { type FailureCode, failureCode } from '../settings/page/ipcErrors';

/** What the person asked for. */
export type TrustAction = 'grant' | 'revoke';

/** What the last action found, for the project it was about. */
export type TrustMessage =
  /** The trust dialog was answered with *Stay in Restricted Mode*, or closed. */
  | { kind: 'stayedRestricted' }
  /** The project is trusted now. */
  | { kind: 'granted' }
  /** The project's record was removed and it is restricted now. */
  | { kind: 'revoked' }
  /** The record was removed, but the project's folder is trusted, so the project still is. */
  | { kind: 'stillTrustedByFolder' }
  /** The action failed. */
  | { kind: 'failed'; action: TrustAction; code: FailureCode };

/** The trust feature's state. */
export interface TrustState {
  /** The action in progress, or `null`. */
  busy: TrustAction | null;
  /** The last message and the project it is about, or `null`. */
  message: { handle: Handle; message: TrustMessage } | null;
}

/** The trust feature's controller; one per installation. */
export class TrustController {
  readonly state: StoreApi<TrustState>;
  private readonly ctx: FeatureContext;
  private disposed = false;
  private readonly unsubscribe: () => void;

  constructor(ctx: FeatureContext) {
    this.ctx = ctx;
    this.state = createStore<TrustState>()(() => ({ busy: null, message: null }));
    // A message is about one project: forget it when another one is opened.
    this.unsubscribe = ctx.store.subscribe((state, previous) => {
      if (state.project?.handle !== previous.project?.handle) {
        this.state.setState({ message: null });
      }
    });
  }

  /** Stops: answers that arrive later are ignored. */
  dispose(): void {
    this.disposed = true;
    this.unsubscribe();
  }

  /**
   * *Trust…*: the backend asks in its native dialog (Trust this project / Trust everything in
   * this folder / Stay in Restricted Mode) and answers with the trust after the choice; a cancel
   * leaves it unchanged. Resolves with the new trust, or `null` when nothing was asked or it
   * failed.
   */
  grant(): Promise<Trust | null> {
    return this.act('grant', async (handle) => {
      const { trust } = await this.ctx.ipc.trustGrant({ handle });
      return {
        trust,
        message: trust.state === 'trusted' ? { kind: 'granted' } : { kind: 'stayedRestricted' },
      };
    });
  }

  /**
   * *Revoke trust*: removes the project's own trust record. A project in a trusted folder stays
   * trusted (the message says why). Resolves with the new trust, or `null` when it failed.
   */
  revoke(): Promise<Trust | null> {
    return this.act('revoke', async (handle) => {
      const { trust } = await this.ctx.ipc.trustRevoke({ handle });
      return {
        trust,
        message: trust.state === 'trusted' ? { kind: 'stillTrustedByFolder' } : { kind: 'revoked' },
      };
    });
  }

  /** Whether {@link dispose} was called (a method, so checks after an `await` are not narrowed). */
  private isDisposed(): boolean {
    return this.disposed;
  }

  /** Runs one action at a time for the open project and applies its answer to that project. */
  private async act(
    action: TrustAction,
    body: (handle: Handle) => Promise<{ trust: Trust; message: TrustMessage }>,
  ): Promise<Trust | null> {
    const project = this.ctx.store.getState().project;
    if (project === null || this.state.getState().busy !== null || this.disposed) {
      return null;
    }
    const { handle } = project;
    this.state.setState({ busy: action, message: null });
    try {
      const { trust, message } = await body(handle);
      if (this.isDisposed()) {
        return null;
      }
      const state = this.ctx.store.getState();
      // The project may have been closed or replaced while the dialog was open.
      if (state.project?.handle === handle) {
        state.actions.updateProject({ trust });
        this.state.setState({ message: { handle, message } });
      }
      return trust;
    } catch (error: unknown) {
      const code = failureCode(error);
      console.error(`The trust action ${action} failed`, code);
      if (!this.isDisposed() && this.ctx.store.getState().project?.handle === handle) {
        this.state.setState({ message: { handle, message: { kind: 'failed', action, code } } });
      }
      return null;
    } finally {
      if (!this.isDisposed()) {
        this.state.setState({ busy: null });
      }
    }
  }
}
