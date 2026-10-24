/**
 * The module switcher (M2 decision "Multi-module files, frames and notes in M2"): one canvas per
 * module, shown one at a time. It only switches; adding, renaming and deleting modules come in M3.
 * It is shown only when the project has more than one module.
 */
import { useShallow } from 'zustand/react/shallow';

import { useAppStore } from '../../app/store';
import './ModuleSwitcher.css';

const NO_MODULES: readonly never[] = [];

/**
 * A strip of buttons, one per module; the shown module's is pressed. It selects the modules' IDs
 * and names (every edit gives the document new module objects), so an edit that keeps them does
 * not render it again.
 */
export function ModuleSwitcher() {
  const ids = useAppStore(
    useShallow((state) => (state.project?.document.modules ?? NO_MODULES).map(({ id }) => id)),
  );
  const names = useAppStore(
    useShallow((state) => (state.project?.document.modules ?? NO_MODULES).map(({ name }) => name)),
  );
  const active = useAppStore((state) => state.project?.activeModuleId ?? null);

  if (ids.length < 2 || active === null) {
    return null;
  }

  return (
    <nav className="module-switcher" aria-label="Modules">
      {ids.map((id, index) => (
        <button
          key={id}
          type="button"
          className="module-switcher-button"
          aria-pressed={id === active}
          data-testid="module-switch"
          onClick={() => {
            if (id !== active) {
              // The editing session follows `activeModuleId` and shows that module's canvas.
              useAppStore.getState().actions.updateProject({ activeModuleId: id });
            }
          }}
        >
          {`${names[index] ?? ''}.cpp`}
        </button>
      ))}
    </nav>
  );
}
