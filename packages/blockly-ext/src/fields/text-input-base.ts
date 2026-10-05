/**
 * The common base of the text-entry fields (name, number, text). It keeps Blockly's inline editor
 * (an `<input>` in Blockly's widget div, created with the DOM API, never from HTML) and adds:
 *
 * - two levels of rules: a value set by code (loading a project, undo, paste) only has to follow
 *   the project text rules, so every loadable value round-trips unchanged; text typed in the editor
 *   must also follow the field's stricter entry rule;
 * - display through {@link visibleInvisibles}, so invisible characters show as `⟨U+200B⟩` in the
 *   block (the stored value is unchanged);
 * - an accessible name on the editor's `<input>`.
 *
 * Values are rendered by Blockly as SVG text nodes only (docs/security/custom-field-review-checklist.md).
 */
import * as Blockly from 'blockly/core';

import { isCleanFieldText, visibleInvisibles } from '../text';

/** Options every text-entry field accepts. */
export interface TextEntryConfig extends Blockly.FieldTextInputConfig {
  /** The accessible name of the editor's input box. */
  readonly ariaLabel?: string;
}

/** The longest text shown in a block before it is cut with `…` (the stored value is unchanged). */
export const MAX_FIELD_DISPLAY_CHARS = 40;

/** A text-entry field whose value is a string. */
export abstract class B2cTextInputBase extends Blockly.FieldTextInput {
  /** The accessible name of the editor input. */
  protected ariaLabel = '';

  /**
   * @param defaultAriaLabel - the input's accessible name when the config gives none.
   */
  protected constructor(value: string | null, defaultAriaLabel: string, config?: TextEntryConfig) {
    super(Blockly.Field.SKIP_SETUP);
    this.maxDisplayLength = MAX_FIELD_DISPLAY_CHARS;
    this.ariaLabel = defaultAriaLabel;
    if (config) {
      this.configure_(config);
    }
    if (value !== null) {
      this.setValue(value);
    }
  }

  protected override configure_(config: TextEntryConfig): void {
    super.configure_(config);
    if (typeof config.ariaLabel === 'string' && config.ariaLabel.length > 0) {
      this.ariaLabel = config.ariaLabel;
    }
  }

  /**
   * The stricter rule for text the user types into the editor. Called only with text that already
   * follows the project text rules.
   */
  protected abstract acceptsEntry(text: string): boolean;

  /**
   * Accepts a string that follows the project text rules; while the editor is open the text must
   * also pass {@link acceptsEntry}. Anything else is refused (null) and the field keeps its value.
   */
  protected override doClassValidation_(newValue?: unknown): string | null {
    if (typeof newValue !== 'string' || !isCleanFieldText(newValue)) {
      return null;
    }
    if (this.isBeingEdited_ && !this.acceptsEntry(newValue)) {
      return null;
    }
    return newValue;
  }

  /** While editing, the raw input (the input box covers the block text); otherwise the display text. */
  protected override getText_(): string | null {
    if (this.isBeingEdited_ && this.htmlInput_) {
      return this.htmlInput_.value;
    }
    return this.displayText(this.getValue() ?? '');
  }

  /** The text shown in the block for a value. Invisible characters become placeholders. */
  protected displayText(value: string): string {
    return visibleInvisibles(value, { lineBreaks: true });
  }

  protected override widgetCreate_(): HTMLInputElement | HTMLTextAreaElement {
    const input = super.widgetCreate_();
    input.setAttribute('aria-label', this.ariaLabel);
    input.setAttribute('autocomplete', 'off');
    return input;
  }
}
