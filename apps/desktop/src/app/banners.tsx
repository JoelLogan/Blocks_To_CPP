/**
 * The window banners: messages that stay under the toolbar while their condition holds, such as
 * the Restricted Mode banner (docs/spec/04-user-interface.md §4.10, 08 §8.3). Features register the
 * component for a banner; the component decides itself whether it shows anything (it renders
 * `null` otherwise), so a banner follows the app's state without the shell knowing about it.
 *
 * Banners are shown in ascending `order`, then in registration order. Registering the same ID
 * again replaces the earlier registration until the newer one is removed, as with screens.
 */
import { type ComponentType, useSyncExternalStore } from 'react';

/** Where a banner goes among the others. */
export interface BannerOptions {
  /** Smaller numbers come first; the default is 0. */
  order?: number;
}

/** A registered banner. */
export interface Banner {
  readonly id: string;
  readonly component: ComponentType;
  readonly order: number;
}

/** The registry behind {@link registerBanner}; tests and features may create their own. */
export interface BannerRegistry {
  /** Registers `component` as the banner `id`; returns the function that removes it again. */
  registerBanner(id: string, component: ComponentType, options?: BannerOptions): () => void;
  /** The banners to show, in display order (one per ID, the newest registration). */
  banners: () => readonly Banner[];
  /** Calls `listener` whenever a banner is added or removed. Returns the unsubscriber. */
  subscribe: (listener: () => void) => () => void;
}

/** Creates an empty registry. */
export function createBannerRegistry(): BannerRegistry {
  let registrations: Banner[] = [];
  let shown: readonly Banner[] = [];
  const listeners = new Set<() => void>();

  /** Recomputes the shown list (a new array only when something changed) and notifies. */
  function update(): void {
    const newest = new Map<string, Banner>();
    for (const entry of registrations) {
      newest.set(entry.id, entry);
    }
    const ordered = [...newest.values()];
    const position = new Map(registrations.map((entry, index) => [entry, index]));
    ordered.sort((a, b) => a.order - b.order || (position.get(a) ?? 0) - (position.get(b) ?? 0));
    shown = ordered;
    for (const listener of [...listeners]) {
      listener();
    }
  }

  return {
    registerBanner(id, component, options = {}) {
      const order = options.order ?? 0;
      if (!Number.isFinite(order)) {
        throw new RangeError('A banner order must be a finite number');
      }
      const entry: Banner = { id, component, order };
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

    banners: () => shown,

    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}

/** The app's banner registry. */
export const banners: BannerRegistry = createBannerRegistry();

/** Registers a banner with the app's registry; see {@link BannerRegistry.registerBanner}. */
export function registerBanner(
  id: string,
  component: ComponentType,
  options?: BannerOptions,
): () => void {
  return banners.registerBanner(id, component, options);
}

/**
 * The banners, stacked under the toolbar. The container is a polite live region, so a banner that
 * appears (or changes its text) is announced without taking the keyboard focus.
 */
export function WindowBanners({ registry = banners }: { registry?: BannerRegistry }) {
  const shown = useSyncExternalStore(registry.subscribe, registry.banners);
  return (
    <div className="window-banners" aria-live="polite" data-testid="window-banners">
      {shown.map(({ id, component: Component }) => (
        <Component key={id} />
      ))}
    </div>
  );
}
