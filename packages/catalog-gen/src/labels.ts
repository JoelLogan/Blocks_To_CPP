// The label template grammar (docs/spec/03-block-language.md §3.11.1): `%NAME` marks where the
// field or input NAME goes, and a bare `…` (a word of its own) marks where a repeated group
// continues. Everything else is text.

import type { LabelPart } from './catalog-types.ts';

/** `%NAME`, with the same name rule as b2c-catalog: `[A-Z][A-Z0-9_]*`. */
const ARG = /^%([A-Z][A-Z0-9_]*)/;

/** The repeat marker. */
const REPEAT = '…';

/**
 * Splits a label template into text, argument and repeat parts. Text parts are trimmed, runs of
 * whitespace become one space, and whitespace-only text is dropped, because the editor lays out
 * the parts with its own spacing. A `%` that does not start a name is text.
 */
export function parseLabel(label: string): LabelPart[] {
  const parts: LabelPart[] = [];
  let text = '';
  const flush = (): void => {
    const trimmed = text.trim().replace(/\s+/gu, ' ');
    if (trimmed !== '') {
      parts.push({ text: trimmed });
    }
    text = '';
  };
  let index = 0;
  while (index < label.length) {
    const arg = ARG.exec(label.slice(index));
    if (arg?.[1] !== undefined) {
      flush();
      parts.push({ arg: arg[1] });
      index += arg[0].length;
      continue;
    }
    const char = label.charAt(index);
    if (char === REPEAT && isWordEdge(label, index - 1) && isWordEdge(label, index + 1)) {
      flush();
      parts.push({ repeat: true });
    } else {
      text += char;
    }
    index += 1;
  }
  flush();
  return parts;
}

/** Whether the position is outside the label or holds whitespace. */
function isWordEdge(label: string, index: number): boolean {
  return index < 0 || index >= label.length || /\s/u.test(label.charAt(index));
}

/** The argument names of a parsed label, in order. */
export function labelArgs(parts: readonly LabelPart[]): string[] {
  return parts.flatMap((part) => ('arg' in part ? [part.arg] : []));
}
