import { useId } from 'react';
import { useStore } from 'zustand';

import { useAppStore } from '../../app/store';
import { LockIcon } from '../../app/ui/icons';
import type { TrustController } from './controller';
import {
  isFailure,
  MARK_OF_THE_WEB_TEXT,
  RESTRICTED_MODE_TEXT,
  restrictedReasonText,
  trustMessageText,
} from './texts';
import './trust.css';

/** The selector of the toolbar's Run button, which gets the focus once the project is trusted. */
export const RUN_BUTTON_SELECTOR = '[data-testid="toolbar-run"]';

/** Moves the focus to Run once the banner (and its focused button) is gone. */
function focusRun(): void {
  window.setTimeout(() => {
    document.querySelector<HTMLElement>(RUN_BUTTON_SELECTOR)?.focus();
  }, 0);
}

/**
 * The Restricted Mode banner (docs/spec/08-security.md §8.3, 04 §4.10): shown under the toolbar
 * for as long as the open project is restricted. It says why (no trust record, or security-relevant
 * content changed outside the app), warns more strongly for a file from the Internet, explains
 * that Build and Run are off, and offers *Trust…*, which opens the backend's native trust dialog.
 */
export function RestrictedModeBanner({ controller }: { controller: TrustController }) {
  const project = useAppStore((state) => state.project);
  const { busy, message } = useStore(controller.state);
  const titleId = useId();

  if (project?.trust.state !== 'restricted') {
    return null;
  }
  const { trust } = project;
  const shown = message?.handle === project.handle ? message.message : null;

  return (
    <section
      className={`trust-banner${trust.markOfTheWeb ? ' trust-banner-motw' : ''}`}
      aria-labelledby={titleId}
      data-testid="restricted-banner"
    >
      <LockIcon className="trust-banner-icon" />
      <div className="trust-banner-text">
        <h2 id={titleId} className="trust-banner-title">
          Restricted Mode
        </h2>
        {trust.markOfTheWeb && (
          <p className="trust-banner-warning" data-testid="restricted-banner-motw">
            <span aria-hidden="true">⚠ </span>
            <strong>{MARK_OF_THE_WEB_TEXT}</strong>
          </p>
        )}
        <p data-testid="restricted-banner-reason">{restrictedReasonText(trust.restrictedReason)}</p>
        <p>{RESTRICTED_MODE_TEXT}</p>
        {shown !== null && (
          <p
            className={isFailure(shown) ? 'trust-banner-error' : 'trust-banner-status'}
            data-testid="restricted-banner-message"
          >
            {isFailure(shown) && <span aria-hidden="true">✖ </span>}
            {trustMessageText(shown)}
          </p>
        )}
      </div>
      <div className="trust-banner-actions">
        <button
          type="button"
          className="button button-primary"
          aria-disabled={busy !== null}
          data-testid="restricted-banner-trust"
          onClick={() => {
            void controller.grant().then((result) => {
              if (result?.state === 'trusted') {
                focusRun();
              }
            });
          }}
        >
          {busy === 'grant' ? 'Waiting for your answer…' : 'Trust…'}
        </button>
      </div>
    </section>
  );
}
