import * as Dialog from '@radix-ui/react-dialog';
import { type ReactNode, type SyntheticEvent, useCallback, useId, useRef, useState } from 'react';
import { useStore } from 'zustand';

import {
  type AlertOptions,
  type ChoiceOptions,
  type ConfirmOptions,
  DEFAULT_PROMPT_MAX_LENGTH,
  type DialogQueue,
  type DialogRequest,
  type DialogTextOptions,
  type PromptOptions,
  settleCancelled,
} from './service';

/**
 * Shows the dialog at the head of `queue`, one at a time, as a modal Radix dialog: focus moves
 * into it and stays there until it closes, Escape dismisses it, and focus returns to where it was.
 * Mount it once, at the root of the window.
 */
export function DialogHost({ queue }: { queue: DialogQueue }) {
  const request = useStore(queue.store, (state) => state.queue[0]);
  if (request === undefined) {
    return null;
  }
  // A new key for each request: no state carries over from the previous dialog.
  return <QueuedDialog key={request.id} request={request} />;
}

function QueuedDialog({ request }: { request: DialogRequest }) {
  /** The element that gets the focus when the dialog opens. */
  const initialFocus = useRef<HTMLElement | null>(null);
  const focusRef = useCallback((element: HTMLElement | null) => {
    initialFocus.current = element;
  }, []);
  const { onOpen } = request.options;
  /** Hands the dialog's element to `onOpen` once it is in the document (the portal renders late). */
  const contentRef = useCallback(
    (element: HTMLDivElement | null) => {
      if (element === null || onOpen === undefined) {
        return undefined;
      }
      const release = onOpen(element);
      return () => {
        release?.();
      };
    },
    [onOpen],
  );

  let body: ReactNode;
  switch (request.kind) {
    case 'alert':
      body = <AlertBody options={request.options} onDone={request.settle} focus={focusRef} />;
      break;
    case 'confirm':
      body = <ConfirmBody options={request.options} onDone={request.settle} focus={focusRef} />;
      break;
    case 'prompt':
      body = <PromptBody options={request.options} onDone={request.settle} focus={focusRef} />;
      break;
    case 'choice':
      body = <ChoiceBody options={request.options} onDone={request.settle} focus={focusRef} />;
      break;
  }

  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open) {
          settleCancelled(request);
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content
          ref={contentRef}
          className="dialog"
          data-testid="app-dialog"
          // Without a separate title the message is the heading, and there is no description.
          {...(request.options.title === undefined ? { 'aria-describedby': undefined } : {})}
          onOpenAutoFocus={(event) => {
            const target = initialFocus.current;
            if (target !== null) {
              event.preventDefault();
              target.focus();
              if (target instanceof HTMLInputElement) {
                target.select();
              }
            }
          }}
        >
          <DialogText options={request.options} />
          {body}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}

/** The heading and, when there is a separate title, the message. */
function DialogText({ options }: { options: DialogTextOptions }) {
  if (options.title === undefined) {
    return <Dialog.Title className="dialog-title">{options.message}</Dialog.Title>;
  }
  return (
    <>
      <Dialog.Title className="dialog-title">{options.title}</Dialog.Title>
      <Dialog.Description className="dialog-message">{options.message}</Dialog.Description>
    </>
  );
}

interface BodyProps<O, R> {
  options: O;
  onDone: (result: R) => void;
  /** The ref for the element that gets the focus when the dialog opens. */
  focus: (element: HTMLElement | null) => void;
}

function AlertBody({ options, onDone, focus }: BodyProps<AlertOptions, void>) {
  return (
    <div className="dialog-actions">
      <button
        ref={focus}
        type="button"
        className="button button-primary"
        onClick={() => {
          onDone();
        }}
      >
        {options.okLabel ?? 'OK'}
      </button>
    </div>
  );
}

function ConfirmBody({ options, onDone, focus }: BodyProps<ConfirmOptions, boolean>) {
  const destructive = options.destructive === true;
  return (
    <div className="dialog-actions">
      <button
        ref={destructive ? focus : undefined}
        type="button"
        className="button"
        onClick={() => {
          onDone(false);
        }}
      >
        {options.cancelLabel ?? 'Cancel'}
      </button>
      <button
        ref={destructive ? undefined : focus}
        type="button"
        className={destructive ? 'button button-danger' : 'button button-primary'}
        onClick={() => {
          onDone(true);
        }}
      >
        {options.confirmLabel ?? 'OK'}
      </button>
    </div>
  );
}

function PromptBody({ options, onDone, focus }: BodyProps<PromptOptions, string | null>) {
  const maxLength = Math.min(
    Math.max(1, Math.floor(options.maxLength ?? DEFAULT_PROMPT_MAX_LENGTH)),
    DEFAULT_PROMPT_MAX_LENGTH,
  );
  const [value, setValue] = useState(() => (options.defaultValue ?? '').slice(0, maxLength));
  const inputId = useId();
  const errorId = useId();
  const problem = options.validate?.(value) ?? null;

  const submit = (event: SyntheticEvent) => {
    event.preventDefault();
    if (problem === null) {
      onDone(value.slice(0, maxLength));
    }
  };

  return (
    <form className="dialog-form" onSubmit={submit} noValidate>
      <label className="visually-hidden" htmlFor={inputId}>
        {options.message}
      </label>
      <input
        ref={focus}
        id={inputId}
        className="dialog-input"
        type="text"
        value={value}
        maxLength={maxLength}
        spellCheck={false}
        autoComplete="off"
        aria-invalid={problem !== null}
        aria-describedby={problem === null ? undefined : errorId}
        onChange={(event) => {
          setValue(event.target.value);
        }}
      />
      {problem !== null && (
        <p id={errorId} className="dialog-error" role="alert">
          <span aria-hidden="true">✖ </span>
          {problem}
        </p>
      )}
      <div className="dialog-actions">
        <button
          type="button"
          className="button"
          onClick={() => {
            onDone(null);
          }}
        >
          {options.cancelLabel ?? 'Cancel'}
        </button>
        <button type="submit" className="button button-primary" aria-disabled={problem !== null}>
          {options.okLabel ?? 'OK'}
        </button>
      </div>
    </form>
  );
}

function ChoiceBody({ options, onDone, focus }: BodyProps<ChoiceOptions<string>, string>) {
  const focusIndex = Math.max(
    0,
    options.choices.findIndex((choice) => choice.primary === true),
  );
  return (
    <div className="dialog-actions">
      {options.choices.map((choice, index) => (
        <button
          key={choice.id}
          ref={index === focusIndex ? focus : undefined}
          type="button"
          className={
            choice.destructive === true
              ? 'button button-danger'
              : choice.primary === true
                ? 'button button-primary'
                : 'button'
          }
          onClick={() => {
            onDone(choice.id);
          }}
        >
          {choice.label}
        </button>
      ))}
    </div>
  );
}
