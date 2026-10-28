/**
 * The type picker's popover (docs/spec/03-block-language.md §3.5.1,
 * docs/plans/m3-contracts/w4-editor-tools.md §2.2): a search box over a list of types, in groups,
 * built step by step for a collection. It follows the ARIA combobox pattern: the box keeps the
 * focus, Up and Down move the active option (`aria-activedescendant`), Enter chooses it, a polite
 * status says how many types match.
 *
 * Keys: typing filters the list (and a typed type, C++ or Friendly, is offered first); Up, Down,
 * Home, End and Page keys move; Enter chooses (a collection opens the step for its first
 * argument); Right opens a collection; Left or Backspace in an empty box go back a step; Escape
 * closes without a change.
 *
 * Every text is set as text (`textContent`), never as HTML.
 */
import {
  PICKER_GROUP_TITLES,
  acceptsArgument,
  acceptsTerm,
  addArgument,
  argumentEntries,
  pathLabel,
  rootEntries,
  termLabel,
  type OpenCollection,
  type PickerContext,
  type PickerEntry,
  type PickerGroup,
} from './model';
import { parseTypeText } from './parse';
import { termKey, type TypeTerm } from '../../types/static-type';

/** The CSS class of the popover's root element. */
export const TYPE_PICKER_CLASS = 'b2c-type-picker';

/** What a picker is opened with. */
export interface TypePickerOptions {
  readonly context: PickerContext;
  /** The field's value now, marked in the list. */
  readonly current: string | null;
  /** The accessible name of the search box (the field's). */
  readonly label: string;
  /** A type was chosen: its key. */
  readonly onChoose: (key: string) => void;
  /** The picker should close without a change (Escape, Tab). */
  readonly onCancel: () => void;
  /** The document to build the popover in (the field's). */
  readonly document?: Document;
}

/** An open picker. */
export interface TypePicker {
  /** The popover's root element, to put in the drop-down. */
  readonly element: HTMLElement;
  /** Moves the focus into the search box. */
  focus(): void;
  /** Types `text` into the search box (tests, and a key that opened the picker). */
  search(text: string): void;
  /** The entries listed now, in order. */
  entries(): readonly PickerEntry[];
}

/** How many entries Page Up and Page Down move. */
const PAGE = 8;

let pickerCount = 0;

/** Whether `entry` matches the lower-case words of a search. */
function matches(entry: PickerEntry, words: readonly string[]): boolean {
  const label = entry.label.toLowerCase();
  const key = entry.kind === 'type' ? entry.key.toLowerCase() : entry.name.toLowerCase();
  return words.every((word) => label.includes(word) || key.includes(word));
}

/** Builds a type picker (see the module comment). */
export function createTypePicker(options: TypePickerOptions): TypePicker {
  const doc = options.document ?? document;
  const context = options.context;
  pickerCount += 1;
  const base = `b2c-type-picker-${String(pickerCount)}`;

  const root = doc.createElement('div');
  root.className = TYPE_PICKER_CLASS;
  const crumb = doc.createElement('div');
  crumb.className = `${TYPE_PICKER_CLASS}-path`;
  crumb.id = `${base}-path`;
  const input = doc.createElement('input');
  input.type = 'text';
  input.className = `${TYPE_PICKER_CLASS}-search`;
  input.setAttribute('role', 'combobox');
  input.setAttribute('aria-autocomplete', 'list');
  input.setAttribute('aria-expanded', 'true');
  input.setAttribute('aria-controls', `${base}-list`);
  input.setAttribute('aria-label', options.label);
  input.setAttribute('aria-describedby', `${base}-path`);
  input.setAttribute('autocomplete', 'off');
  input.setAttribute('spellcheck', 'false');
  input.placeholder = 'Search or type a type';
  const list = doc.createElement('div');
  list.id = `${base}-list`;
  list.className = `${TYPE_PICKER_CLASS}-list`;
  list.setAttribute('role', 'listbox');
  list.setAttribute('aria-label', 'Types');
  const status = doc.createElement('div');
  status.className = `${TYPE_PICKER_CLASS}-status`;
  status.setAttribute('role', 'status');
  status.setAttribute('aria-live', 'polite');
  root.append(crumb, input, list, status);

  let path: OpenCollection[] = [];
  let shown: PickerEntry[] = [];
  let active = 0;
  let optionElements: HTMLElement[] = [];

  const stepEntries = (): PickerEntry[] => {
    const open = path.at(-1);
    return open === undefined
      ? rootEntries(context)
      : argumentEntries(context, open.name, open.args.length, path.length);
  };

  /** The typed type, offered first when it fits the step. */
  const typedEntry = (text: string): Extract<PickerEntry, { kind: 'type' }> | null => {
    const term = parseTypeText(text, context.userTypes);
    if (term === null) {
      return null;
    }
    const open = path.at(-1);
    const accepted =
      open === undefined
        ? acceptsTerm(context, term)
        : acceptsArgument(context, open.name, open.args.length, term);
    if (!accepted) {
      return null;
    }
    return {
      kind: 'type',
      group: 'common',
      key: termKey(term),
      term,
      label: termLabel(term, context),
    };
  };

  const render = (): void => {
    const text = input.value.trim();
    const words = text
      .toLowerCase()
      .split(/\s+/u)
      .filter((word) => word !== '');
    const all = stepEntries();
    const typed = text === '' ? null : typedEntry(text);
    const filtered = words.length === 0 ? all : all.filter((entry) => matches(entry, words));
    shown =
      typed === null
        ? filtered
        : [typed, ...filtered.filter((entry) => entry.kind !== 'type' || entry.key !== typed.key)];
    active = Math.min(active, Math.max(0, shown.length - 1));
    crumb.textContent = path.length === 0 ? '' : `Building ${pathLabel(path, context)}`;
    crumb.hidden = path.length === 0;
    list.replaceChildren();
    optionElements = [];
    let group: HTMLElement | null = null;
    let groupName: PickerGroup | 'typed' | null = null;
    shown.forEach((entry, index) => {
      const name: PickerGroup | 'typed' = typed !== null && index === 0 ? 'typed' : entry.group;
      if (group === null || name !== groupName) {
        groupName = name;
        group = doc.createElement('div');
        group.setAttribute('role', 'group');
        const heading = doc.createElement('div');
        heading.className = `${TYPE_PICKER_CLASS}-heading`;
        heading.id = `${base}-group-${String(index)}`;
        heading.setAttribute('role', 'presentation');
        heading.textContent = name === 'typed' ? 'Typed' : PICKER_GROUP_TITLES[name];
        group.setAttribute('aria-labelledby', heading.id);
        group.append(heading);
        list.append(group);
      }
      const option = doc.createElement('div');
      option.id = `${base}-option-${String(index)}`;
      option.className = `${TYPE_PICKER_CLASS}-option`;
      option.setAttribute('role', 'option');
      const text = entry.kind === 'collection' ? `${entry.label} ▸` : entry.label;
      option.textContent = text;
      if (entry.kind === 'collection') {
        option.setAttribute('aria-label', `${entry.label}, choose its types`);
      }
      const isCurrent = path.length === 0 && entry.kind === 'type' && entry.key === options.current;
      if (isCurrent) {
        option.setAttribute('aria-current', 'true');
      }
      option.addEventListener('pointerdown', (event) => {
        // Keep the focus in the search box.
        event.preventDefault();
      });
      option.addEventListener('click', () => {
        active = index;
        choose();
      });
      group.append(option);
      optionElements.push(option);
    });
    if (shown.length === 0) {
      const none = doc.createElement('div');
      none.className = `${TYPE_PICKER_CLASS}-empty`;
      none.textContent = 'No type matches.';
      list.append(none);
    }
    status.textContent =
      shown.length === 1 ? '1 type' : `${shown.length.toLocaleString('en-US')} types`;
    mark();
  };

  /** Marks the active option. */
  const mark = (): void => {
    optionElements.forEach((option, index) => {
      option.setAttribute('aria-selected', index === active ? 'true' : 'false');
    });
    const option = optionElements[active];
    if (option === undefined) {
      input.removeAttribute('aria-activedescendant');
      return;
    }
    input.setAttribute('aria-activedescendant', option.id);
    option.scrollIntoView({ block: 'nearest' });
  };

  /** Chooses a type (done) or completes a collection's argument. */
  const finish = (term: TypeTerm): void => {
    if (path.length === 0) {
      options.onChoose(termKey(term));
      return;
    }
    const next = addArgument(path, term);
    if (next.done !== null) {
      options.onChoose(termKey(next.done));
      return;
    }
    path = next.path;
    input.value = '';
    active = 0;
    render();
  };

  /** Opens the step of a collection's first argument. */
  const open = (entry: Extract<PickerEntry, { kind: 'collection' }>): void => {
    path = [...path, { name: entry.name, arity: entry.arity, args: [] }];
    input.value = '';
    active = 0;
    render();
  };

  const choose = (): void => {
    const entry = shown[active];
    if (entry === undefined) {
      return;
    }
    if (entry.kind === 'collection') {
      open(entry);
    } else {
      finish(entry.term);
    }
  };

  /** Goes back a step: the innermost collection is taken away. */
  const back = (): boolean => {
    if (path.length === 0) {
      return false;
    }
    path = path.slice(0, -1);
    active = 0;
    render();
    return true;
  };

  input.addEventListener('input', () => {
    active = 0;
    render();
  });
  input.addEventListener('keydown', (event) => {
    let handled = true;
    switch (event.key) {
      case 'ArrowDown':
        active = Math.min(active + 1, shown.length - 1);
        mark();
        break;
      case 'ArrowUp':
        active = Math.max(active - 1, 0);
        mark();
        break;
      case 'PageDown':
        active = Math.min(active + PAGE, shown.length - 1);
        mark();
        break;
      case 'PageUp':
        active = Math.max(active - PAGE, 0);
        mark();
        break;
      case 'Home':
      case 'End':
        if (input.value === '') {
          active = event.key === 'Home' ? 0 : shown.length - 1;
          mark();
        } else {
          handled = false;
        }
        break;
      case 'Enter':
        choose();
        break;
      case 'ArrowRight': {
        const entry = shown[active];
        if (entry?.kind === 'collection' && input.selectionStart === input.value.length) {
          open(entry);
        } else {
          handled = false;
        }
        break;
      }
      case 'ArrowLeft':
      case 'Backspace':
        handled = input.value === '' && back();
        break;
      case 'Escape':
        options.onCancel();
        break;
      case 'Tab':
        options.onCancel();
        break;
      default:
        handled = false;
    }
    if (handled) {
      event.preventDefault();
      event.stopPropagation();
    }
  });

  render();
  // The current type is the active option when the list opens.
  const currentIndex = shown.findIndex(
    (entry) => entry.kind === 'type' && entry.key === options.current,
  );
  if (currentIndex >= 0) {
    active = currentIndex;
    mark();
  }

  return {
    element: root,
    focus() {
      input.focus();
    },
    search(text: string) {
      input.value = text;
      active = 0;
      render();
    },
    entries() {
      return shown;
    },
  };
}
