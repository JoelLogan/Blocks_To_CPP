import { copyPlainText } from '../../panels/shared/clipboard';
import { useTransientStatus } from '../../panels/shared/useTransientStatus';

/**
 * A command to type in a terminal, with a *Copy* button. The command is plain selectable text, so
 * it can also be copied by hand when the clipboard refuses (the button then says so, until the
 * next attempt). A successful copy says *Copied* for 3 seconds (04 §4.6).
 */
export function CopyCommand({ command }: { command: string }) {
  const status = useTransientStatus();

  const copy = async () => {
    if (await copyPlainText(command)) {
      status.show('success', 'Copied');
    } else {
      status.show('failure', 'Could not copy: select the command and press Ctrl+C');
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
        {status.shown?.text}
      </span>
    </div>
  );
}
