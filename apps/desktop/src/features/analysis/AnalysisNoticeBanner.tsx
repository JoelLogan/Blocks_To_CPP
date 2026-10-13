/**
 * The banner for the live analysis' notices (docs/spec/04-user-interface.md §4.4, 02 §2.4.1): the
 * code panel, Problems and the run gate show the preview pipeline's last result, so when that
 * result is not the canvas as it is now, the window says so instead of showing it as current.
 *
 * - `syncFailed`: what the canvas reads back as is not a project the compiler core's loader
 *   accepts. Nothing of it is checked, and Build and Run refuse it. Shown until the canvas loads
 *   again (an undo, or another change); *Undo last change* undoes the block editor's last change
 *   (the command `edit.undo`). When the canvas went past a limit a person can reach by editing
 *   (blocks nested too deeply, a project too large: src/features/project/limits.ts), the banner
 *   names the limit and how to get back within it.
 * - `trapRecovered`: the compiler core stopped and was restarted. What is shown may be out of
 *   date until the next change; the person can dismiss the banner.
 */
import { useId } from 'react';
import { useStore } from 'zustand';
import { useShallow } from 'zustand/react/shallow';

import { type CommandRegistry, commands as appCommands, triggerCommand } from '../../app/commands';
import type { AnalysisNotice, useAppStore } from '../../app/store';
import { limitMessage, reachedLimit } from '../project/limits';
import './analysis.css';

/** What the banner says for each notice (for `syncFailed`, when no limit was reached). */
export const ANALYSIS_NOTICE_TEXT: Readonly<Record<AnalysisNotice, string>> = {
  syncFailed:
    'Blocks2Cpp cannot read your latest change back as a project, so it was not checked. The C++ code and the problems show the last version it could read, and Build and Run will not start. Undo the change (Ctrl+Z) to go back to that version.',
  trapRecovered:
    'The Blocks2Cpp compiler core stopped and was restarted. This is a bug in Blocks2Cpp. If the C++ code or the problems look out of date, change a block to check the project again.',
};

/**
 * What the `syncFailed` banner says for the loader's problem `codes`: which limit the canvas went
 * past and how to get back within it, or {@link ANALYSIS_NOTICE_TEXT}'s text for any other
 * problem.
 */
export function syncFailedText(codes: readonly string[]): string {
  const limit = reachedLimit(codes);
  if (limit === null) {
    return ANALYSIS_NOTICE_TEXT.syncFailed;
  }
  return `${limitMessage(limit)} Until then, the C++ code and the problems show the last version within the limits, and Build and Run will not start.`;
}

/** The banner's heading for each notice. */
const TITLES: Readonly<Record<AnalysisNotice, string>> = {
  syncFailed: 'Your latest change was not checked',
  trapRecovered: 'The compiler core was restarted',
};

/**
 * The live analysis' notice, if there is one; see the module comment. `commands` runs *Undo last
 * change*; the app's registry by default.
 */
export function AnalysisNoticeBanner({
  store,
  commands = appCommands,
}: {
  store: typeof useAppStore;
  commands?: CommandRegistry;
}) {
  const { notice, syncCodes } = useStore(
    store,
    useShallow((state) => ({
      notice: state.project === null ? null : state.analysis.notice,
      syncCodes: state.analysis.syncCodes,
    })),
  );
  const titleId = useId();
  if (notice === null) {
    return null;
  }
  return (
    <section
      className="analysis-banner"
      aria-labelledby={titleId}
      data-testid="analysis-notice-banner"
      data-notice={notice}
    >
      <span className="analysis-banner-glyph" aria-hidden="true">
        ⚠
      </span>
      <div className="analysis-banner-text">
        <h2 id={titleId} className="analysis-banner-title">
          {TITLES[notice]}
        </h2>
        <p>{notice === 'syncFailed' ? syncFailedText(syncCodes) : ANALYSIS_NOTICE_TEXT[notice]}</p>
      </div>
      <div className="analysis-banner-actions">
        {notice === 'syncFailed' ? (
          // The focus stays here while the canvas still does not load, so it can be pressed again.
          <button
            type="button"
            className="button"
            data-testid="analysis-undo"
            onClick={() => {
              triggerCommand('edit.undo', commands);
            }}
          >
            Undo last change
          </button>
        ) : (
          <button
            type="button"
            className="button"
            onClick={() => {
              const { analysis, actions } = store.getState();
              if (analysis.notice === 'trapRecovered') {
                actions.setAnalysis({ notice: null });
              }
            }}
          >
            Dismiss
          </button>
        )}
      </div>
    </section>
  );
}
