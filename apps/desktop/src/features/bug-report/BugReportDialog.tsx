/**
 * The *Copy bug report* dialog (docs/plans/m3-contracts/w4-editor-tools.md §2.7): the report as it
 * will be copied, a tick to include the project's blocks and generated C++ (off), *Copy* and
 * *Close*. Nothing is sent anywhere; the report goes on the clipboard only when *Copy* is pressed.
 * Built on Radix's accessible dialog, like the app's other dialogs; texts are React text.
 */
import * as Dialog from '@radix-ui/react-dialog';
import { useId, useMemo, useState } from 'react';

import { copyText } from './copy';
import { bugReport, type BugReportInput } from './report';
import './bugReport.css';

/** The props of {@link BugReportDialog}. */
export interface BugReportDialogProps {
  /** What the report is made from. */
  readonly input: BugReportInput;
  /** The dialog was closed. */
  readonly onClose: () => void;
  /** Copies the text (the clipboard by default; tests pass their own). */
  readonly copy?: (text: string) => Promise<boolean>;
}

/** The dialog (see the module comment); render it while it is open. */
export function BugReportDialog({ input, onClose, copy = copyText }: BugReportDialogProps) {
  const [includeProject, setIncludeProject] = useState(false);
  const [status, setStatus] = useState('');
  const tickId = useId();
  const text = useMemo(() => bugReport(input, { includeProject }), [input, includeProject]);

  return (
    <Dialog.Root
      open
      onOpenChange={(open) => {
        if (!open) {
          onClose();
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="dialog-overlay" />
        <Dialog.Content className="dialog bug-report-dialog" data-testid="bug-report-dialog">
          <Dialog.Title className="dialog-title">Copy bug report</Dialog.Title>
          <Dialog.Description className="dialog-message">
            Nothing is sent anywhere. Read the report, copy it, then paste it into a new issue on
            the project’s page.
          </Dialog.Description>
          <div className="bug-report-tick">
            <input
              id={tickId}
              type="checkbox"
              checked={includeProject}
              onChange={(event) => {
                setIncludeProject(event.target.checked);
                setStatus('');
              }}
            />
            <label htmlFor={tickId}>
              Include my blocks and the generated C++ (they help find the bug)
            </label>
          </div>
          {/* The report scrolls: it takes the focus so that the keyboard can scroll it. */}
          {/* eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- see above. */}
          <pre className="bug-report-text" data-testid="bug-report-text" tabIndex={0}>
            {text}
          </pre>
          <p className="bug-report-status" role="status" aria-live="polite">
            {status}
          </p>
          <div className="dialog-actions">
            <Dialog.Close asChild>
              <button type="button" className="button">
                Close
              </button>
            </Dialog.Close>
            <button
              type="button"
              className="button button-primary"
              onClick={() => {
                copy(text)
                  .then((copied) => {
                    setStatus(
                      copied
                        ? 'The report is on the clipboard.'
                        : 'The report could not be copied. Select its text and copy it instead.',
                    );
                  })
                  .catch(() => {
                    setStatus(
                      'The report could not be copied. Select its text and copy it instead.',
                    );
                  });
              }}
            >
              Copy
            </button>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
