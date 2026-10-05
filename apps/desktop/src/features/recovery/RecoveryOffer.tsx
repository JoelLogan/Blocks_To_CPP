/**
 * The recovery offer on the start page (docs/spec/04-user-interface.md §4.10): after Blocks2Cpp
 * stopped with unsaved changes (a crash, or the computer shutting down), the snapshots of those
 * sessions are listed here, each with *Restore* and *Discard*. With nothing to offer, the section
 * renders nothing.
 */
import type { SnapshotInfo } from '@blocks2cpp/ipc-types';
import { useId } from 'react';
import { useStore } from 'zustand';

import type { RecoveryController } from './controller';
import type { RecoveryModel, RecoveryOperation } from './model';
import { localDateTime, shownName } from './text';
import './recovery.css';

/** What the offer needs. */
export interface RecoveryOfferProps {
  readonly model: RecoveryModel;
  readonly controller: RecoveryController;
}

/** Runs an operation from a click; its errors are shown by the controller itself. */
function run(task: () => Promise<unknown>): void {
  task().catch((error: unknown) => {
    console.error('A recovery operation failed', error);
  });
}

/** What the status line says while an operation runs. */
function busyText(busy: RecoveryOperation | null, snapshots: readonly SnapshotInfo[]): string {
  if (busy === null) {
    return '';
  }
  const snapshot = snapshots.find((candidate) => candidate.snapshotId === busy.snapshotId);
  const name = snapshot === undefined ? 'the project' : `“${shownName(snapshot.projectName)}”`;
  return busy.kind === 'restore' ? `Restoring ${name}…` : `Discarding the unsaved work on ${name}…`;
}

/** The recovery offer; see the module comment. */
export function RecoveryOffer({ model, controller }: RecoveryOfferProps) {
  const status = useStore(model, (state) => state.status);
  const snapshots = useStore(model, (state) => state.snapshots);
  const busy = useStore(model, (state) => state.busy);
  const titleId = useId();
  const introId = useId();

  if (status === 'loading' || (status === 'ready' && snapshots.length === 0 && busy === null)) {
    return null;
  }

  if (status === 'failed') {
    return (
      <section className="recovery-offer" aria-labelledby={titleId} data-testid="recovery-offer">
        <h3 id={titleId} className="recovery-title">
          Unsaved work
        </h3>
        <p className="recovery-note">
          Blocks2Cpp could not check for unsaved work from an earlier session.{' '}
          <button
            type="button"
            className="button"
            onClick={() => {
              run(() => controller.refresh());
            }}
          >
            Try again
          </button>
        </p>
      </section>
    );
  }

  const idle = busy === null;
  return (
    <section
      className="recovery-offer"
      aria-labelledby={titleId}
      aria-describedby={introId}
      data-testid="recovery-offer"
    >
      <h3 id={titleId} className="recovery-title">
        Recover unsaved work
      </h3>
      <p id={introId} className="recovery-note">
        {snapshots.length === 1
          ? 'Blocks2Cpp closed before this project was saved. Restore it to carry on where you left off, or discard it.'
          : 'Blocks2Cpp closed before these projects were saved. Restore one to carry on where you left off, or discard it.'}
      </p>
      {snapshots.length > 0 && (
        <ul className="recovery-list">
          {snapshots.map((snapshot) => (
            <RecoveryItem
              key={snapshot.snapshotId}
              snapshot={snapshot}
              idle={idle}
              controller={controller}
            />
          ))}
        </ul>
      )}
      <p className="recovery-status" role="status">
        {busyText(busy, snapshots)}
      </p>
    </section>
  );
}

/** One snapshot: its project, when it was saved, and the two actions. */
function RecoveryItem({
  snapshot,
  idle,
  controller,
}: {
  snapshot: SnapshotInfo;
  idle: boolean;
  controller: RecoveryController;
}) {
  const detailsId = useId();
  const name = shownName(snapshot.projectName);
  const saved = localDateTime(snapshot.savedAt);
  return (
    <li className="recovery-item" data-testid="recovery-item">
      <div className="recovery-text">
        <span className="recovery-name">
          <bdi>{name}</bdi>
        </span>
        <span id={detailsId} className="recovery-details">
          {saved === null ? (
            'Saved automatically'
          ) : (
            <>
              Saved automatically on <time dateTime={snapshot.savedAt}>{saved}</time>
            </>
          )}
          {snapshot.hasPath ? '' : ' · never saved to a file'}
        </span>
      </div>
      <div className="recovery-actions">
        <button
          type="button"
          className="button button-primary"
          aria-label={`Restore “${name}”`}
          aria-describedby={detailsId}
          aria-disabled={!idle}
          onClick={() => {
            if (idle) {
              run(() => controller.restore(snapshot.snapshotId));
            }
          }}
        >
          Restore
        </button>
        <button
          type="button"
          className="button"
          aria-label={`Discard “${name}”`}
          aria-describedby={detailsId}
          aria-disabled={!idle}
          onClick={() => {
            if (idle) {
              run(() => controller.discard(snapshot.snapshotId));
            }
          }}
        >
          Discard
        </button>
      </div>
    </li>
  );
}
