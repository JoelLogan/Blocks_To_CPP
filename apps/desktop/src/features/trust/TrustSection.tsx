import { useStore } from 'zustand';
import { useShallow } from 'zustand/react/shallow';

import { useAppStore } from '../../app/store';
import { Notice, PageSection } from '../settings/page/FeaturePage';
import { displayText } from '../toolchain/reasons';
import type { TrustController } from './controller';
import { isFailure, restrictedReasonText, trustedText, trustMessageText } from './texts';

/** The longest project name shown; longer names are cut. */
const MAX_NAME_LENGTH = 120;

/**
 * The Settings page's *This project* section (docs/spec/08-security.md §8.3): whether the open
 * project is trusted and why, *Revoke trust* when the trust comes from the project's own record,
 * and *Trust…* while it is restricted. Hidden while no project is open.
 */
export function TrustSection({ controller }: { controller: TrustController }) {
  const project = useAppStore(
    useShallow((state) =>
      state.project === null
        ? null
        : {
            handle: state.project.handle,
            name: state.project.document.project.name,
            trust: state.project.trust,
          },
    ),
  );
  const { busy, message } = useStore(controller.state);
  if (project === null) {
    return null;
  }
  const { trust } = project;
  const shown = message?.handle === project.handle ? message.message : null;

  return (
    <PageSection title="This project" testId="settings-trust">
      <p>
        Project:{' '}
        <bdi className="trust-project-name">{displayText(project.name, MAX_NAME_LENGTH)}</bdi>
      </p>
      {trust.state === 'trusted' ? (
        <p data-testid="settings-trust-state">{trustedText(trust)}</p>
      ) : (
        <p data-testid="settings-trust-state">
          This project is in Restricted Mode: Build and Run are off.{' '}
          {restrictedReasonText(trust.restrictedReason)}
        </p>
      )}
      <div className="feature-actions">
        {trust.state === 'restricted' && (
          <button
            type="button"
            className="button"
            aria-disabled={busy !== null}
            onClick={() => {
              void controller.grant();
            }}
          >
            {busy === 'grant' ? 'Waiting for your answer…' : 'Trust…'}
          </button>
        )}
        {trust.state === 'trusted' && trust.source === 'project' && (
          <button
            type="button"
            className="button"
            aria-disabled={busy !== null}
            data-testid="settings-trust-revoke"
            onClick={() => {
              void controller.revoke();
            }}
          >
            {busy === 'revoke' ? 'Revoking…' : 'Revoke trust'}
          </button>
        )}
      </div>
      {shown !== null && (
        <Notice tone={isFailure(shown) ? 'error' : 'info'} testId="settings-trust-message">
          <p>{trustMessageText(shown)}</p>
        </Notice>
      )}
    </PageSection>
  );
}
