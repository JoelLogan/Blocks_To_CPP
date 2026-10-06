/**
 * What the user is told when a clipboard action cannot be done: a payload the loader refuses
 * (with its `B2C-E01xx` problems, 05 §5.12), blocks that would take the project past the limits
 * of a project file where they would go (05 §5.6), or a compiler core that is not available.
 * Unresolved references after a successful paste need no notice: the analyser marks them on the
 * blocks (`B2C-E0201`).
 *
 * Messages are plain text, shown by the app's accessible dialogs as React text; the loader's
 * messages never contain control, bidirectional or invisible characters (05 §5.6).
 */
import type { Diagnostic } from '@blocks2cpp/b2c-core-wasm';

import type { DialogService } from '../../app/dialogs/service';

/** A clipboard action of the editor. */
export type ClipboardAction = 'copy' | 'cut' | 'paste' | 'duplicate';

/** Why an action could not run at all. */
export type UnavailableReason =
  /** The compiler core has not started yet. */
  | 'coreNotStarted'
  /** The compiler core stopped (a trap); a new one is being started. */
  | 'coreStopped'
  /** A bug in Blocks2Cpp (an unexpected error); details are in the log. */
  | 'internal';

/** Something the user is told about a clipboard action. */
export type ClipboardNotice =
  /** The loader refused the payload (paste) or the document (copy): nothing changed. */
  | {
      readonly kind: 'refused';
      readonly action: ClipboardAction;
      readonly diagnostics: readonly Diagnostic[];
    }
  /**
   * The payload is fine, but the project with the blocks at the target would break the limits of
   * a project file (05 §5.6), for example nesting too deep: nothing changed.
   */
  | {
      readonly kind: 'limits';
      readonly action: ClipboardAction;
      readonly diagnostics: readonly Diagnostic[];
    }
  /** The action could not run: nothing changed. */
  | {
      readonly kind: 'unavailable';
      readonly action: ClipboardAction;
      readonly reason: UnavailableReason;
    };

/** Shows a clipboard notice to the user. */
export type ClipboardNotifier = (notice: ClipboardNotice) => void;

/** The most problems a notice lists; the rest are counted. */
export const MAX_LISTED_PROBLEMS = 5;

/** The longest problem message shown, in characters; longer ones end with `…`. */
export const MAX_PROBLEM_CHARS = 300;

/** The title of a notice for each action. */
const TITLES: Readonly<Record<ClipboardAction, string>> = {
  copy: 'The blocks could not be copied',
  cut: 'The blocks could not be cut',
  paste: 'The blocks could not be pasted',
  duplicate: 'The block could not be duplicated',
};

/** What the notice says before listing the loader's problems. */
const REFUSED: Readonly<Record<ClipboardAction, string>> = {
  copy: 'The blocks could not be read for copying, so the clipboard was not changed.',
  cut: 'The blocks could not be read for cutting, so nothing was changed.',
  paste: 'The clipboard does not hold blocks that Blocks2Cpp can use, so nothing was pasted.',
  duplicate: 'The block could not be read for duplicating, so nothing was changed.',
};

/** What the notice says before listing the problems of the project with the blocks inserted. */
const LIMITS: Readonly<Record<ClipboardAction, string>> = {
  copy: 'The blocks could not be copied without breaking the limits of a project file.',
  cut: 'The blocks could not be cut without breaking the limits of a project file.',
  paste:
    'Pasting these blocks here would take the project past the limits of a project file, so nothing was pasted.',
  duplicate:
    'Duplicating this block would take the project past the limits of a project file, so nothing was changed.',
};

/** What the notice says when the action could not run. */
const UNAVAILABLE: Readonly<Record<UnavailableReason, string>> = {
  coreNotStarted: 'The compiler core is still starting. Try again in a moment.',
  coreStopped:
    'The compiler core stopped unexpectedly and is being restarted. Nothing was changed; try again in a moment.',
  internal: 'Something went wrong inside Blocks2Cpp. Nothing was changed.',
};

/** `text` cut to {@link MAX_PROBLEM_CHARS} characters (whole code points). */
function shorten(text: string): string {
  const chars = Array.from(text);
  return chars.length > MAX_PROBLEM_CHARS
    ? `${chars.slice(0, MAX_PROBLEM_CHARS - 1).join('')}…`
    : text;
}

/** One line per problem (at most {@link MAX_LISTED_PROBLEMS}), and how many more there are. */
function problemLines(diagnostics: readonly Diagnostic[]): string[] {
  const lines = diagnostics
    .slice(0, MAX_LISTED_PROBLEMS)
    .map((diagnostic) => `${diagnostic.code}: ${shorten(diagnostic.message)}`);
  const more = diagnostics.length - MAX_LISTED_PROBLEMS;
  if (more > 0) {
    lines.push(more === 1 ? 'and 1 more problem' : `and ${String(more)} more problems`);
  }
  return lines;
}

/** The title and message of a notice. */
export function describeNotice(notice: ClipboardNotice): { title: string; message: string } {
  const title = TITLES[notice.action];
  if (notice.kind === 'unavailable') {
    return { title, message: UNAVAILABLE[notice.reason] };
  }
  const lead = notice.kind === 'limits' ? LIMITS[notice.action] : REFUSED[notice.action];
  const lines = problemLines(notice.diagnostics);
  const message = lines.length === 0 ? lead : `${lead}\n\n${lines.join('\n')}`;
  return { title, message };
}

/**
 * A notifier that shows each notice in an alert of the app's dialogs. While one of its notices is
 * open, further ones are only logged, so a held key cannot queue a pile of dialogs.
 */
export function dialogNotifier(dialogs: Pick<DialogService, 'alert'>): ClipboardNotifier {
  let open = false;
  return (notice) => {
    const { title, message } = describeNotice(notice);
    if (open) {
      console.warn(`${title} (not shown: another clipboard notice is open)`);
      return;
    }
    open = true;
    dialogs
      .alert({ title, message })
      .catch((error: unknown) => {
        console.error('A clipboard notice could not be shown', error);
      })
      .finally(() => {
        open = false;
      });
  };
}
