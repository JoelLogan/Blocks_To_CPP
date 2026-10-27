/**
 * The Appearance setting (docs/spec/04-user-interface.md §4.8, F-1 of the M2 polish plan): light,
 * dark or the system's scheme, applied to the whole window through one attribute on `<html>`,
 * `data-theme`. The style sheets hold the light tokens on `:root` and the dark ones on
 * `:root[data-theme='dark']` (app.css); the Blockly theme (EditorWorkspace) and the console's
 * terminal (ConsolePanel) follow the attribute with {@link onThemeChange}.
 *
 * The attribute is the one place the theme in use is kept: whatever sets it, every part of the
 * window reads it, so they never disagree. {@link startAppearance} sets it from the setting in the
 * app's store and, for *Follow the system*, from the system's scheme while the app runs.
 */
import type { Theme } from '@blocks2cpp/ipc-types';
import { useSyncExternalStore } from 'react';

/** The theme in use: what *Follow the system* resolves to, or the one chosen. */
export type ResolvedTheme = 'light' | 'dark';

/** The attribute on `<html>` that holds the theme in use. */
export const THEME_ATTRIBUTE = 'data-theme';

/** The media query of the system's dark scheme. */
export const DARK_SCHEME_QUERY = '(prefers-color-scheme: dark)';

/** The theme in use for a setting (`null` before the settings are read: the system's). */
export function resolveTheme(choice: Theme | null, systemDark: boolean): ResolvedTheme {
  switch (choice) {
    case 'light':
    case 'dark':
      return choice;
    case 'system':
    case null:
      return systemDark ? 'dark' : 'light';
  }
}

/** The theme `root` shows: dark only when its attribute says so. */
export function documentTheme(root: Element = document.documentElement): ResolvedTheme {
  return root.getAttribute(THEME_ATTRIBUTE) === 'dark' ? 'dark' : 'light';
}

/** Shows `theme` on `root`; does nothing when it already does (so no change is reported). */
export function applyTheme(root: Element, theme: ResolvedTheme): void {
  if (root.getAttribute(THEME_ATTRIBUTE) !== theme) {
    root.setAttribute(THEME_ATTRIBUTE, theme);
  }
}

/**
 * Calls `listener` with the new theme whenever the theme `root` shows changes. Returns the
 * function that stops it.
 */
export function onThemeChange(
  listener: (theme: ResolvedTheme) => void,
  root: Element = document.documentElement,
): () => void {
  let shown = documentTheme(root);
  const observer = new MutationObserver(() => {
    const theme = documentTheme(root);
    if (theme !== shown) {
      shown = theme;
      listener(theme);
    }
  });
  observer.observe(root, { attributes: true, attributeFilter: [THEME_ATTRIBUTE] });
  return () => {
    observer.disconnect();
  };
}

/** The theme the page shows, as React state (for components that pass it on as a prop). */
export function useResolvedTheme(): ResolvedTheme {
  return useSyncExternalStore(subscribeToTheme, () => documentTheme());
}

/** {@link onThemeChange} on the page, in `useSyncExternalStore`'s shape. */
function subscribeToTheme(listener: () => void): () => void {
  return onThemeChange(() => {
    listener();
  });
}

/** The part of a media query list the appearance uses. */
export interface SchemeQuery {
  readonly matches: boolean;
  addEventListener(type: 'change', listener: () => void): void;
  removeEventListener(type: 'change', listener: () => void): void;
}

/** What {@link startAppearance} needs. */
export interface AppearanceOptions {
  /** The element that carries the attribute (`document.documentElement`). */
  readonly root: Element;
  /** The chosen setting, or `null` before the settings are read. */
  readonly choice: () => Theme | null;
  /** Calls its argument whenever the setting may have changed; returns the unsubscriber. */
  readonly subscribe: (listener: () => void) => () => void;
  /** The system's dark-scheme query (`window.matchMedia(DARK_SCHEME_QUERY)`), or `null`. */
  readonly scheme: SchemeQuery | null;
}

/**
 * Applies the setting now, then follows it and the system's scheme. Returns the function that
 * stops following them (the attribute stays as it is).
 */
export function startAppearance(options: AppearanceOptions): () => void {
  const { root, choice, subscribe, scheme } = options;
  const apply = (): void => {
    applyTheme(root, resolveTheme(choice(), scheme?.matches === true));
  };
  apply();
  const unsubscribe = subscribe(apply);
  scheme?.addEventListener('change', apply);
  return () => {
    unsubscribe();
    scheme?.removeEventListener('change', apply);
  };
}

/** The system's dark-scheme query in `target`, or `null` where it has none. */
export function systemSchemeQuery(target: Pick<Window, 'matchMedia'>): SchemeQuery | null {
  return typeof target.matchMedia === 'function' ? target.matchMedia(DARK_SCHEME_QUERY) : null;
}
