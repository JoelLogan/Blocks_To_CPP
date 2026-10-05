import type { IndentWidth, OnErrors, Settings } from '@blocks2cpp/ipc-types';
import { type ReactNode, useEffect, useId, useRef, useState } from 'react';
import { useStore } from 'zustand';
import { useShallow } from 'zustand/react/shallow';

import type { ScreenRegistry } from '../../app/screens';
import { useRegisteredScreen } from '../../app/screens';
import { useAppStore } from '../../app/store';
import { type SettingsController, settingsFailureText } from './controller';
import { FeaturePage, leavePage, Notice, PageSection } from './page/FeaturePage';
import { type SettingsSectionRegistry, useSettingsSections } from './sections';
import {
  cacheClearedText,
  formatLines,
  noticeText,
  parseScrollback,
  SCROLLBACK_LIMITS,
  shownNotices,
} from './texts';
import './settings.css';

/** The props of {@link SettingsPage}. */
export interface SettingsPageProps {
  controller: SettingsController;
  /** Where the toolchain page is registered (for the *Toolchain* section's link). */
  screens: ScreenRegistry;
  /** The sections other features add. */
  sections: SettingsSectionRegistry;
}

/**
 * The Settings page (docs/spec/04-user-interface.md §4.12): the machine settings, saved at once
 * and applied to every project on this computer. Nothing here goes into a project file.
 */
export function SettingsPage({ controller, screens, sections }: SettingsPageProps) {
  const { settings, notices, hasProject } = useAppStore(
    useShallow((state) => ({
      settings: state.settings.value,
      notices: state.settings.notices,
      hasProject: state.project !== null,
    })),
  );
  const { saving, error } = useStore(
    controller.page,
    useShallow((state) => ({ saving: state.saving, error: state.error })),
  );
  const extraSections = useSettingsSections(sections);

  useEffect(() => {
    // Read again when the page opens: the answer is the settings in effect now.
    void controller.refresh();
  }, [controller]);

  return (
    <FeaturePage
      title="Settings"
      hasProject={hasProject}
      testId="settings-page"
      onBack={() => {
        leavePage(useAppStore);
      }}
    >
      <p className="feature-muted">
        These settings are kept on this computer and apply to every project; none of them is saved
        in a project file. Changes are saved at once.
      </p>
      <p className="visually-hidden" role="status" data-testid="settings-saving">
        {saving > 0 ? 'Saving…' : ''}
      </p>

      {notices.length > 0 && <NoticeList notices={notices} />}

      {error !== null && (
        <Notice tone="error" testId="settings-error">
          <p>{settingsFailureText(error.action, error.code)}</p>
          {error.action === 'read' && settings === null && (
            <button
              type="button"
              className="button"
              onClick={() => {
                void controller.refresh();
              }}
            >
              Try again
            </button>
          )}
        </Notice>
      )}

      {settings === null ? (
        error === null && <p className="feature-muted">Reading the settings…</p>
      ) : (
        <SettingsForm controller={controller} settings={settings} />
      )}

      <ToolchainSection screens={screens} />
      <BuildCacheSection controller={controller} />

      {extraSections.map(({ id, component: Section }) => (
        <Section key={id} />
      ))}
    </FeaturePage>
  );
}

/** The notices the backend gave when it read `settings.json`. */
function NoticeList({ notices }: { notices: Parameters<typeof shownNotices>[0] }) {
  const { shown, more } = shownNotices(notices);
  return (
    <Notice tone="warning" testId="settings-notices">
      <p>Blocks2Cpp found problems in its settings file when it started:</p>
      <ul>
        {shown.map((notice, index) => (
          <li key={`${notice.key}-${notice.reason}-${String(index)}`}>{noticeText(notice)}</li>
        ))}
        {more > 0 && <li>and {String(more)} more</li>}
      </ul>
    </Notice>
  );
}

/** The code style, Run on errors and console sections. */
function SettingsForm({
  controller,
  settings,
}: {
  controller: SettingsController;
  settings: Settings;
}) {
  return (
    <>
      <PageSection title="Code style" testId="settings-code-style">
        <ChoiceField<IndentWidth>
          legend="Indent width of the generated C++"
          value={settings.codeStyle.indentWidth}
          options={[
            { value: 2, label: '2 spaces' },
            { value: 4, label: '4 spaces' },
          ]}
          onChange={(indentWidth) => controller.update({ codeStyle: { indentWidth } })}
          testId="settings-indent"
        />
        <p className="feature-muted">The C++ panel shows the change at once.</p>
      </PageSection>

      <PageSection title="Run on errors" testId="settings-run-on-errors">
        <ChoiceField<OnErrors>
          legend="When your blocks have errors"
          value={settings.run.onErrors}
          options={[
            {
              value: 'disableRun',
              label: 'Disable Run',
              description: 'Run is turned off, and its hint says how many errors there are.',
            },
            {
              value: 'showProblems',
              label: 'Show problems',
              description:
                'Run stays on; pressing it opens Problems at the first error instead of running.',
            },
          ]}
          onChange={(onErrors) => controller.update({ run: { onErrors } })}
          testId="settings-on-errors"
        />
        <p className="feature-muted">Either way, a project with errors is never built or run.</p>
      </PageSection>

      <PageSection title="Console" testId="settings-console">
        <ScrollbackField
          key={settings.console.scrollbackLines}
          value={settings.console.scrollbackLines}
          onCommit={(scrollbackLines) => controller.update({ console: { scrollbackLines } })}
        />
      </PageSection>
    </>
  );
}

/** One option of a {@link ChoiceField}. */
interface ChoiceOption<T> {
  value: T;
  label: string;
  description?: string;
}

/**
 * A group of radio buttons that saves its choice at once. While the change is saved the new
 * choice is shown; when it is refused the saved one comes back.
 */
function ChoiceField<T extends string | number>({
  legend,
  value,
  options,
  onChange,
  testId,
}: {
  legend: string;
  value: T;
  options: readonly ChoiceOption<T>[];
  onChange: (value: T) => Promise<boolean>;
  testId: string;
}) {
  const name = useId();
  const [pending, setPending] = useState<T | null>(null);
  const mounted = useMounted();
  const shown = pending ?? value;

  return (
    <fieldset className="feature-field" data-testid={testId}>
      <legend>{legend}</legend>
      {options.map((option) => {
        const id = `${name}-${String(option.value)}`;
        const descriptionId = `${id}-description`;
        return (
          <div key={String(option.value)} className="feature-option">
            <input
              type="radio"
              id={id}
              name={name}
              value={String(option.value)}
              checked={shown === option.value}
              aria-describedby={option.description === undefined ? undefined : descriptionId}
              onChange={() => {
                if (option.value === shown) {
                  return;
                }
                setPending(option.value);
                void onChange(option.value).finally(() => {
                  if (mounted.current) {
                    setPending(null);
                  }
                });
              }}
            />
            <span className="feature-option-text">
              <label htmlFor={id}>{option.label}</label>
              {option.description !== undefined && (
                <span id={descriptionId} className="feature-muted">
                  {option.description}
                </span>
              )}
            </span>
          </div>
        );
      })}
    </fieldset>
  );
}

/**
 * The scrollback size. It is saved when the field loses the focus or Enter is pressed, and only
 * when it holds a whole number within the limits; otherwise the field says what it accepts.
 */
function ScrollbackField({
  value,
  onCommit,
}: {
  value: number;
  onCommit: (lines: number) => Promise<boolean>;
}) {
  const id = useId();
  const hintId = `${id}-hint`;
  const errorId = `${id}-error`;
  const [text, setText] = useState(formatLines(value));
  const [invalid, setInvalid] = useState(false);

  const commit = () => {
    const lines = parseScrollback(text);
    if (lines === null) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    if (lines !== value) {
      void onCommit(lines);
    } else {
      setText(formatLines(value));
    }
  };

  return (
    <div className="feature-field">
      <label className="feature-field-label" htmlFor={id}>
        Scrollback lines
      </label>
      <input
        id={id}
        className="feature-input"
        type="text"
        inputMode="numeric"
        autoComplete="off"
        spellCheck={false}
        maxLength={12}
        value={text}
        aria-invalid={invalid}
        aria-describedby={invalid ? `${errorId} ${hintId}` : hintId}
        data-testid="settings-scrollback"
        onChange={(event) => {
          setText(event.target.value);
        }}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === 'Enter') {
            event.preventDefault();
            commit();
          } else if (event.key === 'Escape') {
            setText(formatLines(value));
            setInvalid(false);
          }
        }}
      />
      <p id={hintId} className="feature-muted">
        How many lines of your program&apos;s output the console keeps:{' '}
        {formatLines(SCROLLBACK_LIMITS.min)} to {formatLines(SCROLLBACK_LIMITS.max)}.
      </p>
      {invalid && (
        <p id={errorId} className="feature-field-error" role="alert">
          Enter a whole number from {formatLines(SCROLLBACK_LIMITS.min)} to{' '}
          {formatLines(SCROLLBACK_LIMITS.max)}.
        </p>
      )}
    </div>
  );
}

/** The link to the toolchain page, where the default compiler is chosen (§4.12). */
function ToolchainSection({ screens }: { screens: ScreenRegistry }) {
  const toolchainPage = useRegisteredScreen('toolchainSetup', screens);
  if (toolchainPage === null) {
    return null;
  }
  return (
    <PageSection title="Toolchain" testId="settings-toolchain">
      <p>The C++ compiler Blocks2Cpp builds with is chosen on the toolchain page.</p>
      <div className="feature-actions">
        <button
          type="button"
          className="button"
          onClick={() => {
            useAppStore.getState().actions.setUi({ screen: 'toolchainSetup' });
          }}
        >
          Open the toolchain page
        </button>
      </div>
    </PageSection>
  );
}

/** *Clear build cache* with its confirmation and result (07 §7.5.1). */
function BuildCacheSection({ controller }: { controller: SettingsController }) {
  const { clearing, cacheResult } = useStore(
    controller.page,
    useShallow((state) => ({ clearing: state.clearing, cacheResult: state.cacheResult })),
  );
  return (
    <PageSection title="Build cache" testId="settings-build-cache">
      <p>
        Blocks2Cpp keeps the programs it builds, so it does not have to build them again. Clearing
        the cache frees disk space; builds in use right now are kept.
      </p>
      <div className="feature-actions">
        <button
          type="button"
          className="button"
          aria-disabled={clearing}
          data-testid="settings-clear-cache"
          onClick={() => {
            void controller.clearBuildCache();
          }}
        >
          {clearing ? 'Clearing…' : 'Clear build cache…'}
        </button>
      </div>
      {cacheResult !== null && (
        <Notice tone="success" testId="settings-cache-result">
          <p>{cacheClearedText(cacheResult)}</p>
        </Notice>
      )}
    </PageSection>
  );
}

/** A ref that says whether the component is still mounted. */
function useMounted(): { readonly current: boolean } {
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  return mounted;
}

/** The window banner about settings that were reset when the app started. */
export function SettingsNoticesBanner({
  controller,
  onOpen,
}: {
  controller: SettingsController;
  onOpen: () => void;
}): ReactNode {
  const count = useAppStore((state) => state.settings.notices.length);
  const dismissed = useStore(controller.page, (state) => state.noticesDismissed);
  const screen = useAppStore((state) => state.ui.screen);
  const titleId = useId();
  if (count === 0 || dismissed || screen === 'settings') {
    return null;
  }
  return (
    <section
      className="settings-banner"
      aria-labelledby={titleId}
      data-testid="settings-notices-banner"
    >
      <span className="settings-banner-glyph" aria-hidden="true">
        ⚠
      </span>
      <p id={titleId} className="settings-banner-text">
        Blocks2Cpp found problems in its settings file and used default values for some settings.
      </p>
      <div className="settings-banner-actions">
        <button type="button" className="button" onClick={onOpen}>
          Show settings
        </button>
        <button
          type="button"
          className="button"
          onClick={() => {
            controller.dismissNotices();
          }}
        >
          Dismiss
        </button>
      </div>
    </section>
  );
}
