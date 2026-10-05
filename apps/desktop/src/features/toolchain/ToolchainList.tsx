import type {
  CppStandard,
  Diagnostic,
  Toolchain,
  ToolchainId,
  ToolchainSource,
} from '@blocks2cpp/ipc-types';
import { displayText, problemViews, toolchainName } from './reasons';

/** The most toolchains listed; discovery finds a handful, so this only bounds a broken answer. */
export const MAX_LISTED_TOOLCHAINS = 100;

/** The longest location shown, in UTF-16 code units. */
export const MAX_PATH_LENGTH = 1024;

const SOURCE_LABELS: Record<ToolchainSource, string> = {
  path: 'On the PATH',
  wellKnown: 'In a usual install folder',
  manual: 'Chosen by hand',
};

const STANDARD_LABELS: Record<CppStandard, string> = {
  'c++17': 'C++17',
  'c++20': 'C++20',
  'c++23': 'C++23',
  'c++26': 'C++26',
};

const SEVERITY_LABELS: Record<Diagnostic['severity'], string> = {
  error: 'Error',
  warning: 'Warning',
  info: 'Note',
};

const SEVERITY_GLYPHS: Record<Diagnostic['severity'], string> = {
  error: '✖',
  warning: '⚠',
  info: 'ℹ',
};

/** `Yes` or `No`. */
function yesNo(value: boolean): string {
  return value ? 'Yes' : 'No';
}

/** The props of {@link ToolchainList}. */
export interface ToolchainListProps {
  toolchains: readonly Toolchain[];
  /** Whether discovery is still running. */
  discovering: boolean;
  /** *Select as default*. */
  onSelect: (id: ToolchainId) => void;
  /** Whether another action runs, so the buttons wait. */
  busy: boolean;
  /** The toolchain being selected, if any. */
  selecting: ToolchainId | null;
}

/**
 * The toolchain list (docs/spec/04-user-interface.md §4.6): every compiler found or added, with
 * its version, target, flavour, location, capabilities and health checks, and *Select as default*
 * for the usable ones. Problems list their code, what kind of problem it is and what to do.
 */
export function ToolchainList({
  toolchains,
  discovering,
  onSelect,
  busy,
  selecting,
}: ToolchainListProps) {
  const shown = toolchains.slice(0, MAX_LISTED_TOOLCHAINS);
  if (shown.length === 0) {
    return (
      <p className="feature-muted" data-testid="toolchain-list-empty">
        {discovering ? 'Looking for compilers…' : 'No compilers were found on this computer yet.'}
      </p>
    );
  }
  return (
    <ul className="toolchain-list" data-testid="toolchain-list">
      {shown.map((toolchain) => (
        <ToolchainItem
          key={toolchain.id}
          toolchain={toolchain}
          onSelect={onSelect}
          busy={busy}
          selecting={selecting === toolchain.id}
        />
      ))}
    </ul>
  );
}

function ToolchainItem({
  toolchain,
  onSelect,
  busy,
  selecting,
}: {
  toolchain: Toolchain;
  onSelect: (id: ToolchainId) => void;
  busy: boolean;
  selecting: boolean;
}) {
  const name = toolchainName(toolchain);
  const { capabilities } = toolchain;
  const standards = capabilities.standards
    .filter((standard) => Object.hasOwn(STANDARD_LABELS, standard))
    .map((standard) => STANDARD_LABELS[standard]);

  return (
    <li className="toolchain-item" data-testid={`toolchain-${toolchain.id}`}>
      <div className="toolchain-item-header">
        <h4 className="toolchain-name">{name}</h4>
        <span
          className={`toolchain-health ${toolchain.usable ? 'toolchain-usable' : 'toolchain-unusable'}`}
        >
          <span aria-hidden="true">{toolchain.usable ? '√ ' : '✖ '}</span>
          {toolchain.usable ? 'Can build' : 'Cannot be used'}
        </span>
        {toolchain.selected && (
          <span className="toolchain-default" data-testid="toolchain-default">
            Default
          </span>
        )}
      </div>

      {toolchain.selected && !toolchain.usable && (
        <p className="toolchain-fallback">
          This compiler is selected as the default but cannot be used, so builds use the first
          usable compiler instead (B2C-T1022).
        </p>
      )}

      <dl className="toolchain-details">
        <dt>Version</dt>
        <dd>{toolchain.version === null ? 'Unknown' : displayText(toolchain.version, 64)}</dd>
        <dt>Target</dt>
        <dd>{toolchain.target === null ? 'Unknown' : displayText(toolchain.target, 128)}</dd>
        {toolchain.flavor !== null && (
          <>
            <dt>Flavour</dt>
            <dd>{displayText(toolchain.flavor, 64)}</dd>
          </>
        )}
        <dt>Location</dt>
        <dd className="feature-path" dir="ltr">
          <bdi>{displayText(toolchain.displayPath, MAX_PATH_LENGTH)}</bdi>
        </dd>
        <dt>Found</dt>
        <dd>{SOURCE_LABELS[toolchain.source]}</dd>
        <dt>C++ standards</dt>
        <dd>{standards.length === 0 ? 'None' : standards.join(', ')}</dd>
        <dt>std::format</dt>
        <dd>{yesNo(capabilities.stdFormat)}</dd>
        <dt>Sanitizers</dt>
        <dd>{yesNo(capabilities.sanitizers)}</dd>
        <dt>SARIF diagnostics</dt>
        <dd>{yesNo(capabilities.sarif)}</dd>
        <dt>Health checks</dt>
        <dd>
          {toolchain.problems.length === 0 ? (
            'All passed'
          ) : (
            <ProblemList diagnostics={toolchain.problems} />
          )}
        </dd>
      </dl>

      {toolchain.usable && !toolchain.selected && (
        <div className="feature-actions">
          <button
            type="button"
            className="button"
            aria-disabled={busy}
            onClick={() => {
              if (!busy) {
                onSelect(toolchain.id);
              }
            }}
          >
            {selecting ? 'Selecting…' : 'Select as default'}
            <span className="visually-hidden"> ({name})</span>
          </button>
        </div>
      )}
    </li>
  );
}

/**
 * The problems of a toolchain or of a refused file: each one's code, a short summary and what to
 * do, and the backend's own message.
 */
export function ProblemList({ diagnostics }: { diagnostics: readonly Diagnostic[] }) {
  const { shown, more } = problemViews(diagnostics);
  return (
    <ul className="toolchain-problems">
      {shown.map((problem, index) => (
        <li key={`${problem.code}-${String(index)}`} className="toolchain-problem">
          <span className="toolchain-problem-glyph" aria-hidden="true">
            {SEVERITY_GLYPHS[problem.severity]}
          </span>
          <div>
            <p>
              <span className="visually-hidden">{SEVERITY_LABELS[problem.severity]}: </span>
              <strong>{problem.title}</strong>{' '}
              <span className="toolchain-problem-code">({problem.code})</span>
            </p>
            <p>{problem.message}</p>
            {problem.fix !== null && <p className="feature-muted">How to fix: {problem.fix}</p>}
          </div>
        </li>
      ))}
      {more > 0 && (
        <li className="toolchain-problem feature-muted">
          and {String(more)} more {more === 1 ? 'problem' : 'problems'}
        </li>
      )}
    </ul>
  );
}
