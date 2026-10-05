/**
 * Sections other features add to the start page (docs/spec/04-user-interface.md §4.10): the
 * recovery feature's *Restore* / *Discard* offer after a crash is the first. A section is a React
 * component the start page renders above *New project*, ordered by `order` and then by ID; it
 * renders nothing when it has nothing to offer.
 *
 * ```ts
 * const unregister = registerStartPageSection('recovery', RecoveryOffer, { order: 0 });
 * ```
 *
 * Registrations stack like screens and commands: the newest component for an ID is shown, and
 * removing it brings the previous one back.
 */
import { type ComponentType, useSyncExternalStore } from 'react';

/** The most sections the start page shows; more registrations are refused (and logged). */
export const MAX_START_PAGE_SECTIONS = 16;

/** A section of the start page. */
export interface StartPageSection {
  /** A short, stable ID (`recovery`), also used as the React key. */
  readonly id: string;
  /** Lower comes first; ties are ordered by ID. */
  readonly order: number;
  /** What the section shows. It gets no props; it reads what it needs itself. */
  readonly component: ComponentType;
}

/** Options for {@link StartPageSections.register}. */
export interface StartPageSectionOptions {
  /** Lower comes first (default 0). */
  readonly order?: number;
}

/** The registry behind {@link registerStartPageSection}; tests can make their own. */
export interface StartPageSections {
  /** Adds a section and returns the function that removes this registration. */
  register(id: string, component: ComponentType, options?: StartPageSectionOptions): () => void;
  /** The sections to show, in order (a new array only when a registration changed). */
  list: () => readonly StartPageSection[];
  /** Calls `listener` whenever a registration is added or removed. Returns the unsubscriber. */
  subscribe: (listener: () => void) => () => void;
}

interface Registration {
  readonly section: StartPageSection;
  readonly serial: number;
}

/** Section IDs: short identifiers, so a mistake shows at registration, not as a React warning. */
const SECTION_ID = /^[A-Za-z][A-Za-z0-9_.-]{0,63}$/;

/** Creates an empty registry. */
export function createStartPageSections(): StartPageSections {
  const registrations: Registration[] = [];
  const listeners = new Set<() => void>();
  let serial = 0;
  let shown: readonly StartPageSection[] = [];

  function update(): void {
    // The newest registration of each ID wins.
    const newest = new Map<string, Registration>();
    for (const registration of registrations) {
      const current = newest.get(registration.section.id);
      if (current === undefined || current.serial < registration.serial) {
        newest.set(registration.section.id, registration);
      }
    }
    shown = [...newest.values()]
      .map((registration) => registration.section)
      .sort((a, b) => a.order - b.order || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
    for (const listener of [...listeners]) {
      listener();
    }
  }

  return {
    register(id, component, options = {}) {
      const order = options.order ?? 0;
      if (!SECTION_ID.test(id) || !Number.isFinite(order)) {
        throw new Error(`invalid start page section ${JSON.stringify(id)}`);
      }
      const ids = new Set(registrations.map((registration) => registration.section.id));
      if (!ids.has(id) && ids.size >= MAX_START_PAGE_SECTIONS) {
        console.error(`Too many start page sections; "${id}" was not added`);
        return () => undefined;
      }
      serial += 1;
      const registration: Registration = { section: { id, order, component }, serial };
      registrations.push(registration);
      update();
      let registered = true;
      return () => {
        if (!registered) {
          return;
        }
        registered = false;
        const index = registrations.indexOf(registration);
        if (index >= 0) {
          registrations.splice(index, 1);
        }
        update();
      };
    },

    list: () => shown,

    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/** The app's start page sections. */
export const startPageSections: StartPageSections = createStartPageSections();

/**
 * Adds a section to the app's start page; see {@link StartPageSections.register}.
 *
 * @throws Error for an ID that is not a short identifier or an order that is not a number.
 */
export function registerStartPageSection(
  id: string,
  component: ComponentType,
  options?: StartPageSectionOptions,
): () => void {
  return startPageSections.register(id, component, options);
}

/** The sections of `registry`, as React state that follows registrations. */
export function useStartPageSections(
  registry: StartPageSections = startPageSections,
): readonly StartPageSection[] {
  return useSyncExternalStore(registry.subscribe, registry.list);
}
