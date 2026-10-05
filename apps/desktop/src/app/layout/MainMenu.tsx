/**
 * The toolbar's main menu, `≡` (docs/spec/04-user-interface.md §4.1, §4.10): *New project…*,
 * *Open…*, *Save*, *Save as…* and *Close project*. Like the rest of the toolbar it only runs
 * commands: an item is listed only while a feature has registered its command, and the items
 * about the open project only while one is open, so the menu never offers something that does
 * nothing (§4.13). Without any item the button is not shown.
 *
 * Keyboard (the WAI-ARIA menu button pattern): `Enter`, `Space` or `↓` opens the menu on its
 * first item and `↑` on its last; `↑`/`↓` move with wrap-around, `Home`/`End` jump, `Escape`
 * closes and returns to the button, and `Tab` closes and moves on.
 */
import {
  type KeyboardEvent as ReactKeyboardEvent,
  useEffect,
  useId,
  useRef,
  useState,
  useSyncExternalStore,
} from 'react';

import { type CommandId, type CommandRegistry, commands, triggerCommand } from '../commands';
import { SHORTCUTS } from '../shortcuts';
import { useAppStore } from '../store';
import './MainMenu.css';

/** One item of the menu. */
interface MenuItem {
  readonly command: CommandId;
  readonly label: string;
  /** The shortcut shown next to the label. */
  readonly shortcut?: { readonly label: string; readonly aria: string };
  /** Only while a project is open. */
  readonly needsProject: boolean;
  /** Items of different groups are separated by a line. */
  readonly group: number;
}

/** Every item, in menu order. */
export const MAIN_MENU_ITEMS: readonly MenuItem[] = [
  { command: 'project.new', label: 'New project…', needsProject: false, group: 0 },
  { command: 'project.open', label: 'Open…', needsProject: false, group: 0 },
  {
    command: 'project.save',
    label: 'Save',
    shortcut: SHORTCUTS.save,
    needsProject: true,
    group: 1,
  },
  { command: 'project.saveAs', label: 'Save as…', needsProject: true, group: 1 },
  { command: 'project.close', label: 'Close project', needsProject: true, group: 2 },
];

/** The registered commands among the menu's, as one string (a stable external-store snapshot). */
function registeredKey(registry: CommandRegistry): string {
  return MAIN_MENU_ITEMS.filter((item) => registry.hasCommand(item.command))
    .map((item) => item.command)
    .join(' ');
}

/** The main menu; `registry` is the app's command registry unless a test passes its own. */
export function MainMenu({ registry = commands }: { registry?: CommandRegistry }) {
  const registered = useSyncExternalStore(registry.subscribe, () => registeredKey(registry));
  const hasProject = useAppStore((state) => state.project !== null);
  const [open, setOpen] = useState(false);
  /** The item to focus once the menu has rendered: `first`, `last`, or none. */
  const [focusOnOpen, setFocusOnOpen] = useState<'first' | 'last'>('first');
  const button = useRef<HTMLButtonElement>(null);
  const container = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const buttonId = useId();
  const menuId = useId();

  const commandsShown = new Set(registered.split(' '));
  const items = MAIN_MENU_ITEMS.filter(
    (item) => commandsShown.has(item.command) && (hasProject || !item.needsProject),
  );

  // Move the focus into the menu when it opens.
  useEffect(() => {
    if (!open) {
      return;
    }
    const shown = itemRefs.current.filter((item) => item !== null);
    (focusOnOpen === 'last' ? shown.at(-1) : shown[0])?.focus();
  }, [open, focusOnOpen]);

  // A press anywhere outside closes the menu.
  useEffect(() => {
    if (!open) {
      return undefined;
    }
    const onPointerDown = (event: PointerEvent) => {
      if (!(event.target instanceof Node) || !container.current?.contains(event.target)) {
        setOpen(false);
      }
    };
    document.addEventListener('pointerdown', onPointerDown, true);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown, true);
    };
  }, [open]);

  // Nothing to offer, or the last item went away while open.
  useEffect(() => {
    if (items.length === 0 && open) {
      setOpen(false);
    }
  }, [items.length, open]);

  if (items.length === 0) {
    return null;
  }

  const openMenu = (focus: 'first' | 'last') => {
    setFocusOnOpen(focus);
    setOpen(true);
  };

  const close = (returnFocus: boolean) => {
    setOpen(false);
    if (returnFocus) {
      button.current?.focus();
    }
  };

  const onButtonKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>) => {
    if (event.key === 'ArrowDown') {
      event.preventDefault();
      openMenu('first');
    } else if (event.key === 'ArrowUp') {
      event.preventDefault();
      openMenu('last');
    }
  };

  const onMenuKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const shown = itemRefs.current.filter((item) => item !== null);
    const index = shown.findIndex((item) => item === document.activeElement);
    const focusAt = (next: number) => {
      shown[(next + shown.length) % shown.length]?.focus();
    };
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        focusAt(index + 1);
        break;
      case 'ArrowUp':
        event.preventDefault();
        focusAt(index < 0 ? -1 : index - 1);
        break;
      case 'Home':
        event.preventDefault();
        focusAt(0);
        break;
      case 'End':
        event.preventDefault();
        focusAt(-1);
        break;
      case 'Escape':
        event.preventDefault();
        event.stopPropagation();
        close(true);
        break;
      case 'Tab':
        close(false);
        break;
      default:
        break;
    }
  };

  itemRefs.current = [];
  return (
    <div className="main-menu" ref={container}>
      <button
        ref={button}
        id={buttonId}
        type="button"
        className="toolbar-button main-menu-button"
        aria-label="Main menu"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        data-testid="main-menu"
        onClick={() => {
          if (open) {
            close(false);
          } else {
            openMenu('first');
          }
        }}
        onKeyDown={onButtonKeyDown}
      >
        <span aria-hidden="true">≡</span>
      </button>
      {open && (
        <div
          id={menuId}
          className="main-menu-list"
          role="menu"
          aria-labelledby={buttonId}
          tabIndex={-1}
          onKeyDown={onMenuKeyDown}
        >
          {items.map((item, index) => (
            <MenuEntry
              key={item.command}
              item={item}
              separated={index > 0 && items[index - 1]?.group !== item.group}
              itemRef={(element) => {
                itemRefs.current[index] = element;
              }}
              onChoose={() => {
                close(true);
                triggerCommand(item.command, registry);
              }}
            />
          ))}
        </div>
      )}
    </div>
  );
}

/** One item, with a separator line above it when it starts a new group. */
function MenuEntry({
  item,
  separated,
  itemRef,
  onChoose,
}: {
  item: MenuItem;
  separated: boolean;
  itemRef: (element: HTMLButtonElement | null) => void;
  onChoose: () => void;
}) {
  return (
    <>
      {separated && <div role="separator" className="main-menu-separator" />}
      <button
        ref={itemRef}
        type="button"
        role="menuitem"
        tabIndex={-1}
        className="main-menu-item"
        aria-keyshortcuts={item.shortcut?.aria}
        onClick={onChoose}
      >
        <span>{item.label}</span>
        {item.shortcut !== undefined && (
          <span className="main-menu-shortcut" aria-hidden="true">
            {item.shortcut.label}
          </span>
        )}
      </button>
    </>
  );
}
