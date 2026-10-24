/**
 * The screen registry: the full-window pages besides the block editor (the start page, the
 * toolchain setup page and the Settings page, docs/spec/04-user-interface.md §4.6, §4.10, §4.12).
 * Features register the component for a page; the shell shows it while `ui.screen` names it.
 *
 * Like commands, registrations stack: the newest component for a screen is shown, and disposing it
 * brings the previous one back. A screen with no component falls back to the editor, so no entry
 * point leads to an empty page (§4.13).
 */
import { type ComponentType, useSyncExternalStore } from 'react';

import type { ScreenId } from './store';

/** The screens a feature can provide; the editor itself belongs to the shell. */
export type RegisteredScreenId = Exclude<ScreenId, 'editor'>;

/** The registry behind the module's functions; tests and the feature context use it directly. */
export interface ScreenRegistry {
  /** Registers `component` for `id` and returns the function that removes this registration. */
  registerScreen(id: RegisteredScreenId, component: ComponentType): () => void;
  /** The component to show for `id`, or `null` when none is registered. */
  screen(id: ScreenId): ComponentType | null;
  /** Calls `listener` whenever a registration is added or removed. Returns the unsubscriber. */
  subscribe: (listener: () => void) => () => void;
}

interface Registration {
  readonly component: ComponentType;
}

/** Creates an empty registry. */
export function createScreenRegistry(): ScreenRegistry {
  const registrations = new Map<RegisteredScreenId, Registration[]>();
  const listeners = new Set<() => void>();

  function notify(): void {
    for (const listener of [...listeners]) {
      listener();
    }
  }

  return {
    registerScreen(id, component) {
      const entry: Registration = { component };
      registrations.set(id, [...(registrations.get(id) ?? []), entry]);
      notify();
      let registered = true;
      return () => {
        if (!registered) {
          return;
        }
        registered = false;
        const remaining = (registrations.get(id) ?? []).filter((candidate) => candidate !== entry);
        if (remaining.length === 0) {
          registrations.delete(id);
        } else {
          registrations.set(id, remaining);
        }
        notify();
      };
    },

    screen(id) {
      if (id === 'editor') {
        return null;
      }
      return registrations.get(id)?.at(-1)?.component ?? null;
    },

    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/** The app's screen registry. */
export const screens: ScreenRegistry = createScreenRegistry();

/** Registers a screen with the app's registry; see {@link ScreenRegistry.registerScreen}. */
export function registerScreen(id: RegisteredScreenId, component: ComponentType): () => void {
  return screens.registerScreen(id, component);
}

/** The component registered for `id`, as React state that follows registrations. */
export function useRegisteredScreen(
  id: ScreenId,
  registry: ScreenRegistry = screens,
): ComponentType | null {
  return useSyncExternalStore(registry.subscribe, () => registry.screen(id));
}
