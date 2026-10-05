/**
 * Extra sections of the Settings page, contributed by other features (the trust feature's *This
 * project* section). A section component renders `null` when it has nothing to show. Sections are
 * shown after the page's own ones, in ascending `order`, then in registration order.
 */
import { type ComponentType, useSyncExternalStore } from 'react';

/** A registered section. */
export interface SettingsSection {
  readonly id: string;
  readonly component: ComponentType;
  readonly order: number;
}

/** The registry behind {@link registerSettingsSection}. */
export interface SettingsSectionRegistry {
  /** Registers `component` as the section `id`; returns the function that removes it again. */
  registerSection(id: string, component: ComponentType, options?: { order?: number }): () => void;
  /** The sections, in display order (one per ID: the newest registration). */
  sections: () => readonly SettingsSection[];
  /** Calls `listener` whenever a section is added or removed. Returns the unsubscriber. */
  subscribe: (listener: () => void) => () => void;
}

/** Creates an empty registry. */
export function createSettingsSectionRegistry(): SettingsSectionRegistry {
  let registrations: SettingsSection[] = [];
  let shown: readonly SettingsSection[] = [];
  const listeners = new Set<() => void>();

  function update(): void {
    const newest = new Map<string, SettingsSection>();
    for (const entry of registrations) {
      newest.set(entry.id, entry);
    }
    const position = new Map(registrations.map((entry, index) => [entry, index]));
    shown = [...newest.values()].sort(
      (a, b) => a.order - b.order || (position.get(a) ?? 0) - (position.get(b) ?? 0),
    );
    for (const listener of [...listeners]) {
      listener();
    }
  }

  return {
    registerSection(id, component, options = {}) {
      const order = options.order ?? 0;
      if (!Number.isFinite(order)) {
        throw new RangeError('A section order must be a finite number');
      }
      const entry: SettingsSection = { id, component, order };
      registrations = [...registrations, entry];
      update();
      let registered = true;
      return () => {
        if (!registered) {
          return;
        }
        registered = false;
        registrations = registrations.filter((candidate) => candidate !== entry);
        update();
      };
    },
    sections: () => shown,
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/** The app's Settings page sections. */
export const settingsSections: SettingsSectionRegistry = createSettingsSectionRegistry();

/** Registers a section with the app's registry; see {@link SettingsSectionRegistry}. */
export function registerSettingsSection(
  id: string,
  component: ComponentType,
  options?: { order?: number },
): () => void {
  return settingsSections.registerSection(id, component, options);
}

/** The registered sections, as React state. */
export function useSettingsSections(
  registry: SettingsSectionRegistry = settingsSections,
): readonly SettingsSection[] {
  return useSyncExternalStore(registry.subscribe, registry.sections);
}
