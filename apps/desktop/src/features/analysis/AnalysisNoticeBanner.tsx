/**
 * The banner for the live analysis' notices (docs/spec/04-user-interface.md §4.4, 02 §2.4.1): the
 * code panel, Problems and the run gate show the preview pipeline's last result, so when that
 * result is not the canvas as it is now, the window says so instead of showing it as current.
 *
 * - `syncFailed`: what the canvas reads back as is not a project the compiler core's loader
 *   accepts (for example blocks nested deeper than a project may be). Nothing of it is checked,
 *   and Build and Run refuse it. Shown until the canvas loads again (an undo, or another change).
 * - `trapRecovered`: the compiler core stopped and was restarted. What is shown may be out of
 *   date until the next change; the person can dismiss the banner.
 */
import { useId } from 'react';
import { useStore } from 'zustand';

import type { AnalysisNotice, useAppStore } from '../../app/store';
import './analysis.css';

/** What the banner says for each notice. */
export const ANALYSIS_NOTICE_TEXT: Readonly<Record<AnalysisNotice, string>> = {
  syncFailed:
    'Blocks2Cpp cannot read your latest change back as a project, so it was not checked. The C++ code and the problems show the last version it could read, and Build and Run will not start. Undo the change (Ctrl+Z) to go back to that version.',
  trapRecovered:
    'The Blocks2Cpp compiler core stopped and was restarted. This is a bug in Blocks2Cpp. If the C++ code or the problems look out of date, change a block to check the project again.',
};

/** The banner's heading for each notice. */
const TITLES: Readonly<Record<AnalysisNotice, string>> = {
  syncFailed: 'Your latest change was not checked',
  trapRecovered: 'The compiler core was restarted',
};

/** The live analysis' notice, if there is one; see the module comment. */
export function AnalysisNoticeBanner({ store }: { store: typeof useAppStore }) {
  const notice = useStore(store, (state) =>
    state.project === null ? null : state.analysis.notice,
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
        <p>{ANALYSIS_NOTICE_TEXT[notice]}</p>
      </div>
      {notice === 'trapRecovered' && (
        <div className="analysis-banner-actions">
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
        </div>
      )}
    </section>
  );
}
