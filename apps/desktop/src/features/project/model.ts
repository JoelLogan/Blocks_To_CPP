/**
 * The project feature's own state, which only its start page shows: the recent-projects list, the
 * problems of the last file that did not load, and which lifecycle operation is running. The open
 * project itself lives in the app's store (`project`).
 */
import type { RecentEntry } from '@blocks2cpp/ipc-types';
import { createStore, type StoreApi } from 'zustand/vanilla';

import type { LoadProblems } from './errors';

/** The most recent-list entries shown (the backend keeps at most 10, 04 §4.10). */
export const MAX_RECENT_ENTRIES = 10;

/** Where the recent-projects list is. */
export type RecentStatus = 'idle' | 'loading' | 'ready' | 'failed';

/** The recent-projects list, newest first. */
export interface RecentState {
  readonly status: RecentStatus;
  readonly entries: readonly RecentEntry[];
}

/** A file that did not load, shown on the start page until it is dismissed or a project opens. */
export interface LoadFailureState extends LoadProblems {
  /** What could not be opened, for the heading (already safe for display), or `null`. */
  readonly name: string | null;
}

/**
 * A project lifecycle operation; they run one at a time. `restore` (a recovery snapshot replaces
 * the open project) and `reload` (the file is read again after an outside change) are run for the
 * recovery and external-change features.
 */
export type LifecycleOperation =
  'new' | 'open' | 'save' | 'saveAs' | 'close' | 'quit' | 'restore' | 'reload';

/** The feature's state. */
export interface ProjectFeatureState {
  readonly recent: RecentState;
  readonly loadFailure: LoadFailureState | null;
  /** The operation running now, or `null`. */
  readonly busy: LifecycleOperation | null;
}

/** The feature's store. */
export type ProjectModel = StoreApi<ProjectFeatureState>;

/** A new store: no list read yet, no failure, nothing running. */
export function createProjectModel(): ProjectModel {
  return createStore<ProjectFeatureState>()(() => ({
    recent: { status: 'idle', entries: [] },
    loadFailure: null,
    busy: null,
  }));
}

/** Whether `value` is a well-formed recent-list entry (the channel's data is checked before use). */
function isRecentEntry(value: unknown): value is RecentEntry {
  if (typeof value !== 'object' || value === null) {
    return false;
  }
  const entry = value as Record<string, unknown>;
  const id = entry['recentId'];
  return (
    typeof id === 'string' &&
    /^rc_[0-9a-f]{32}$/.test(id) &&
    typeof entry['projectName'] === 'string' &&
    typeof entry['displayPath'] === 'string' &&
    typeof entry['lastOpenedAt'] === 'string'
  );
}

/**
 * The backend's entries as the start page lists them: well-formed ones only, without repeated
 * IDs, at most {@link MAX_RECENT_ENTRIES}.
 */
export function shownRecentEntries(entries: readonly unknown[]): RecentEntry[] {
  const seen = new Set<string>();
  const shown: RecentEntry[] = [];
  for (const entry of entries) {
    if (shown.length >= MAX_RECENT_ENTRIES) {
      break;
    }
    if (!isRecentEntry(entry) || seen.has(entry.recentId)) {
      continue;
    }
    seen.add(entry.recentId);
    shown.push(entry);
  }
  return shown;
}
