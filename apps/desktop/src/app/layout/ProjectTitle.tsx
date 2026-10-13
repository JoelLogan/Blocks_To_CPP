/**
 * The project's name in the top bar (docs/spec/04-user-interface.md §4.1), followed by `•` while
 * there are unsaved changes, and renaming the project in place.
 *
 * The name is a button. Pressing it (or choosing *Rename project…* in the ≡ menu, the command
 * `project.rename` this component provides while a project is open) turns it into a text field:
 * `Enter` or moving the focus away keeps the new name, `Escape` cancels, and `Ctrl+S`, `Ctrl+B` and
 * `F5` keep it before they save, build or run (../shortcuts.ts). A name is trimmed and has
 * 1–100 characters on one line; the compiler core's loader checks the rest of the project-text
 * rules (no bidirectional controls, 05 §5.6) through `canonical()`. `Enter` (or one of those
 * keys) with a name that breaks a rule keeps the field open and says why, and the key does nothing
 * else; moving the focus away keeps the old name.
 * `Enter` and `Escape` return the focus to the name.
 *
 * Renaming goes through the editor (`EditorHandle.updateProjectInfo`): the project has unsaved
 * changes and the generated code's header comment follows. It is not undoable, and it never
 * renames the file (D12).
 */
import type { BdmDocument, CoreWasm } from '@blocks2cpp/b2c-core-wasm';
import {
  type KeyboardEvent as ReactKeyboardEvent,
  useCallback,
  useEffect,
  useId,
  useRef,
  useState,
} from 'react';
import { useShallow } from 'zustand/react/shallow';

import { type CommandRegistry, commands } from '../commands';
import { getCore } from '../core';
import { type EditorHandle, getEditorHandle } from '../editor-types';
import { registerPendingEdit } from '../shortcuts';
import { useAppStore } from '../store';
import { Hint } from '../ui/Hint';
import './ProjectTitle.css';

/** The top bar's name when no project is open. */
export const APP_NAME = 'Blocks2Cpp';

/** The longest project name, in characters (Unicode code points; 04 §4.1, D12). */
export const MAX_PROJECT_NAME_CHARS = 100;

/** The loader's code for a bidirectional control character in project text (05 §5.6). */
const BIDI_CODE = 'B2C-E0126';

/**
 * Line breaks, tabs and every other control character (C0, DEL and C1), and the Unicode line and
 * paragraph separators: a name is one line of plain text.
 */
const NOT_ONE_LINE = /[\p{Cc}\u2028\u2029]/u;

/** What the name's hint says. */
const RENAME_HINT = 'Rename the project';

/** What {@link checkProjectName} decided: the name to use, or why it cannot be used. */
export type ProjectNameCheck =
  { readonly ok: true; readonly name: string } | { readonly ok: false; readonly message: string };

/** `doc` with the project named `name`. */
function withName(doc: BdmDocument, name: string): BdmDocument {
  return { ...doc, project: { ...doc.project, name } };
}

/**
 * Checks `text` as a new name for the project whose document is `doc`: trimmed, 1–100
 * characters, one line, and accepted by the compiler core's loader in the project (which checks
 * the project-text rules). `core` is `null` before the core has started; the name then cannot be
 * checked yet.
 */
export function checkProjectName(
  text: string,
  doc: BdmDocument,
  core: CoreWasm | null,
): ProjectNameCheck {
  const name = text.trim();
  if (name === '') {
    return { ok: false, message: 'Type a name for the project.' };
  }
  // Code points, so a character outside the Basic Multilingual Plane counts once; unlike grapheme
  // clusters, they also bound the name's size.
  const length = Array.from(name).length;
  if (length > MAX_PROJECT_NAME_CHARS) {
    return {
      ok: false,
      message: `A project name can have at most ${String(MAX_PROJECT_NAME_CHARS)} characters; this one has ${String(length)}.`,
    };
  }
  if (NOT_ONE_LINE.test(name)) {
    return {
      ok: false,
      message: 'A project name is one line of text, without tabs or other control characters.',
    };
  }
  if (core === null) {
    return {
      ok: false,
      message: 'The name cannot be checked until Blocks2Cpp has finished starting. Try again.',
    };
  }
  let codes: string[];
  try {
    const result = core.canonical(JSON.stringify(withName(doc, name)));
    if (result.ok) {
      return { ok: true, name };
    }
    codes = result.diagnostics.map((diagnostic) => diagnostic.code);
  } catch (error: unknown) {
    console.error('The compiler core could not check a project name', error);
    return { ok: false, message: 'The name could not be checked. Try again.' };
  }
  if (codes.includes(BIDI_CODE)) {
    return {
      ok: false,
      message:
        'A project name cannot contain invisible characters that change the direction of text.',
    };
  }
  return { ok: false, message: `This name cannot be used (${[...new Set(codes)].join(', ')}).` };
}

/** Where {@link ProjectTitle} reaches the rest of the app (tests pass their own). */
export interface ProjectTitleProps {
  /** Where `project.rename` is registered; the app's registry by default. */
  readonly registry?: CommandRegistry;
  /** The block editor, which applies the new name; the app's editor by default. */
  readonly editor?: () => EditorHandle | null;
  /** The compiler core, which checks the name; the app's core by default. */
  readonly core?: () => CoreWasm | null;
}

/**
 * The project's name with the unsaved-changes marker, or the app's name without a project. The
 * name alone is cut off with an ellipsis when it is too long, so the marker after it always stays
 * in view.
 */
export function ProjectTitle(props: ProjectTitleProps) {
  const { handle, name, dirty } = useAppStore(
    useShallow((state) => ({
      handle: state.project?.handle ?? null,
      name: state.project?.document.project.name ?? null,
      dirty: state.project?.dirty ?? false,
    })),
  );
  return (
    <div className="toolbar-title">
      <span className="app-mark" aria-hidden="true" />
      {handle === null || name === null ? (
        <h1 className="project-name">
          <span className="project-name-text">{APP_NAME}</span>
        </h1>
      ) : (
        // A new project starts with the name shown, never in the middle of renaming another.
        <ProjectName key={handle} name={name} dirty={dirty} {...props} />
      )}
    </div>
  );
}

/** The open project's name as a button that renames it; see the module comment. */
function ProjectName({
  name,
  dirty,
  registry = commands,
  editor = getEditorHandle,
  core = getCore,
}: ProjectTitleProps & { name: string; dirty: boolean }) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState('');
  const [error, setError] = useState<string | null>(null);
  const button = useRef<HTMLButtonElement>(null);
  const input = useRef<HTMLInputElement>(null);
  /** Set when `Enter` or `Escape` ends the editing, so the blur that follows changes nothing. */
  const ended = useRef(false);
  /** Set when the name should take the focus back once it is shown again. */
  const refocus = useRef(false);
  const hintId = useId();
  const errorId = useId();

  const begin = useCallback(() => {
    ended.current = false;
    setDraft(name);
    setError(null);
    setEditing(true);
    // Already editing (the menu was used meanwhile): the field takes the focus again.
    input.current?.focus();
  }, [name]);

  useEffect(() => registry.registerCommand('project.rename', begin), [registry, begin]);

  // The field takes the focus with its text selected, ready to be typed over.
  useEffect(() => {
    if (editing) {
      input.current?.focus();
      input.current?.select();
    } else if (refocus.current) {
      refocus.current = false;
      button.current?.focus();
    }
  }, [editing]);

  /** Ends the editing; with `focusName`, the focus goes back to the name. */
  const finish = (focusName: boolean) => {
    ended.current = true;
    refocus.current = focusName;
    setEditing(false);
    setError(null);
  };

  /**
   * Applies the draft if it is a valid name, or shows why it is not; returns whether the editing
   * may end (a valid or unchanged name).
   */
  const apply = (): boolean => {
    const project = useAppStore.getState().project;
    if (project === null) {
      return true;
    }
    const checked = checkProjectName(draft, project.document, core());
    if (!checked.ok) {
      setError(checked.message);
      return false;
    }
    if (checked.name === project.document.project.name) {
      return true;
    }
    const target = editor();
    if (target === null) {
      setError('The project cannot be renamed until the block editor has started. Try again.');
      return false;
    }
    target.updateProjectInfo({ name: checked.name });
    return true;
  };

  // While the field is open, Save, Build and Run keep the name first, as Enter does, so they act on
  // the name shown; a name that breaks a rule stops them and says why. (Re-registered at every
  // render, so it applies the draft as it is now.)
  useEffect(() => {
    if (!editing) {
      return undefined;
    }
    return registerPendingEdit(() => {
      if (!apply()) {
        return false;
      }
      finish(true);
      return true;
    });
  });

  const onKeyDown = (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'Enter' && !event.nativeEvent.isComposing) {
      event.preventDefault();
      if (apply()) {
        finish(true);
      }
    } else if (event.key === 'Escape') {
      // The field's own: nothing else (a dialog, the canvas) closes or cancels with it.
      event.preventDefault();
      event.stopPropagation();
      finish(true);
    }
  };

  const onBlur = () => {
    if (ended.current) {
      return;
    }
    // Moving away keeps a valid name; a name that breaks a rule is dropped, as with Escape.
    apply();
    finish(false);
  };

  if (editing) {
    return (
      <div className="project-name-editor">
        <input
          ref={input}
          type="text"
          className="project-name-input"
          data-testid="project-name-input"
          aria-label="Project name"
          aria-invalid={error !== null}
          aria-describedby={error === null ? undefined : errorId}
          spellCheck={false}
          autoComplete="off"
          value={draft}
          onChange={(event) => {
            setDraft(event.target.value);
            setError(null);
          }}
          onKeyDown={onKeyDown}
          onBlur={onBlur}
        />
        {error !== null && (
          <p id={errorId} className="project-name-error" role="alert">
            {error}
          </p>
        )}
      </div>
    );
  }

  return (
    <>
      <h1 className="project-name" data-testid="project-name">
        <Hint text={RENAME_HINT}>
          <button
            ref={button}
            type="button"
            className="project-name-button"
            data-testid="project-name-button"
            aria-describedby={hintId}
            onClick={begin}
          >
            <bdi className="project-name-text" data-testid="project-name-text">
              {name}
            </bdi>
          </button>
        </Hint>
        {/* The marker's space keeps the heading's text as words; on screen, the gap spaces it. */}
        {dirty && (
          <>
            <span className="dirty-marker" aria-hidden="true">
              {' •'}
            </span>
            <span className="visually-hidden"> (unsaved changes)</span>
          </>
        )}
      </h1>
      <span id={hintId} className="visually-hidden">
        {RENAME_HINT}
      </span>
    </>
  );
}
