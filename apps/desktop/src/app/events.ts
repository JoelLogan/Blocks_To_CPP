/**
 * The app event bus: how features hear about things nobody asked for. The backend's push events
 * (`app_subscribe`, docs/spec/02-architecture.md §2.5.3) arrive here once, from the bootstrap, and
 * features can also tell each other about local events, such as a save that found the file
 * changed on disk.
 */
import type { AppEvent, Handle } from '@blocks2cpp/ipc-types';

/**
 * Events that start in the frontend. `project:changedOnDisk`: `project_save` was refused with
 * `changedOnDisk`, so the external-change feature takes over (04 §4.10).
 */
export interface LocalAppEvent {
  kind: 'project:changedOnDisk';
  handle: Handle;
}

/** Every event on the bus. */
export type BusEvent = AppEvent | LocalAppEvent;

/** The event of one kind. */
export type BusEventOf<K extends BusEvent['kind']> = Extract<BusEvent, { kind: K }>;

/** The app event bus. */
export interface AppEventBus {
  /** Calls `handler` for every event of `kind`, until the returned function is called. */
  on<K extends BusEvent['kind']>(kind: K, handler: (event: BusEventOf<K>) => void): () => void;
  /**
   * Delivers `event` to its handlers, in the order they subscribed. A handler that throws is
   * logged and does not stop the others.
   */
  emit(event: BusEvent): void;
}

type AnyHandler = (event: BusEvent) => void;

/** Creates a bus with no handlers. */
export function createAppEventBus(): AppEventBus {
  const handlers = new Map<BusEvent['kind'], Set<AnyHandler>>();

  return {
    on(kind, handler) {
      const entry: AnyHandler = (event) => {
        handler(event as BusEventOf<typeof kind>);
      };
      const set = handlers.get(kind) ?? new Set<AnyHandler>();
      set.add(entry);
      handlers.set(kind, set);
      return () => {
        set.delete(entry);
      };
    },

    emit(event) {
      for (const handler of [...(handlers.get(event.kind) ?? [])]) {
        try {
          handler(event);
        } catch (error: unknown) {
          console.error(`A handler of the ${event.kind} event failed`, error);
        }
      }
    },
  };
}

/**
 * Whether a message from the app-event channel is an event this frontend knows, with fields of
 * the right types. The backend ships with the frontend, but the channel's messages are checked
 * before use like any other input.
 */
export function isAppEvent(message: unknown): message is AppEvent {
  if (typeof message !== 'object' || message === null || Array.isArray(message)) {
    return false;
  }
  const event = message as Record<string, unknown>;
  switch (event['kind']) {
    case 'projectChangedOnDisk':
      return (
        typeof event['handle'] === 'string' &&
        event['handle'].startsWith('ph_') &&
        typeof event['deleted'] === 'boolean'
      );
    case 'toolchainsUpdated':
      return Array.isArray(event['toolchains']) && typeof event['discovering'] === 'boolean';
    case 'settingsNotice':
      return Array.isArray(event['notices']);
    case 'closeRequested':
      return true;
    default:
      return false;
  }
}
