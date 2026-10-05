import { useEffect, useRef, useState } from 'react';

import { copyPlainText } from '../../panels/shared/clipboard';

/** What the last copy did. */
type CopyState = 'idle' | 'copied' | 'failed';

/**
 * A command to type in a terminal, with a *Copy* button. The command is plain selectable text, so
 * it can also be copied by hand when the clipboard refuses (the button then says so).
 */
export function CopyCommand({ command }: { command: string }) {
  const [state, setState] = useState<CopyState>('idle');
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const copy = async () => {
    const copied = await copyPlainText(command);
    if (mounted.current) {
      setState(copied ? 'copied' : 'failed');
    }
  };

  return (
    <div className="copy-command">
      <code className="copy-command-text" translate="no">
        {command}
      </code>
      <button
        type="button"
        className="button copy-command-button"
        onClick={() => {
          void copy();
        }}
      >
        Copy<span className="visually-hidden"> the command {command}</span>
      </button>
      <span className="copy-command-status" role="status">
        {state === 'copied' && 'Copied'}
        {state === 'failed' && 'Could not copy: select the command and press Ctrl+C'}
      </span>
    </div>
  );
}
