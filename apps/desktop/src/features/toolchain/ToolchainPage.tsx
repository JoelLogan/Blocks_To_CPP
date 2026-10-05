import type { Platform } from '@blocks2cpp/ipc-types';
import { useStore } from 'zustand';
import { useShallow } from 'zustand/react/shallow';

import { useAppStore } from '../../app/store';
import { hasUsableToolchain, selectedToolchain } from '../../app/store/selectors';
import { FeaturePage, leavePage, Notice, PageSection } from '../settings/page/FeaturePage';
import type { FailureCode } from '../settings/page/ipcErrors';
import type {
  SetupLink,
  ToolchainAction,
  ToolchainController,
  ToolchainOutcome,
} from './controller';
import { RESCAN_LABEL, SetupGuide } from './SetupGuide';
import { ProblemList, ToolchainList } from './ToolchainList';
import { displayText, toolchainName } from './reasons';
import './toolchain.css';

/** Where the help links go, to show when the browser cannot be opened (02 §2.5 open_help_link). */
export const LINK_ADDRESSES: Record<SetupLink, string> = {
  msys2Install: 'https://www.msys2.org/',
  winlibs: 'https://winlibs.com/',
};

/** The page's heading: a setup page while nothing can build, the toolchain page otherwise. */
export function pageTitle(needsSetup: boolean): string {
  return needsSetup ? 'Set up a C++ compiler' : 'C++ compiler';
}

/**
 * The toolchain page (docs/spec/04-user-interface.md §4.6). In M2 the setup page and the
 * toolchain list are one page: while no compiler can build, it explains what a compiler is and how
 * to install g++ on this platform; it always offers *Rescan* and *Choose g++ manually…* and lists
 * every compiler found, with *Select as default*.
 */
export function ToolchainPage({ controller }: { controller: ToolchainController }) {
  const { list, discovering, setupInfo, usable, current, hasProject, appPlatform } = useAppStore(
    useShallow((state) => ({
      list: state.toolchains.list,
      discovering: state.toolchains.discovering,
      setupInfo: state.toolchains.setupInfo,
      usable: hasUsableToolchain(state),
      current: selectedToolchain(state),
      hasProject: state.project !== null,
      appPlatform: state.appInfo?.platform ?? null,
    })),
  );
  const { busy, selecting, outcome } = useStore(controller.page);
  const platform: Platform | null = setupInfo?.platform ?? appPlatform;
  // The steps stay while the person's own rescan runs, so the page does not jump under them.
  const showGuide = !usable && (!discovering || busy === 'rescan');

  return (
    <FeaturePage
      title={pageTitle(showGuide)}
      hasProject={hasProject}
      testId="toolchain-page"
      onBack={() => {
        controller.clearOutcome();
        leavePage(useAppStore);
      }}
    >
      <div data-testid="toolchain-status">
        {discovering ? (
          <Notice tone="info">
            <p>Looking for g++ on this computer… The list below updates when the search ends.</p>
          </Notice>
        ) : current !== null ? (
          <Notice tone="success">
            <p>
              Blocks2Cpp builds your projects with <strong>{toolchainName(current)}</strong>.
            </p>
          </Notice>
        ) : (
          <Notice tone="warning">
            <p>
              No usable g++ was found on this computer, so your projects cannot be built and run
              yet. Install it as shown below; it takes a few minutes.
            </p>
          </Notice>
        )}
      </div>

      {showGuide && (
        <SetupGuide
          platform={platform}
          distro={setupInfo?.distro ?? null}
          busy={busy !== null}
          onOpenLink={(link) => {
            void controller.openLink(link);
          }}
        />
      )}

      <PageSection title={showGuide ? 'When g++ is installed' : 'Find compilers'}>
        <p>
          {showGuide
            ? 'Blocks2Cpp looks for g++ again when you press the button below. If you installed it in another folder, choose its g++ program yourself.'
            : 'Look for compilers again after installing or updating one, or choose a g++ program yourself.'}
        </p>
        {platform !== 'linux' && (
          <p className="feature-muted">On Windows, only a program named g++.exe can be chosen.</p>
        )}
        <div className="feature-actions">
          <button
            type="button"
            className="button button-primary"
            aria-disabled={busy !== null}
            data-testid="toolchain-rescan"
            onClick={() => {
              void controller.rescan();
            }}
          >
            {busy === 'rescan' ? (
              'Looking for g++…'
            ) : showGuide ? (
              <>
                I installed it<span aria-hidden="true"> →</span>
                <span className="visually-hidden">:</span> Rescan
              </>
            ) : (
              'Rescan'
            )}
          </button>
          <button
            type="button"
            className="button"
            aria-disabled={busy !== null}
            data-testid="toolchain-add"
            onClick={() => {
              void controller.addManually();
            }}
          >
            Choose g++ manually…
          </button>
        </div>
        {outcome !== null && <OutcomeNotice outcome={outcome} />}
      </PageSection>

      <PageSection title="Compilers on this computer" testId="toolchain-list-section">
        <ToolchainList
          toolchains={list}
          discovering={discovering}
          busy={busy !== null}
          selecting={selecting}
          onSelect={(id) => {
            void controller.select(id);
          }}
        />
      </PageSection>
    </FeaturePage>
  );
}

/** `1 compiler` or `3 compilers`. */
function compilers(count: number): string {
  return count === 1 ? '1 compiler' : `${String(count)} compilers`;
}

/** What the last action found. */
function OutcomeNotice({ outcome }: { outcome: ToolchainOutcome }) {
  switch (outcome.kind) {
    case 'rescanned':
      if (outcome.usable > 0) {
        return (
          <Notice tone="success" testId="toolchain-outcome">
            <p>
              Found {compilers(outcome.found)}; {String(outcome.usable)} can build. You are ready to
              build and run your projects.
            </p>
          </Notice>
        );
      }
      return (
        <Notice tone="warning" testId="toolchain-outcome">
          <p>
            {outcome.found === 0
              ? `Still no g++ found. Install it as described above, then press ${RESCAN_LABEL} again.`
              : `Found ${compilers(outcome.found)}, but none can be used. The list below says why.`}
          </p>
        </Notice>
      );
    case 'added':
      return outcome.toolchain.usable ? (
        <Notice tone="success" testId="toolchain-outcome">
          <p>
            Added {toolchainName(outcome.toolchain)} from{' '}
            <span className="feature-path" dir="ltr">
              <bdi>{displayText(outcome.toolchain.displayPath, 1024)}</bdi>
            </span>
            .
          </p>
        </Notice>
      ) : (
        <Notice tone="warning" testId="toolchain-outcome">
          <p>Added {toolchainName(outcome.toolchain)}, but it cannot be used to build:</p>
          <ProblemList diagnostics={outcome.toolchain.problems} />
        </Notice>
      );
    case 'rejected':
      return (
        <Notice tone="error" testId="toolchain-outcome">
          <p>This program cannot be used as the compiler, so nothing was added:</p>
          <ProblemList diagnostics={outcome.diagnostics} />
        </Notice>
      );
    case 'selected':
      return (
        <Notice tone="success" testId="toolchain-outcome">
          <p>{toolchainName(outcome.toolchain)} is now the default compiler.</p>
        </Notice>
      );
    case 'failed':
      return (
        <Notice tone="error" testId="toolchain-outcome">
          <p>{failureText(outcome.action, outcome.code, outcome.link)}</p>
        </Notice>
      );
  }
}

/** What to say when an action failed. Only fixed texts: the backend's errors carry no text. */
export function failureText(action: ToolchainAction, code: FailureCode, link?: SetupLink): string {
  if (code === 'busy') {
    return 'Another dialog is already open. Close it, then try again.';
  }
  switch (action) {
    case 'rescan':
      return `Looking for compilers did not work (${code}). Try again.`;
    case 'add':
      return `The file dialog could not be shown (${code}). Try again.`;
    case 'select':
      if (code === 'unknownToolchain') {
        return 'That compiler is no longer in the list, which has been updated. Choose another one.';
      }
      if (code === 'io') {
        return 'The choice could not be saved. Try again.';
      }
      return `The default compiler could not be changed (${code}). Try again.`;
    case 'openLink':
      return link === undefined
        ? 'Your web browser could not be opened.'
        : `Your web browser could not be opened. The page is at ${LINK_ADDRESSES[link]}`;
  }
}
