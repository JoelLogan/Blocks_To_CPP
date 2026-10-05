import type { BootPhase } from './bootstrap';

/** Shown while the window asks the backend what it is (a moment at most). */
export function StartingScreen() {
  return (
    <main className="startup" aria-label="Blocks2Cpp">
      <p className="startup-status" role="status">
        Starting Blocks2Cpp…
      </p>
    </main>
  );
}

/**
 * The blocking error when the window cannot work with its backend (docs/spec/02-architecture.md
 * §2.5.7): the two always ship together, so a mismatch means a broken installation. Only codes and
 * version numbers are shown; nothing the backend said is displayed.
 */
export function StartupErrorScreen({
  phase,
}: {
  phase: Extract<BootPhase, { kind: 'versionMismatch' | 'failed' }>;
}) {
  return (
    <main className="startup" aria-labelledby="startup-error-title">
      <div className="startup-error" role="alert">
        <h1 id="startup-error-title" className="startup-title">
          <span aria-hidden="true">✖ </span>
          Blocks2Cpp cannot start
        </h1>
        {phase.kind === 'versionMismatch' ? (
          <>
            <p>
              This window and the program behind it come from different versions of Blocks2Cpp, so
              they cannot work together.
            </p>
            <p>Install Blocks2Cpp again to repair it.</p>
            <p className="startup-details" data-testid="startup-details">
              Technical details: the window speaks IPC version {String(phase.frontend)}, the program
              behind it{' '}
              {phase.backend === null
                ? 'did not say which version it speaks'
                : `speaks IPC version ${String(phase.backend)}`}
              .
            </p>
          </>
        ) : (
          <>
            <p>The window could not reach the program behind it.</p>
            <p>Close Blocks2Cpp and start it again. If this keeps happening, install it again.</p>
            <p className="startup-details" data-testid="startup-details">
              Technical details: {phase.code}.
            </p>
          </>
        )}
      </div>
    </main>
  );
}
