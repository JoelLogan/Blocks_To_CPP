/**
 * The start page (docs/spec/04-user-interface.md §4.10): new project from a template, *Open…*,
 * the recent projects, the sections other features add (the recovery offer), and the problems of
 * a file that did not load. It is shown while `ui.screen` is `start`, which is where the window
 * starts and where closing a project leads.
 */
import type { RecentEntry, Template } from '@blocks2cpp/ipc-types';
import { useEffect, useId } from 'react';
import { useStore } from 'zustand';

import type { useAppStore } from '../../app/store';
import { type ProjectLifecycle, TEMPLATE_LABELS } from './lifecycle';
import type { LifecycleOperation, ProjectModel } from './model';
import { type StartPageSections, useStartPageSections } from './sections';
import { localDateTime, MAX_SHOWN_PATH_CHARS, shownName, shownText } from './text';
import './startPage.css';

/** What the start page needs. */
export interface StartPageProps {
  readonly lifecycle: ProjectLifecycle;
  readonly model: ProjectModel;
  readonly store: typeof useAppStore;
  readonly sections: StartPageSections;
}

/** What each template is, in one line. */
const TEMPLATE_DESCRIPTIONS: Readonly<Record<Template, string>> = {
  empty: 'A “when program starts” block, ready for your own program.',
  helloWorld: 'A small program that prints a greeting: a good first build.',
};

/** The templates, in the order shown. */
const TEMPLATES: readonly Template[] = ['empty', 'helloWorld'];

/** What the status line says while an operation runs. */
const BUSY_TEXT: Readonly<Record<LifecycleOperation, string>> = {
  new: 'Creating the project…',
  open: 'Opening the project…',
  save: 'Saving the project…',
  saveAs: 'Saving the project…',
  close: 'Closing the project…',
  quit: 'Quitting…',
  restore: 'Restoring the project…',
  reload: 'Reloading the project…',
};

/** Runs a lifecycle operation from a click; its errors are shown by the lifecycle itself. */
function run(task: () => Promise<unknown>): void {
  task().catch((error: unknown) => {
    console.error('A project operation failed', error);
  });
}

/** The start page; see the module comment. */
export function StartPage({ lifecycle, model, store, sections }: StartPageProps) {
  const busy = useStore(model, (state) => state.busy);
  const openName = useStore(store, (state) =>
    state.project === null ? null : shownName(state.project.document.project.name),
  );
  const shown = useStartPageSections(sections);
  const newId = useId();
  const openId = useId();

  useEffect(() => {
    run(() => lifecycle.refreshRecent());
  }, [lifecycle]);

  const idle = busy === null;

  return (
    <div className="start-page" data-testid="start-page">
      <header className="start-header">
        <h2 className="start-title">Start</h2>
        <p className="start-intro">
          Build real C++ programs from blocks. Start a new project, or open one you made before.
        </p>
        {openName !== null && (
          <button
            type="button"
            className="button"
            // The same words as shown; the name is isolated on screen only.
            aria-label={`Back to “${openName}”`}
            onClick={() => {
              store.getState().actions.setUi({ screen: 'editor' });
            }}
          >
            Back to “<bdi>{openName}</bdi>”
          </button>
        )}
      </header>

      <LoadFailurePanel model={model} lifecycle={lifecycle} />

      {shown.map((section) => (
        <section.component key={section.id} />
      ))}

      <section className="start-section" aria-labelledby={newId}>
        <h3 id={newId} className="start-section-title">
          New project
        </h3>
        <ul className="start-templates">
          {TEMPLATES.map((template) => (
            <li key={template}>
              <TemplateButton
                template={template}
                idle={idle}
                onCreate={() => {
                  run(() => lifecycle.newProject(template));
                }}
              />
            </li>
          ))}
        </ul>
      </section>

      <section className="start-section" aria-labelledby={openId}>
        <h3 id={openId} className="start-section-title">
          Open a project
        </h3>
        <button
          type="button"
          className="button button-primary"
          data-testid="start-open"
          aria-disabled={!idle}
          onClick={() => {
            if (idle) {
              run(() => lifecycle.open());
            }
          }}
        >
          Open…
        </button>
      </section>

      <RecentProjects model={model} lifecycle={lifecycle} idle={idle} />

      <p className="start-status" role="status">
        {busy === null ? '' : BUSY_TEXT[busy]}
      </p>
    </div>
  );
}

/** A template's button: its name, with its description as the button's description. */
function TemplateButton({
  template,
  idle,
  onCreate,
}: {
  template: Template;
  idle: boolean;
  onCreate: () => void;
}) {
  const titleId = useId();
  const descriptionId = useId();
  return (
    <button
      type="button"
      className="start-card"
      data-testid={`template-${template}`}
      aria-disabled={!idle}
      aria-labelledby={titleId}
      aria-describedby={descriptionId}
      onClick={() => {
        if (idle) {
          onCreate();
        }
      }}
    >
      <span id={titleId} className="start-card-title">
        {TEMPLATE_LABELS[template]}
      </span>
      <span id={descriptionId} className="start-card-text">
        {TEMPLATE_DESCRIPTIONS[template]}
      </span>
    </button>
  );
}

/** The problems of the last file that did not load (04 §4.10: nothing opens). */
function LoadFailurePanel({
  model,
  lifecycle,
}: {
  model: ProjectModel;
  lifecycle: ProjectLifecycle;
}) {
  const failure = useStore(model, (state) => state.loadFailure);
  const titleId = useId();
  if (failure === null) {
    return null;
  }
  return (
    <section
      className="start-section start-load-failure"
      aria-labelledby={titleId}
      data-testid="project-load-failure"
    >
      <div role="alert">
        <h3 id={titleId} className="start-section-title">
          {failure.name === null ? (
            'The project could not be opened'
          ) : (
            <>
              “<bdi>{failure.name}</bdi>” could not be opened
            </>
          )}
        </h3>
        <p>Nothing was opened. Blocks2Cpp found these problems in the file:</p>
      </div>
      <ul className="load-problems">
        {failure.problems.map((problem, index) => (
          // The list never changes while shown, so the position is a stable key.
          <li key={`${String(index)}-${problem.code}`}>
            <code className="load-problem-code">{problem.code}</code>{' '}
            <span className="load-problem-message">{problem.message}</span>
          </li>
        ))}
      </ul>
      {failure.omitted > 0 && (
        <p>
          …and {failure.omitted} more {failure.omitted === 1 ? 'problem' : 'problems'}.
        </p>
      )}
      <button
        type="button"
        className="button"
        onClick={() => {
          lifecycle.dismissLoadFailure();
        }}
      >
        Dismiss
      </button>
    </section>
  );
}

/** The recent projects, newest first, each with a button to remove it from the list. */
function RecentProjects({
  model,
  lifecycle,
  idle,
}: {
  model: ProjectModel;
  lifecycle: ProjectLifecycle;
  idle: boolean;
}) {
  const recent = useStore(model, (state) => state.recent);
  const titleId = useId();

  let content;
  if (recent.status === 'failed') {
    content = (
      <p className="start-note">
        The list of recent projects could not be read.{' '}
        <button
          type="button"
          className="button"
          onClick={() => {
            run(() => lifecycle.refreshRecent());
          }}
        >
          Try again
        </button>
      </p>
    );
  } else if (recent.entries.length === 0) {
    content = (
      <p className="start-note">
        {recent.status === 'ready'
          ? 'No recent projects yet. Projects you open or save appear here.'
          : 'Reading the list…'}
      </p>
    );
  } else {
    content = (
      <ul className="recent-list">
        {recent.entries.map((entry) => (
          <RecentItem key={entry.recentId} entry={entry} lifecycle={lifecycle} idle={idle} />
        ))}
      </ul>
    );
  }

  return (
    <section className="start-section" aria-labelledby={titleId}>
      <h3 id={titleId} className="start-section-title">
        Recent projects
      </h3>
      {content}
    </section>
  );
}

/** One recent project: open it, or remove it from the list. */
function RecentItem({
  entry,
  lifecycle,
  idle,
}: {
  entry: RecentEntry;
  lifecycle: ProjectLifecycle;
  idle: boolean;
}) {
  const nameId = useId();
  const detailsId = useId();
  const name = shownName(entry.projectName);
  const path = shownText(entry.displayPath, MAX_SHOWN_PATH_CHARS);
  const opened = localDateTime(entry.lastOpenedAt);
  return (
    <li className="recent-item" data-testid="recent-item">
      <button
        type="button"
        className="recent-open"
        aria-labelledby={nameId}
        aria-describedby={detailsId}
        aria-disabled={!idle}
        onClick={() => {
          if (idle) {
            run(() => lifecycle.openRecent(entry));
          }
        }}
      >
        <span id={nameId} className="recent-name">
          <bdi>{name}</bdi>
        </span>
        <span id={detailsId} className="recent-details">
          <span className="recent-path">
            <bdi>{path}</bdi>
          </span>
          {opened !== null && (
            <span className="recent-time">
              {' · Opened '}
              <time dateTime={entry.lastOpenedAt}>{opened}</time>
            </span>
          )}
        </span>
      </button>
      <button
        type="button"
        className="recent-remove"
        aria-label={`Remove “${name}” from the list`}
        title="Remove from the list"
        aria-disabled={!idle}
        onClick={() => {
          if (idle) {
            run(() => lifecycle.removeRecent(entry.recentId));
          }
        }}
      >
        <span aria-hidden="true">×</span>
      </button>
    </li>
  );
}
