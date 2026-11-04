/**
 * A pop-up menu of the module strip (*Module actions*, a module's own actions, *All modules…*),
 * following the app's menu pattern (app/layout/MainMenu.tsx, WAI-ARIA's menu): it opens with the
 * focus on its first item (or the checked one), `↑`/`↓` move with wrap-around, `Home`/`End` jump,
 * `Escape` closes and returns the focus to what opened it, `Tab` closes and moves on, and a press
 * outside closes it. With `typeAhead`, typing moves to the next item whose label starts with what
 * was typed. A disabled item keeps the focus in the arrow keys' path and gives its reason as its
 * accessible description and tooltip; choosing it does nothing.
 */
import {
  type KeyboardEvent as ReactKeyboardEvent,
  type RefObject,
  useEffect,
  useId,
  useRef,
} from 'react';

/** One item of a strip menu. */
export interface StripMenuItem {
  /** A key unique in the menu. */
  readonly key: string;
  readonly label: string;
  readonly onChoose: () => void;
  /** Why it cannot be chosen now, or `null`/absent when it can. */
  readonly disabledReason?: string | null;
  /** A tooltip for an item that can be chosen. */
  readonly hint?: string;
  /** For a menu of choices (*All modules…*): whether it is the current one. */
  readonly checked?: boolean;
}

/** How long typed letters count as one search, in milliseconds. */
const TYPE_AHEAD_MS = 700;

/** What a {@link StripMenu} needs. */
export interface StripMenuProps {
  /** The menu's accessible name. */
  readonly label: string;
  readonly items: readonly StripMenuItem[];
  /** The element the menu is shown below, which gets the focus back on `Escape`. */
  readonly anchor: RefObject<HTMLElement | null>;
  /** Closes the menu; `returnFocus` gives the focus back to the anchor. */
  readonly onClose: (returnFocus: boolean) => void;
  readonly typeAhead?: boolean;
  /** The menu's HTML ID, for the anchor's `aria-controls`. */
  readonly id?: string;
  /** Where its left edge is, in pixels from the strip's. */
  readonly left?: number;
}

/** A strip menu; see the module comment. */
export function StripMenu({
  label,
  items,
  anchor,
  onClose,
  typeAhead,
  id,
  left = 0,
}: StripMenuProps) {
  const menu = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const typed = useRef({ text: '', at: 0 });
  const reasonPrefix = useId();
  const checkable = items.some((item) => item.checked !== undefined);

  // Focus the checked item, or the first, when the menu opens.
  const firstFocus = useRef(
    Math.max(
      items.findIndex((item) => item.checked === true),
      0,
    ),
  );
  useEffect(() => {
    itemRefs.current[firstFocus.current]?.focus();
  }, []);

  // A press outside the menu and its anchor closes it.
  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (
        !(target instanceof Node) ||
        (!menu.current?.contains(target) && !anchor.current?.contains(target))
      ) {
        onClose(false);
      }
    };
    document.addEventListener('pointerdown', onPointerDown, true);
    return () => {
      document.removeEventListener('pointerdown', onPointerDown, true);
    };
  }, [anchor, onClose]);

  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const shown = itemRefs.current.filter((item) => item !== null);
    const index = shown.findIndex((item) => item === document.activeElement);
    const focusAt = (next: number) => {
      shown[(next + shown.length) % shown.length]?.focus();
    };
    switch (event.key) {
      case 'ArrowDown':
        event.preventDefault();
        focusAt(index + 1);
        return;
      case 'ArrowUp':
        event.preventDefault();
        focusAt(index < 0 ? -1 : index - 1);
        return;
      case 'Home':
        event.preventDefault();
        focusAt(0);
        return;
      case 'End':
        event.preventDefault();
        focusAt(-1);
        return;
      case 'Escape':
        event.preventDefault();
        event.stopPropagation();
        onClose(true);
        return;
      case 'Tab':
        onClose(false);
        return;
      default:
        break;
    }
    if (
      typeAhead === true &&
      event.key.length === 1 &&
      !event.ctrlKey &&
      !event.metaKey &&
      !event.altKey
    ) {
      event.preventDefault();
      const now = Date.now();
      const text =
        now - typed.current.at > TYPE_AHEAD_MS
          ? event.key.toLowerCase()
          : typed.current.text + event.key.toLowerCase();
      typed.current = { text, at: now };
      // The next item that starts with the text, from the focused one on; one letter typed again
      // goes on to the next item with that letter.
      const repeated = /^(.)\1*$/su.test(text);
      const search = repeated ? text.charAt(0) : text;
      const start = repeated ? index + 1 : Math.max(index, 0);
      for (let step = 0; step < items.length; step += 1) {
        const at = (start + step) % items.length;
        if (items[at]?.label.toLowerCase().startsWith(search) === true) {
          focusAt(at);
          break;
        }
      }
    }
  };

  itemRefs.current = [];
  return (
    <div
      ref={menu}
      id={id}
      className="module-strip-menu"
      style={{ left }}
      role="menu"
      aria-label={label}
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      {items.map((item, index) => {
        const reason = item.disabledReason ?? null;
        const reasonId = `${reasonPrefix}-${String(index)}`;
        return (
          <button
            key={item.key}
            ref={(element) => {
              itemRefs.current[index] = element;
            }}
            type="button"
            role={checkable ? 'menuitemradio' : 'menuitem'}
            tabIndex={-1}
            className="module-strip-menu-item"
            aria-checked={checkable ? item.checked === true : undefined}
            aria-disabled={reason === null ? undefined : true}
            aria-describedby={reason === null ? undefined : reasonId}
            title={reason ?? item.hint}
            onClick={() => {
              if (reason !== null) {
                return;
              }
              onClose(false);
              item.onChoose();
            }}
          >
            <span>{item.label}</span>
          </button>
        );
      })}
      {/* The disabled items' reasons, read as their descriptions (not part of their names). */}
      <div hidden>
        {items.map((item, index) =>
          item.disabledReason === undefined || item.disabledReason === null ? null : (
            <span key={item.key} id={`${reasonPrefix}-${String(index)}`}>
              {item.disabledReason}
            </span>
          ),
        )}
      </div>
    </div>
  );
}
