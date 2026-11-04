/**
 * The module strip above the canvas (docs/plans/m3-contracts/w3-modules.md §1.2, §2.11; ADR-0019
 * §9): one canvas per module, shown one at a time, and the module edits. It is shown whenever a
 * project is open, from one module on, so that *Add module* can be reached (§6 W3M-D7).
 *
 * - The module buttons are a toolbar labelled *Modules* with a roving tab index: one Tab stop (the
 *   shown module's button); `←`/`→`, `Home` and `End` move between them, and `Enter` or `Space`
 *   shows a module. Each is a toggle button (`aria-pressed`) labelled with its file,
 *   `<name>.cpp`, or `module<N>.cpp` with an error mark when the module's name cannot be a file
 *   name (`B2C-E0221`), its name then in the accessible name and tooltip.
 * - The context-menu key or `Shift+F10` on a module button (or a right click) opens that module's
 *   actions: *Rename…*, *Delete…*, *Move left*, *Move right*. The same actions are on one *Module
 *   actions* menu button, which acts on the shown module; ⊕ *Add module* is its own Tab stop.
 *   Disabled items (⊕ at 256 modules, *Delete…* on the only module) keep the focus and give their
 *   reason as their accessible description and tooltip.
 * - When the buttons do not fit, they scroll inside the strip (never the page), the shown one kept
 *   in view, and *All modules…* opens a list with type-ahead that shows the module chosen.
 * - The last module edit's message is in the strip's status region, with *Undo delete* after a
 *   deletion (features/project/modules/).
 *
 * It selects only what it shows (the modules' IDs, names and file labels), so an edit of the
 * canvas does not render it again.
 */
import {
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react';
import { useStore } from 'zustand';
import { useShallow } from 'zustand/react/shallow';

import { type ProjectState, useAppStore } from '../../app/store';
import {
  getModuleEdits,
  MAX_MODULES,
  MODULE_EDIT_PROBLEMS,
  type ModuleEdits,
  type ModuleEditsState,
  subscribeModuleEdits,
} from '../../features/project/modules';
import { visibleInvisibles } from '../../panels/shared/invisibles';
import { StripMenu, type StripMenuItem } from './StripMenu';
import './ModuleSwitcher.css';

const NO_MODULES: readonly never[] = [];

/** What a module's button shows. */
export interface ModuleFileLabel {
  /** The file name shown: `<name>.cpp`, or `module<N>.cpp` for a name that cannot be one. */
  readonly file: string;
  /** Whether the module's name cannot be its file name (`B2C-E0221`, the loader's fallback). */
  readonly refused: boolean;
}

/**
 * The file each module's button shows: its `.cpp` as the latest preview names it (the layout's
 * `FileStem`, `module<N>` for a name that cannot be a file name), or before a preview, its name,
 * with `module<N>` for a name that starts with `b2c` (`B2C-E0221`).
 */
export function moduleFileLabels(
  project: ProjectState | null,
  files: readonly { path: string; module?: string }[] | null,
): ModuleFileLabel[] {
  const modules = project?.document.modules ?? NO_MODULES;
  return modules.map((module, index) => {
    const generated = files?.find(
      (file) => file.module === module.id && file.path.endsWith('.cpp'),
    )?.path;
    const fallback = `module${String(index + 1)}.cpp`;
    const file =
      generated ??
      (module.name.slice(0, 3).toLowerCase() === 'b2c' ? fallback : `${module.name}.cpp`);
    return { file, refused: file !== `${module.name}.cpp` };
  });
}

/** The module edits, while the feature is installed (`null` otherwise: the strip only switches). */
function useModuleEdits(): ModuleEdits | null {
  return useSyncExternalStore(subscribeModuleEdits, getModuleEdits);
}

const NO_EDITS_STATE: ModuleEditsState = { kept: null, status: null };
const NO_EDITS_STORE = {
  getState: () => NO_EDITS_STATE,
  getInitialState: () => NO_EDITS_STATE,
  setState: () => undefined,
  subscribe: () => () => undefined,
};

/** Shows module `id` (the editing session follows `activeModuleId`). */
function showModule(id: string): void {
  const project = useAppStore.getState().project;
  if (project !== null && project.activeModuleId !== id) {
    useAppStore.getState().actions.updateProject({ activeModuleId: id });
  }
}

/** The widest a strip menu is drawn, in pixels (ModuleSwitcher.css). */
const MENU_WIDTH = 240;

/**
 * Which menu is open (a module's own, *Module actions*, or *All modules…*) and where its left
 * edge is in the strip, below what opened it.
 */
type OpenMenu =
  | ({ readonly left: number } & (
      | { readonly kind: 'module'; readonly id: string }
      | { readonly kind: 'actions' }
      | { readonly kind: 'all' }
    ))
  | null;

/** The module strip; see the module comment. */
export function ModuleSwitcher() {
  const ids = useAppStore(
    useShallow((state) => (state.project?.document.modules ?? NO_MODULES).map(({ id }) => id)),
  );
  const names = useAppStore(
    useShallow((state) => (state.project?.document.modules ?? NO_MODULES).map(({ name }) => name)),
  );
  const labels = useAppStore(
    useShallow((state) =>
      moduleFileLabels(state.project, state.analysis.preview?.files ?? null).map(
        ({ file, refused }) => `${refused ? '!' : ' '}${file}`,
      ),
    ),
  );
  const active = useAppStore((state) => state.project?.activeModuleId ?? null);
  const edits = useModuleEdits();
  const { kept, status } = useStore(edits?.state ?? NO_EDITS_STORE);

  /** The module button with the roving Tab stop, and the module shown when it got it. */
  const [focused, setFocusedState] = useState<{ id: string; shown: string | null } | null>(null);
  const [menu, setMenu] = useState<OpenMenu>(null);
  const [overflowing, setOverflowing] = useState(false);
  const overflowSeen = useRef(false);
  const strip = useRef<HTMLDivElement>(null);
  const tabs = useRef<HTMLDivElement>(null);
  const buttons = useRef(new Map<string, HTMLButtonElement>());
  const menuAnchor = useRef<HTMLElement | null>(null);
  const actionsButton = useRef<HTMLButtonElement>(null);
  const allButton = useRef<HTMLButtonElement>(null);
  const addReasonId = useId();
  const menuId = useId();

  // The roving Tab stop is the shown module's button, until the arrow keys move it; showing
  // another module gives it back to the shown one.
  const tabStop =
    focused !== null && focused.shown === active && ids.includes(focused.id) ? focused.id : active;
  const setFocused = (id: string) => {
    if (focused?.id !== id || focused.shown !== active) {
      setFocusedState({ id, shown: active });
    }
  };

  // Keep the shown module's button in view inside the strip.
  useEffect(() => {
    const button = active === null ? undefined : buttons.current.get(active);
    if (typeof button?.scrollIntoView === 'function') {
      button.scrollIntoView({ block: 'nearest', inline: 'nearest' });
    }
  }, [active, ids.length]);

  // Whether the buttons overflow the strip (then *All modules…* is offered).
  useLayoutEffect(() => {
    const element = tabs.current;
    if (element === null) {
      return undefined;
    }
    const measure = () => {
      // Set only on a change, so a rename that keeps it does not render the strip again.
      const now = element.scrollWidth > element.clientWidth + 1;
      if (now !== overflowSeen.current) {
        overflowSeen.current = now;
        setOverflowing(now);
      }
    };
    measure();
    if (typeof ResizeObserver === 'undefined') {
      return undefined;
    }
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, [ids.length, labels]);

  const closeMenu = useCallback((returnFocus: boolean) => {
    setMenu(null);
    if (returnFocus) {
      menuAnchor.current?.focus();
    }
  }, []);

  if (active === null) {
    return null;
  }

  const full = ids.length >= MAX_MODULES;

  /** Opens a menu below `anchor`, kept inside the strip. */
  const openMenu = (
    which: { kind: 'module'; id: string } | { kind: 'actions' } | { kind: 'all' },
    anchor: HTMLElement | null,
  ) => {
    menuAnchor.current = anchor;
    const box = strip.current?.getBoundingClientRect();
    const at = anchor?.getBoundingClientRect();
    const left =
      box === undefined || at === undefined
        ? 0
        : Math.max(0, Math.min(at.left - box.left, box.width - MENU_WIDTH));
    setMenu({ ...which, left });
  };

  const focusButton = (id: string | undefined) => {
    if (id === undefined) {
      return;
    }
    setFocused(id);
    buttons.current.get(id)?.focus();
  };

  const openModuleMenu = (id: string, anchor: HTMLElement) => {
    openMenu({ kind: 'module', id }, anchor);
  };

  const onTabsKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const current = ids.indexOf(tabStop ?? '');
    switch (event.key) {
      case 'ArrowRight':
        event.preventDefault();
        focusButton(ids[(current + 1) % ids.length]);
        return;
      case 'ArrowLeft':
        event.preventDefault();
        focusButton(ids[(current - 1 + ids.length) % ids.length]);
        return;
      case 'Home':
        event.preventDefault();
        focusButton(ids[0]);
        return;
      case 'End':
        event.preventDefault();
        focusButton(ids[ids.length - 1]);
        return;
      default:
        break;
    }
    if (
      edits !== null &&
      (event.key === 'ContextMenu' || (event.key === 'F10' && event.shiftKey)) &&
      event.target instanceof HTMLElement
    ) {
      const id = event.target.dataset['moduleId'];
      if (id !== undefined) {
        event.preventDefault();
        openModuleMenu(id, event.target);
      }
    }
  };

  /** The actions on module `id` (its own menu, or *Module actions* for the shown one). */
  const actionsFor = (id: string): StripMenuItem[] => {
    if (edits === null) {
      return [];
    }
    const at = ids.indexOf(id);
    const programName = 'The first module names the program.';
    return [
      {
        key: 'rename',
        label: 'Rename…',
        onChoose: () => void edits.rename(id),
      },
      {
        key: 'delete',
        label: 'Delete…',
        onChoose: () => void edits.remove(id),
        disabledReason: ids.length <= 1 ? MODULE_EDIT_PROBLEMS.onlyModule : null,
      },
      {
        key: 'left',
        label: 'Move left',
        onChoose: () => {
          edits.move(id, -1);
        },
        disabledReason: at <= 0 ? 'This module is already the first.' : null,
        hint: programName,
      },
      {
        key: 'right',
        label: 'Move right',
        onChoose: () => {
          edits.move(id, 1);
        },
        disabledReason: at >= ids.length - 1 ? 'This module is already the last.' : null,
        hint: programName,
      },
    ];
  };

  const allModules: StripMenuItem[] = ids.map((id, at) => ({
    key: id,
    label: names[at] ?? id,
    checked: id === active,
    onChoose: () => {
      showModule(id);
      buttons.current.get(id)?.focus();
    },
  }));

  const menuItems =
    menu === null
      ? []
      : menu.kind === 'all'
        ? allModules
        : actionsFor(menu.kind === 'actions' ? active : menu.id);
  const menuLabel =
    menu?.kind === 'all'
      ? 'All modules'
      : `Actions of module ${visibleInvisibles(names[ids.indexOf(menu?.kind === 'module' ? menu.id : active)] ?? '')}`;

  return (
    <div ref={strip} className="module-switcher" data-testid="module-strip">
      <div
        ref={tabs}
        role="toolbar"
        aria-label="Modules"
        className="module-switcher-tabs"
        onKeyDown={onTabsKeyDown}
      >
        {ids.map((id, at) => {
          const label = labels[at] ?? ` ${names[at] ?? ''}.cpp`;
          const refused = label.startsWith('!');
          const file = label.slice(1);
          const name = visibleInvisibles(names[at] ?? '');
          const described = refused
            ? `${file}: the module “${name}”, whose name cannot be used for its file. Rename it.`
            : undefined;
          return (
            <button
              key={id}
              ref={(element) => {
                if (element === null) {
                  buttons.current.delete(id);
                } else {
                  buttons.current.set(id, element);
                }
              }}
              type="button"
              className="module-switcher-button"
              aria-pressed={id === active}
              aria-label={described}
              title={described ?? `Module “${name}”`}
              tabIndex={id === tabStop ? 0 : -1}
              data-testid="module-switch"
              data-module-id={id}
              onFocus={() => {
                setFocused(id);
              }}
              onClick={() => {
                showModule(id);
              }}
              onContextMenu={(event: ReactMouseEvent<HTMLButtonElement>) => {
                if (edits !== null) {
                  event.preventDefault();
                  openModuleMenu(id, event.currentTarget);
                }
              }}
            >
              {refused && (
                <span className="module-switcher-error" aria-hidden="true">
                  ⚠{' '}
                </span>
              )}
              {file}
            </button>
          );
        })}
      </div>
      {overflowing && (
        <button
          ref={allButton}
          type="button"
          className="module-switcher-tool"
          aria-haspopup="menu"
          aria-expanded={menu?.kind === 'all'}
          aria-controls={menu?.kind === 'all' ? menuId : undefined}
          onClick={() => {
            if (menu?.kind === 'all') {
              setMenu(null);
            } else {
              openMenu({ kind: 'all' }, allButton.current);
            }
          }}
        >
          All modules…
        </button>
      )}
      {edits !== null && (
        <>
          <button
            ref={actionsButton}
            type="button"
            className="module-switcher-tool"
            aria-label="Module actions"
            title="Module actions"
            aria-haspopup="menu"
            aria-expanded={menu?.kind === 'actions'}
            aria-controls={menu?.kind === 'actions' ? menuId : undefined}
            data-testid="module-actions"
            onClick={() => {
              if (menu?.kind === 'actions') {
                setMenu(null);
              } else {
                openMenu({ kind: 'actions' }, actionsButton.current);
              }
            }}
          >
            <span aria-hidden="true">⋯</span>
          </button>
          <button
            type="button"
            className="module-switcher-tool"
            aria-disabled={full || undefined}
            aria-describedby={full ? addReasonId : undefined}
            title={full ? MODULE_EDIT_PROBLEMS.tooMany : 'Add module'}
            data-testid="module-add"
            onClick={() => {
              if (!full) {
                void edits.add();
              }
            }}
          >
            <span aria-hidden="true">⊕</span>
            <span className="visually-hidden">Add module</span>
          </button>
          {full && (
            <span id={addReasonId} hidden>
              {MODULE_EDIT_PROBLEMS.tooMany}
            </span>
          )}
        </>
      )}
      <div className="module-switcher-status">
        <span role="status" aria-live="polite" title={status?.text}>
          {status === null ? '' : <span key={status.serial}>{status.text}</span>}
        </span>
        {kept !== null && edits !== null && (
          <button
            type="button"
            className="module-switcher-tool module-switcher-undo"
            data-testid="module-undo-delete"
            onClick={() => {
              edits.undoDelete();
            }}
          >
            Undo delete
          </button>
        )}
      </div>
      {menu !== null && menuItems.length > 0 && (
        <StripMenu
          id={menuId}
          label={menuLabel}
          items={menuItems}
          anchor={menuAnchor}
          onClose={closeMenu}
          typeAhead={menu.kind === 'all'}
          left={menu.left}
        />
      )}
    </div>
  );
}
