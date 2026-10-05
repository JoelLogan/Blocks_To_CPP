/**
 * The state of the recovery offer on the start page (docs/spec/04-user-interface.md §4.10): the
 * snapshots of earlier sessions that `recovery_list` offers for restore, and the operation that is
 * running on one of them.
 */
import type { SnapshotId, SnapshotInfo } from '@blocks2cpp/ipc-types';
import { createStore, type StoreApi } from 'zustand/vanilla';

/**
 * The most snapshots the offer lists. The backend offers at most 100 (05 §5.10); anything beyond
 * that is dropped rather than rendered.
 */
export const MAX_OFFERED_SNAPSHOTS = 100;

/** The longest project name kept from the backend, in UTF-16 code units (it keeps 1 KiB). */
export const MAX_SNAPSHOT_NAME_CHARS = 1024;

/** A snapshot ID as the backend gives them out: `sn_` and 32 lower-case hex digits. */
const SNAPSHOT_ID = /^sn_[0-9a-f]{32}$/;

/** The longest `savedAt` kept (an RFC 3339 timestamp is about 30 characters). */
const MAX_TIMESTAMP_CHARS = 64;

/** Whether the list has been read. */
export type OfferStatus = 'loading' | 'ready' | 'failed';

/** An operation on one snapshot. */
export interface RecoveryOperation {
  readonly kind: 'restore' | 'discard';
  readonly snapshotId: SnapshotId;
}

/** The offer's state. */
export interface RecoveryOfferState {
  /** `loading` until `recovery_list` answered; `failed` when it could not be read. */
  readonly status: OfferStatus;
  /** The snapshots to offer, newest first (the backend's order). */
  readonly snapshots: readonly SnapshotInfo[];
  /** The operation running, or `null`: one at a time. */
  readonly busy: RecoveryOperation | null;
}

/** The offer's store. */
export type RecoveryModel = StoreApi<RecoveryOfferState>;

/** A store with nothing read yet. */
export function createRecoveryModel(): RecoveryModel {
  return createStore<RecoveryOfferState>()(() => ({
    status: 'loading',
    snapshots: [],
    busy: null,
  }));
}

/** Whether `value` is a well-formed snapshot description. */
function isSnapshotInfo(value: unknown): value is SnapshotInfo {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return false;
  }
  const entry = value as Record<string, unknown>;
  return (
    typeof entry['snapshotId'] === 'string' &&
    SNAPSHOT_ID.test(entry['snapshotId']) &&
    typeof entry['projectName'] === 'string' &&
    typeof entry['savedAt'] === 'string' &&
    entry['savedAt'].length <= MAX_TIMESTAMP_CHARS &&
    typeof entry['hasPath'] === 'boolean'
  );
}

/**
 * The snapshots of a `recovery_list` answer that the offer shows: well-formed entries only, each
 * ID once, at most {@link MAX_OFFERED_SNAPSHOTS}, in the backend's order (newest first), with
 * names cut to {@link MAX_SNAPSHOT_NAME_CHARS}. The backend ships with the app, but its answers
 * are checked before use like any other input.
 */
export function offeredSnapshots(list: unknown): SnapshotInfo[] {
  if (!Array.isArray(list)) {
    return [];
  }
  const seen = new Set<string>();
  const offered: SnapshotInfo[] = [];
  for (const entry of list as unknown[]) {
    if (offered.length >= MAX_OFFERED_SNAPSHOTS) {
      break;
    }
    if (!isSnapshotInfo(entry) || seen.has(entry.snapshotId)) {
      continue;
    }
    seen.add(entry.snapshotId);
    offered.push({
      snapshotId: entry.snapshotId,
      projectName: entry.projectName.slice(0, MAX_SNAPSHOT_NAME_CHARS),
      savedAt: entry.savedAt,
      hasPath: entry.hasPath,
    });
  }
  return offered;
}

/** Removes `snapshotId` from the offer. */
export function withoutSnapshot(model: RecoveryModel, snapshotId: SnapshotId): void {
  model.setState((state) => ({
    snapshots: state.snapshots.filter((snapshot) => snapshot.snapshotId !== snapshotId),
  }));
}
