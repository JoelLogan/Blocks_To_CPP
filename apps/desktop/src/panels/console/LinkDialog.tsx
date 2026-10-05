import * as Dialog from '@radix-ui/react-dialog';
import { useState } from 'react';

import '../panels.css';
import { copyPlainText } from '../shared/clipboard';
import { visibleInvisibles } from '../shared/invisibles';

/** The props of {@link LinkDialog}. */
export interface LinkDialogProps {
  /** The checked `http`/`https` URL to show, or `null` when the dialog is closed. */
  url: string | null;
  /** The person closed the dialog. */
  onClose: () => void;
}

/**
 * Shows a link the running program printed (OSC 8) in full, with *Copy link*. Nothing is opened:
 * the program controls the link, and in M2 the app has no way to open it (04 §4.5). An accessible
 * Radix dialog: it traps focus, closes with Escape and returns focus to the console.
 */
export function LinkDialog({ url, onClose }: LinkDialogProps) {
  const [status, setStatus] = useState<'idle' | 'copied' | 'failed'>('idle');

  return (
    <Dialog.Root
      open={url !== null}
      onOpenChange={(open) => {
        if (!open) {
          setStatus('idle');
          onClose();
        }
      }}
    >
      <Dialog.Portal>
        <Dialog.Overlay className="b2c-dialog-overlay" />
        <Dialog.Content className="b2c-dialog" data-testid="link-dialog">
          <Dialog.Title>Link from your program</Dialog.Title>
          <Dialog.Description>
            Your program printed this link. Blocks2Cpp does not open links from programs; copy it if
            you want to open it in your browser yourself.
          </Dialog.Description>
          <p className="b2c-dialog-url" data-testid="link-dialog-url">
            {visibleInvisibles(url ?? '')}
          </p>
          <div className="b2c-dialog-actions">
            <span className="b2c-panel-status" role="status">
              {status === 'copied' ? 'Link copied' : status === 'failed' ? 'Could not copy' : ''}
            </span>
            <button
              type="button"
              onClick={() => {
                if (url !== null) {
                  void copyPlainText(url).then((ok) => {
                    setStatus(ok ? 'copied' : 'failed');
                  });
                }
              }}
            >
              Copy link
            </button>
            <Dialog.Close asChild>
              <button type="button">Close</button>
            </Dialog.Close>
          </div>
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
