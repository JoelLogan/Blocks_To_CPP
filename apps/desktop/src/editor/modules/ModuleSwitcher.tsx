/**
 * The module switcher (M2 decision "Multi-module files, frames and notes in M2"): one canvas per
 * module, shown one at a time. It only switches; adding, renaming and deleting modules come in M3.
 * It is shown only when the project has more than one module.
 */
import { useAppStore } from '../../app/store';
import './ModuleSwitcher.css';

const NO_MODULES: readonly never[] = [];

/** A strip of buttons, one per module; the shown module's is pressed. */
export function ModuleSwitcher() {
  const modules = useAppStore((state) => state.project?.document.modules ?? NO_MODULES);
  const active = useAppStore((state) => state.project?.activeModuleId ?? null);

  if (modules.length < 2 || active === null) {
    return null;
  }

  return (
    <nav className="module-switcher" aria-label="Modules">
      {modules.map((module) => (
        <button
          key={module.id}
          type="button"
          className="module-switcher-button"
          aria-pressed={module.id === active}
          data-testid="module-switch"
          onClick={() => {
            if (module.id !== active) {
              // The editing session follows `activeModuleId` and shows that module's canvas.
              useAppStore.getState().actions.updateProject({ activeModuleId: module.id });
            }
          }}
        >
          {`${module.name}.cpp`}
        </button>
      ))}
    </nav>
  );
}
