/**
 * Text the mutators show on blocks. Everything is rendered by Blockly label fields as SVG text,
 * never as HTML.
 */
import * as Blockly from 'blockly/core';

/** The most characters of a name shown in a label. */
export const MAX_LABEL_CHARS = 64;

/** A valid C++ identifier of at most 64 characters, shown as it is. */
const PLAIN_NAME = /^[A-Za-z_][A-Za-z0-9_]{0,63}$/;

/**
 * A symbol name as a label shows it. Valid names are ASCII identifiers and are shown unchanged.
 * In any other name, each character outside printable ASCII is shown as `⟨U+XXXX⟩` (so invisible,
 * bidi and look-alike characters can be seen, 08 §8.4), and the text is cut to 64 characters.
 */
export function displayName(name: string): string {
  if (PLAIN_NAME.test(name)) {
    return name;
  }
  const out: string[] = [];
  for (const char of name) {
    if (out.length === MAX_LABEL_CHARS) {
      out.push('…');
      break;
    }
    const code = char.codePointAt(0) ?? 0;
    out.push(
      code >= 0x20 && code < 0x7f
        ? char
        : `⟨U+${code.toString(16).toUpperCase().padStart(4, '0')}⟩`,
    );
  }
  return out.join('');
}

/**
 * A label that shows the current text of another field of its block: the operator shown between
 * the items of `logic.operation` (`a and b and c`). It reads the text when it renders; call
 * `markDirty()` after the other field changes.
 */
export class MirrorLabel extends Blockly.FieldLabel {
  /** The name of the field whose text is shown. */
  readonly sourceName: string;

  constructor(sourceName: string) {
    super('');
    this.sourceName = sourceName;
  }

  protected override getText_(): string {
    return this.getSourceBlock()?.getField(this.sourceName)?.getText() ?? '';
  }
}
