import type { BdmBuildConfiguration, BdmProject, CppStandard } from '@blocks2cpp/b2c-core-wasm';
import type { RunInputFileAction } from '@blocks2cpp/ipc-types';
import { type ReactNode, useEffect, useId, useState } from 'react';

import type { FeatureContext } from '../../app/features';
import { useAppStore } from '../../app/store';
import { failureCode } from '../build-run/messages';
import { FeaturePage, leavePage, Notice, PageSection } from '../settings/page/FeaturePage';
import '../settings/page/page.css';
import {
  type ArgRow,
  type DefineRow,
  type DefineType,
  type Draft,
  draftFrom,
  draftProblems,
  MAX_ARG_LENGTH,
  MAX_ARGS,
  MAX_DEFINE_NAME,
  MAX_DEFINE_TEXT,
  MAX_DEFINES,
  rowKey,
  settingsChanged,
  settingsFrom,
  STANDARDS,
} from './model';
import './projectSettings.css';

/**
 * The project settings page (docs/spec/04-user-interface.md §4.1; ≡ → *Project settings…*): the
 * project's C++ standard, its Debug and Release configurations, its defines and how its program
 * runs (arguments and working folder), and the two run options kept on this computer (the input
 * file and the separate terminal window), which apply at once.
 *
 * The project's settings are a draft until *Save changes*: then they become one edit of the project
 * (it has unsaved changes, as after a rename, and Undo does not undo it), and the page closes.
 * *Cancel* and *Back* leave them as they were. A problem is shown under its field as the person
 * types (`aria-invalid`, `aria-describedby`); *Save changes* with problems moves the focus to the
 * first one and saves nothing.
 */
export function ProjectSettingsPage({ ctx }: { ctx: FeatureContext }) {
  const project = useAppStore((state) => state.project);
  const back = () => {
    leavePage(ctx.store);
  };
  if (project === null) {
    return (
      <FeaturePage
        title="Project settings"
        hasProject={false}
        onBack={back}
        testId="project-settings"
      >
        <Notice tone="info">No project is open.</Notice>
      </FeaturePage>
    );
  }
  return (
    <ProjectSettingsForm
      // A new form when another project opens.
      key={project.handle}
      ctx={ctx}
      project={project.document.project}
      onBack={back}
    />
  );
}

const OPTIMIZATIONS: readonly { value: BdmBuildConfiguration['optimization']; label: string }[] = [
  { value: 'none', label: 'None (-O0)' },
  { value: 'debug', label: 'For debugging (-Og)' },
  { value: 'speed', label: 'Speed (-O2)' },
  { value: 'size', label: 'Size (-Os)' },
];

const WARNINGS: readonly { value: BdmBuildConfiguration['warnings']; label: string }[] = [
  { value: 'minimal', label: 'Minimal (-Wall)' },
  { value: 'helpful', label: 'Helpful (-Wall -Wextra -Wpedantic)' },
  { value: 'strict', label: 'Strict (helpful and more)' },
];

const STANDARD_LABELS: Record<CppStandard, string> = {
  'c++17': 'C++17',
  'c++20': 'C++20',
  'c++23': 'C++23',
  'c++26': 'C++26',
};

const DEFINE_TYPES: readonly { value: DefineType; label: string }[] = [
  { value: 'int', label: 'Whole number' },
  { value: 'bool', label: 'True or false' },
  { value: 'string', label: 'Text' },
];

function ProjectSettingsForm({
  ctx,
  project,
  onBack,
}: {
  ctx: FeatureContext;
  project: BdmProject;
  onBack: () => void;
}) {
  const [draft, setDraft] = useState<Draft>(() => draftFrom(project));
  const [showAll, setShowAll] = useState(false);
  const problems = draftProblems(draft);
  const problemOf = (field: string) =>
    problems.find((problem) => problem.field === field)?.message ?? null;
  const update = (patch: Partial<Draft>) => {
    setDraft((current) => ({ ...current, ...patch }));
  };

  const save = () => {
    const first = problems[0];
    if (first !== undefined) {
      setShowAll(true);
      document.getElementById(first.field)?.focus();
      return;
    }
    const settings = settingsFrom(draft, project);
    if (settingsChanged(settings, project)) {
      const editor = ctx.editor();
      if (editor !== null) {
        editor.updateProjectInfo({ name: project.name, settings });
      } else {
        const open = ctx.store.getState().project;
        if (open !== null) {
          ctx.store.getState().actions.updateProject({
            document: {
              ...open.document,
              project: { ...open.document.project, ...settings },
            },
            dirty: true,
          });
        }
      }
    }
    onBack();
  };

  return (
    <FeaturePage title="Project settings" hasProject onBack={onBack} testId="project-settings">
      <p className="feature-muted">
        These settings are saved in the project. Compiler flags are never: Blocks2Cpp makes them
        from these choices.
      </p>

      <PageSection title="Language" testId="project-settings-language">
        <SelectField
          label="C++ standard"
          value={draft.standard}
          options={STANDARDS.map((value) => ({ value, label: STANDARD_LABELS[value] }))}
          onChange={(standard) => {
            update({ standard });
          }}
          testId="project-settings-standard"
        />
        <CheckField
          label="GNU extensions (gnu++ instead of c++)"
          checked={draft.gnuExtensions}
          onChange={(gnuExtensions) => {
            update({ gnuExtensions });
          }}
          testId="project-settings-gnu"
        />
      </PageSection>

      <PageSection title="Build" testId="project-settings-build">
        <ConfigurationFields
          name="Debug"
          value={draft.debug}
          onChange={(debug) => {
            update({ debug });
          }}
        />
        <ConfigurationFields
          name="Release"
          value={draft.release}
          onChange={(release) => {
            update({ release });
          }}
        />
        <DefinesField
          rows={draft.defines}
          problemOf={problemOf}
          showAll={showAll}
          onChange={(defines) => {
            update({ defines });
          }}
        />
      </PageSection>

      <PageSection title="Run" testId="project-settings-run">
        <ArgsField
          rows={draft.args}
          problemOf={problemOf}
          showAll={showAll}
          onChange={(args) => {
            update({ args });
          }}
        />
        <RadioField
          legend="Working folder"
          value={draft.workingDirectory}
          options={[
            { value: 'project', label: 'The project’s folder' },
            {
              value: 'sandbox',
              label: 'A sandbox folder for this project',
              description:
                'An empty folder of its own, which Blocks2Cpp keeps for 30 days after a run.',
            },
          ]}
          onChange={(workingDirectory) => {
            update({ workingDirectory });
          }}
          testId="project-settings-working-folder"
        />
        <MachineRunOptions ctx={ctx} />
      </PageSection>

      {showAll && problems.length > 0 && (
        <Notice tone="error" testId="project-settings-problems">
          {problems.length === 1
            ? 'One setting needs fixing before the changes can be saved.'
            : `${String(problems.length)} settings need fixing before the changes can be saved.`}
        </Notice>
      )}
      <div className="feature-actions">
        <button
          type="button"
          className="button button-primary"
          onClick={save}
          data-testid="project-settings-save"
        >
          Save changes
        </button>
        <button
          type="button"
          className="button"
          onClick={onBack}
          data-testid="project-settings-cancel"
        >
          Cancel
        </button>
      </div>
    </FeaturePage>
  );
}

/** A labelled drop-down. */
function SelectField<T extends string>({
  label,
  value,
  options,
  onChange,
  testId,
}: {
  label: string;
  value: T;
  options: readonly { value: T; label: string }[];
  onChange: (value: T) => void;
  testId: string;
}) {
  const id = useId();
  return (
    <div className="feature-field">
      <label className="feature-field-label" htmlFor={id}>
        {label}
      </label>
      <select
        id={id}
        className="feature-input"
        value={value}
        data-testid={testId}
        onChange={(event) => {
          const chosen = options.find((option) => option.value === event.target.value);
          if (chosen !== undefined) {
            onChange(chosen.value);
          }
        }}
      >
        {options.map((option) => (
          <option key={option.value} value={option.value}>
            {option.label}
          </option>
        ))}
      </select>
    </div>
  );
}

/** A labelled checkbox. */
function CheckField({
  label,
  checked,
  onChange,
  testId,
  description,
  disabled = false,
}: {
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  testId: string;
  description?: string;
  disabled?: boolean;
}) {
  const id = useId();
  const descriptionId = `${id}-description`;
  return (
    <div className="feature-option">
      <input
        id={id}
        type="checkbox"
        checked={checked}
        disabled={disabled}
        aria-describedby={description === undefined ? undefined : descriptionId}
        data-testid={testId}
        onChange={(event) => {
          onChange(event.target.checked);
        }}
      />
      <span className="feature-option-text">
        <label htmlFor={id}>{label}</label>
        {description !== undefined && (
          <span id={descriptionId} className="feature-muted">
            {description}
          </span>
        )}
      </span>
    </div>
  );
}

/** A group of radio buttons. */
function RadioField<T extends string>({
  legend,
  value,
  options,
  onChange,
  testId,
}: {
  legend: string;
  value: T;
  options: readonly { value: T; label: string; description?: string }[];
  onChange: (value: T) => void;
  testId: string;
}) {
  const name = useId();
  return (
    <fieldset className="feature-field" data-testid={testId}>
      <legend>{legend}</legend>
      {options.map((option) => {
        const id = `${name}-${option.value}`;
        const descriptionId = `${id}-description`;
        return (
          <div key={option.value} className="feature-option">
            <input
              type="radio"
              id={id}
              name={name}
              value={option.value}
              checked={value === option.value}
              aria-describedby={option.description === undefined ? undefined : descriptionId}
              onChange={() => {
                onChange(option.value);
              }}
            />
            <span className="feature-option-text">
              <label htmlFor={id}>{option.label}</label>
              {option.description !== undefined && (
                <span id={descriptionId} className="feature-muted">
                  {option.description}
                </span>
              )}
            </span>
          </div>
        );
      })}
    </fieldset>
  );
}

/** One configuration's choices. */
function ConfigurationFields({
  name,
  value,
  onChange,
}: {
  name: 'Debug' | 'Release';
  value: BdmBuildConfiguration;
  onChange: (value: BdmBuildConfiguration) => void;
}) {
  const lower = name.toLowerCase();
  const sanitizer = (which: 'address' | 'undefined', on: boolean) => {
    const others = value.sanitizers.filter((item) => item !== which);
    // In the file's order: address before undefined.
    const sanitizers = on
      ? (['address', 'undefined'] as const).filter(
          (item) => item === which || others.includes(item),
        )
      : others;
    onChange({ ...value, sanitizers: [...sanitizers] });
  };
  const warningsAsErrors = (on: boolean) => {
    const next = { ...value };
    if (on) {
      next.warningsAsErrors = true;
    } else {
      delete next.warningsAsErrors;
    }
    onChange(next);
  };
  return (
    <fieldset
      className="feature-field project-settings-configuration"
      data-testid={`project-settings-${lower}`}
    >
      <legend>{name}</legend>
      <SelectField
        label={`${name}: optimisation`}
        value={value.optimization}
        options={OPTIMIZATIONS}
        onChange={(optimization) => {
          onChange({ ...value, optimization });
        }}
        testId={`project-settings-${lower}-optimization`}
      />
      <SelectField
        label={`${name}: warnings`}
        value={value.warnings}
        options={WARNINGS}
        onChange={(warnings) => {
          onChange({ ...value, warnings });
        }}
        testId={`project-settings-${lower}-warnings`}
      />
      <CheckField
        label="Treat warnings as errors"
        checked={value.warningsAsErrors === true}
        onChange={warningsAsErrors}
        testId={`project-settings-${lower}-werror`}
      />
      <CheckField
        label="Debug information"
        checked={value.debugInfo}
        onChange={(debugInfo) => {
          onChange({ ...value, debugInfo });
        }}
        testId={`project-settings-${lower}-debug-info`}
      />
      <CheckField
        label="Check memory use (AddressSanitizer; not on Windows)"
        checked={value.sanitizers.includes('address')}
        onChange={(on) => {
          sanitizer('address', on);
        }}
        testId={`project-settings-${lower}-asan`}
      />
      <CheckField
        label="Check for undefined behaviour (UndefinedBehaviorSanitizer)"
        checked={value.sanitizers.includes('undefined')}
        onChange={(on) => {
          sanitizer('undefined', on);
        }}
        testId={`project-settings-${lower}-ubsan`}
      />
      <CheckField
        label="Hardening (protections against exploits)"
        checked={value.hardening}
        onChange={(hardening) => {
          onChange({ ...value, hardening });
        }}
        testId={`project-settings-${lower}-hardening`}
      />
    </fieldset>
  );
}

/** A field's problem, tied to the field by `id`. */
function Problem({ id, message }: { id: string; message: string }) {
  return (
    <p id={id} className="feature-field-error" role="alert">
      {message}
    </p>
  );
}

/** A text field of a list row, with its problem. */
function RowInput({
  id,
  label,
  value,
  maxLength,
  problem,
  onChange,
  testId,
}: {
  id: string;
  label: string;
  value: string;
  maxLength: number;
  problem: string | null;
  onChange: (value: string) => void;
  testId: string;
}): ReactNode {
  const problemId = `${id}-problem`;
  return (
    <span className="project-settings-cell">
      <input
        id={id}
        className="feature-input"
        type="text"
        autoComplete="off"
        spellCheck={false}
        aria-label={label}
        maxLength={maxLength}
        value={value}
        aria-invalid={problem !== null}
        aria-describedby={problem === null ? undefined : problemId}
        data-testid={testId}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
      {problem !== null && <Problem id={problemId} message={problem} />}
    </span>
  );
}

/** The defines: name, type and value rows. */
function DefinesField({
  rows,
  problemOf,
  showAll,
  onChange,
}: {
  rows: readonly DefineRow[];
  problemOf: (field: string) => string | null;
  showAll: boolean;
  onChange: (rows: DefineRow[]) => void;
}) {
  const [touched, setTouched] = useState<ReadonlySet<number>>(new Set());
  const replace = (key: number, patch: Partial<DefineRow>) => {
    setTouched((current) => new Set(current).add(key));
    onChange(rows.map((row) => (row.key === key ? { ...row, ...patch } : row)));
  };
  return (
    <fieldset className="feature-field" data-testid="project-settings-defines">
      <legend>Defines</legend>
      <p className="feature-muted">
        Names the program’s C++ can use, each with a value (-DNAME=value). Raw C++ blocks see them.
      </p>
      {rows.length === 0 && <p className="feature-muted">No defines.</p>}
      {rows.map((row, index) => {
        const shown = showAll || touched.has(row.key);
        const typeId = `define-type-${String(row.key)}`;
        return (
          <div key={row.key} className="project-settings-row" data-testid="project-settings-define">
            <RowInput
              id={`define-name-${String(row.key)}`}
              label={`Define ${String(index + 1)}: name`}
              value={row.name}
              maxLength={MAX_DEFINE_NAME}
              problem={shown ? problemOf(`define-name-${String(row.key)}`) : null}
              onChange={(name) => {
                replace(row.key, { name });
              }}
              testId="project-settings-define-name"
            />
            <select
              id={typeId}
              className="feature-input"
              aria-label={`Define ${String(index + 1)}: type`}
              value={row.type}
              onChange={(event) => {
                const type = DEFINE_TYPES.find((option) => option.value === event.target.value);
                if (type !== undefined) {
                  replace(row.key, {
                    type: type.value,
                    value: type.value === 'bool' ? 'true' : type.value === 'int' ? '0' : '',
                  });
                }
              }}
            >
              {DEFINE_TYPES.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
            {row.type === 'bool' ? (
              <select
                id={`define-value-${String(row.key)}`}
                className="feature-input"
                aria-label={`Define ${String(index + 1)}: value`}
                value={row.value}
                onChange={(event) => {
                  replace(row.key, { value: event.target.value });
                }}
              >
                <option value="true">true</option>
                <option value="false">false</option>
              </select>
            ) : (
              <RowInput
                id={`define-value-${String(row.key)}`}
                label={`Define ${String(index + 1)}: value`}
                value={row.value}
                maxLength={row.type === 'int' ? 20 : MAX_DEFINE_TEXT}
                problem={shown ? problemOf(`define-value-${String(row.key)}`) : null}
                onChange={(value) => {
                  replace(row.key, { value });
                }}
                testId="project-settings-define-value"
              />
            )}
            <button
              type="button"
              className="button"
              aria-label={`Remove define ${String(index + 1)}`}
              onClick={() => {
                onChange(rows.filter((other) => other.key !== row.key));
              }}
            >
              Remove
            </button>
          </div>
        );
      })}
      <div className="feature-actions">
        <button
          type="button"
          className="button"
          disabled={rows.length >= MAX_DEFINES}
          data-testid="project-settings-add-define"
          onClick={() => {
            onChange([...rows, { key: rowKey(), name: '', type: 'int', value: '0' }]);
          }}
        >
          Add a define
        </button>
      </div>
    </fieldset>
  );
}

/** The program's arguments, one per row, in order. */
function ArgsField({
  rows,
  problemOf,
  showAll,
  onChange,
}: {
  rows: readonly ArgRow[];
  problemOf: (field: string) => string | null;
  showAll: boolean;
  onChange: (rows: ArgRow[]) => void;
}) {
  const move = (index: number, by: -1 | 1) => {
    const next = [...rows];
    const [row] = next.splice(index, 1);
    if (row !== undefined) {
      next.splice(index + by, 0, row);
      onChange(next);
    }
  };
  return (
    <fieldset className="feature-field" data-testid="project-settings-args">
      <legend>Arguments</legend>
      <p className="feature-muted">
        What the program gets in argv, one argument per row: spaces and quotes are part of an
        argument, never a shell’s.
      </p>
      {rows.length === 0 && <p className="feature-muted">No arguments.</p>}
      {rows.map((row, index) => (
        <div key={row.key} className="project-settings-row" data-testid="project-settings-arg">
          <RowInput
            id={`arg-${String(row.key)}`}
            label={`Argument ${String(index + 1)}`}
            value={row.text}
            maxLength={MAX_ARG_LENGTH}
            problem={showAll ? problemOf(`arg-${String(row.key)}`) : null}
            onChange={(text) => {
              onChange(rows.map((other) => (other.key === row.key ? { ...other, text } : other)));
            }}
            testId="project-settings-arg-text"
          />
          <button
            type="button"
            className="button"
            aria-label={`Move argument ${String(index + 1)} up`}
            disabled={index === 0}
            onClick={() => {
              move(index, -1);
            }}
          >
            Up
          </button>
          <button
            type="button"
            className="button"
            aria-label={`Move argument ${String(index + 1)} down`}
            disabled={index === rows.length - 1}
            onClick={() => {
              move(index, 1);
            }}
          >
            Down
          </button>
          <button
            type="button"
            className="button"
            aria-label={`Remove argument ${String(index + 1)}`}
            onClick={() => {
              onChange(rows.filter((other) => other.key !== row.key));
            }}
          >
            Remove
          </button>
        </div>
      ))}
      <div className="feature-actions">
        <button
          type="button"
          className="button"
          disabled={rows.length >= MAX_ARGS}
          data-testid="project-settings-add-arg"
          onClick={() => {
            onChange([...rows, { key: rowKey(), text: '' }]);
          }}
        >
          Add an argument
        </button>
      </div>
    </fieldset>
  );
}

/**
 * The run options kept on this computer, which apply at once: the file the program reads as its
 * input (`run_input_file`; chosen in a native dialog, only its name is shown) and the separate
 * terminal window (`settings.json` `run.externalTerminal`).
 */
function MachineRunOptions({ ctx }: { ctx: FeatureContext }) {
  const handle = useAppStore((state) => state.project?.handle ?? null);
  const external = useAppStore((state) => state.settings.value?.run.externalTerminal ?? false);
  const [fileName, setFileName] = useState<string | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const ask = async (action: RunInputFileAction) => {
    if (handle === null) {
      return;
    }
    setBusy(true);
    try {
      const response = await ctx.ipc.runInputFile({ handle, action });
      setFileName(response.fileName);
      setProblem(null);
    } catch (error: unknown) {
      const code = failureCode(error);
      console.warn('run_input_file failed', code);
      setProblem(
        code === 'io'
          ? 'That is not a file a program can read: choose a regular file.'
          : code === 'busy'
            ? 'Another dialog is open. Close it, then try again.'
            : 'The input file could not be changed. The details are in the log file.',
      );
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    void ask('get');
    // Once per project.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [handle]);

  const setExternal = async (on: boolean) => {
    try {
      const response = await ctx.ipc.settingsUpdate({ run: { externalTerminal: on } });
      ctx.store.getState().actions.setSettings({ value: response.settings });
    } catch (error: unknown) {
      console.warn('settings_update failed', failureCode(error));
      setProblem('The setting could not be saved. The details are in the log file.');
    }
  };

  return (
    <fieldset className="feature-field" data-testid="project-settings-machine">
      <legend>On this computer</legend>
      <p className="feature-muted">
        These apply at once and are not saved in the project, which never names a file of yours.
      </p>
      <p data-testid="project-settings-input-file">
        Input: {fileName === null ? 'typed in the console' : `read from the file “${fileName}”`}
      </p>
      <div className="feature-actions">
        <button
          type="button"
          className="button"
          disabled={busy}
          data-testid="project-settings-choose-input"
          onClick={() => {
            void ask('choose');
          }}
        >
          Choose an input file…
        </button>
        <button
          type="button"
          className="button"
          disabled={busy || fileName === null}
          data-testid="project-settings-clear-input"
          onClick={() => {
            void ask('clear');
          }}
        >
          Type input in the console
        </button>
      </div>
      <CheckField
        label="Run programs in a separate terminal window"
        checked={external}
        onChange={(on) => {
          void setExternal(on);
        }}
        description="For every project. A program that reads an input file still runs in the console."
        testId="project-settings-external"
      />
      {problem !== null && <Notice tone="error">{problem}</Notice>}
    </fieldset>
  );
}
