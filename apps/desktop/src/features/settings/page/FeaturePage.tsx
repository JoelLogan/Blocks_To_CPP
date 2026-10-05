/**
 * The frame of the full-window pages of the toolchain and settings features
 * (docs/spec/04-user-interface.md §4.6, §4.12): a heading that takes the keyboard focus when the
 * page opens, a button back to where the person came from, and notices that are announced.
 */
import { type ReactNode, useEffect, useId, useRef } from 'react';

import type { useAppStore } from '../../../app/store';
import './page.css';

/** Where *Back* goes: the editor while a project is open, the start page otherwise. */
export function leavePage(store: typeof useAppStore): void {
  const state = store.getState();
  state.actions.setUi({ screen: state.project === null ? 'start' : 'editor' });
}

/** The label of the *Back* button for the current state. */
export function backLabel(hasProject: boolean): string {
  return hasProject ? 'Back to the editor' : 'Back to the start page';
}

/** The props of {@link FeaturePage}. */
export interface FeaturePageProps {
  /** The page's heading. */
  title: string;
  /** Whether a project is open (it names where *Back* goes). */
  hasProject: boolean;
  /** Called by *Back*. */
  onBack: () => void;
  /** A `data-testid` for the page. */
  testId: string;
  children: ReactNode;
}

/**
 * A full-window page. The page sits inside the shell's `main` landmark, so its heading is an `h2`
 * (the top bar's project name is the `h1`). The heading is focused when the page opens, so screen
 * readers announce it and the keyboard starts at the top.
 */
export function FeaturePage({ title, hasProject, onBack, testId, children }: FeaturePageProps) {
  const heading = useRef<HTMLHeadingElement>(null);
  const titleId = useId();

  useEffect(() => {
    heading.current?.focus({ preventScroll: true });
  }, []);

  return (
    <div className="feature-page" data-testid={testId}>
      <section className="feature-page-content" aria-labelledby={titleId}>
        <header className="feature-page-header">
          <h2 id={titleId} ref={heading} className="feature-page-title" tabIndex={-1}>
            {title}
          </h2>
          <button type="button" className="button" onClick={onBack}>
            {backLabel(hasProject)}
          </button>
        </header>
        {children}
      </section>
    </div>
  );
}

/** How a {@link Notice} looks and how it is announced. */
export type NoticeTone = 'info' | 'success' | 'warning' | 'error';

const TONE_GLYPHS: Record<NoticeTone, string> = {
  info: 'ℹ',
  success: '√',
  warning: '⚠',
  error: '✖',
};

const TONE_LABELS: Record<NoticeTone, string> = {
  info: 'Note:',
  success: 'Done:',
  warning: 'Warning:',
  error: 'Error:',
};

/**
 * A message on a page. Errors are alerts (announced at once); the others are polite status
 * messages. The tone is shown by a glyph and spoken as a word, never by colour alone.
 */
export function Notice({
  tone,
  children,
  testId,
}: {
  tone: NoticeTone;
  children: ReactNode;
  testId?: string;
}) {
  return (
    <div
      className={`feature-notice feature-notice-${tone}`}
      role={tone === 'error' ? 'alert' : 'status'}
      data-testid={testId}
    >
      <span className="feature-notice-glyph" aria-hidden="true">
        {TONE_GLYPHS[tone]}
      </span>
      <div className="feature-notice-body">
        <span className="visually-hidden">{TONE_LABELS[tone]} </span>
        {children}
      </div>
    </div>
  );
}

/** A titled part of a page. */
export function PageSection({
  title,
  children,
  testId,
}: {
  title: string;
  children: ReactNode;
  testId?: string;
}) {
  const id = useId();
  return (
    <section className="feature-section" aria-labelledby={id} data-testid={testId}>
      <h3 id={id} className="feature-section-title">
        {title}
      </h3>
      {children}
    </section>
  );
}
