/** The Blockly definitions of the expression shadow types (see ./types.ts). */
import * as Blockly from 'blockly/core';

import { B2C_CSS_CLASS } from '../blocks/css';
import { ZELOS_OUTPUT_SHAPE } from '../blocks/shape';
import { B2cDropdownField } from '../fields/dropdown';
import { B2cNumberField } from '../fields/number';
import { B2cSymbolRefField } from '../fields/symbol-ref';
import { B2cTextField } from '../fields/text';
import type { TypeClass } from '../generated/catalog';
import { truncateForDisplay } from '../text';
import { EXPRESSION_BLOCK_STYLE } from '../theme/themes';
import { copyTokens, isTokenList, spaceAt, tokensDisplay } from './tokens';
import { EXPR_SHADOW_TYPE, EXPR_VALUE_FIELD, type ExprShadowExtra, type TokenRange } from './types';

/** A shadow block with its expression state. */
export interface ExprShadowBlock extends Blockly.Block {
  b2cExpr: ExprShadowExtra;
  b2cHighlight: TokenRange | null;
}

const TYPE_CLASSES: readonly TypeClass[] = ['any', 'number', 'integer', 'bool', 'text'];

const EMPTY_EXTRA: ExprShadowExtra = Object.freeze({
  check: 'any',
  absent: false,
  draft: false,
  tokens: Object.freeze([]),
});

/** The longest part of a read-only expression shown in a block (the tooltip has all of it). */
const MAX_SHOWN_CHARS = 48;

/** Label fields of the read-only shadow: before, at and after the highlighted tokens. */
const TOKENS_FIELD = Object.freeze({
  before: 'TEXT_BEFORE',
  mark: 'TEXT_MARK',
  after: 'TEXT_AFTER',
  draft: 'DRAFT',
});

/** Whether `value` is expression shadow extra state. */
export function isExprShadowExtra(value: unknown): value is ExprShadowExtra {
  if (typeof value !== 'object' || value === null || Array.isArray(value)) {
    return false;
  }
  const { check, absent, draft, tokens } = value as Partial<Record<keyof ExprShadowExtra, unknown>>;
  return (
    typeof check === 'string' &&
    TYPE_CLASSES.includes(check as TypeClass) &&
    typeof absent === 'boolean' &&
    typeof draft === 'boolean' &&
    isTokenList(tokens)
  );
}

/** Whether a range selects tokens of a list of `length` tokens. */
export function isTokenRange(range: TokenRange, length: number): boolean {
  return (
    Number.isInteger(range.start) &&
    Number.isInteger(range.end) &&
    range.start >= 0 &&
    range.start < range.end &&
    range.end <= length
  );
}

/** Shows a read-only shadow's tokens, with the highlighted range in its own label. */
export function renderTokens(block: ExprShadowBlock): void {
  const { tokens, draft } = block.b2cExpr;
  const range =
    block.b2cHighlight !== null && isTokenRange(block.b2cHighlight, tokens.length)
      ? block.b2cHighlight
      : null;
  const parts =
    range === null
      ? [tokensDisplay(tokens), '', '']
      : [
          tokensDisplay(tokens, 0, range.start) + (spaceAt(tokens, range.start) ? ' ' : ''),
          tokensDisplay(tokens, range.start, range.end),
          (spaceAt(tokens, range.end) ? ' ' : '') + tokensDisplay(tokens, range.end),
        ];
  const names = [TOKENS_FIELD.before, TOKENS_FIELD.mark, TOKENS_FIELD.after];
  names.forEach((name, index) => {
    const text = truncateForDisplay(parts[index] ?? '', MAX_SHOWN_CHARS);
    const field = block.getField(name);
    if (field !== null) {
      field.setValue(text);
      field.setVisible(text.length > 0);
    }
  });
  block.getField(TOKENS_FIELD.draft)?.setVisible(draft);
  const full = tokensDisplay(tokens);
  block.setTooltip(
    draft
      ? `${full}\nAn unfinished expression, kept as it is.`
      : full.length > 0
        ? full
        : 'An empty expression.',
  );
  if (block instanceof Blockly.BlockSvg) {
    // Hiding or showing a label changes the block's width.
    void block.queueRender();
  }
}

/** The parts every expression shadow shares: style, output and its expression state. */
const SHADOW_MIXIN = {
  saveExtraState(this: ExprShadowBlock): ExprShadowExtra {
    const { check, absent, draft, tokens } = this.b2cExpr;
    return { check, absent, draft, tokens: copyTokens(tokens) };
  },

  loadExtraState(this: ExprShadowBlock, state: unknown): void {
    if (!isExprShadowExtra(state)) {
      return;
    }
    this.b2cExpr = Object.freeze({
      check: state.check,
      absent: state.absent,
      draft: state.draft,
      tokens: Object.freeze(copyTokens(state.tokens)),
    });
    this.setOutputShape(
      state.check === 'bool' ? ZELOS_OUTPUT_SHAPE.hexagonal : ZELOS_OUTPUT_SHAPE.round,
    );
    if (this.type === EXPR_SHADOW_TYPE.tokens) {
      renderTokens(this);
    }
  },
};

/** A Blocks2Cpp field as Blockly's general field type (`Input.appendField` cannot infer it). */
function asField(field: Blockly.Field): Blockly.Field {
  return field;
}

function initShadow(block: ExprShadowBlock): Blockly.Input {
  block.b2cExpr = EMPTY_EXTRA;
  block.b2cHighlight = null;
  block.setStyle(EXPRESSION_BLOCK_STYLE);
  block.setOutput(true, null);
  block.setOutputShape(ZELOS_OUTPUT_SHAPE.round);
  return block.appendDummyInput('b2c_row0');
}

const DEFINITIONS: Readonly<Record<string, object>> = {
  [EXPR_SHADOW_TYPE.num]: {
    ...SHADOW_MIXIN,
    init(this: ExprShadowBlock): void {
      initShadow(this).appendField(asField(new B2cNumberField('0')), EXPR_VALUE_FIELD);
    },
  },
  [EXPR_SHADOW_TYPE.str]: {
    ...SHADOW_MIXIN,
    init(this: ExprShadowBlock): void {
      initShadow(this)
        .appendField(new Blockly.FieldLabel('"'))
        .appendField(asField(new B2cTextField('')), EXPR_VALUE_FIELD)
        .appendField(new Blockly.FieldLabel('"'));
    },
  },
  [EXPR_SHADOW_TYPE.chr]: {
    ...SHADOW_MIXIN,
    init(this: ExprShadowBlock): void {
      initShadow(this)
        .appendField(new Blockly.FieldLabel("'"))
        .appendField(asField(new B2cTextField('a', { singleChar: true })), EXPR_VALUE_FIELD)
        .appendField(new Blockly.FieldLabel("'"));
    },
  },
  [EXPR_SHADOW_TYPE.kw]: {
    ...SHADOW_MIXIN,
    init(this: ExprShadowBlock): void {
      initShadow(this).appendField(
        asField(
          new B2cDropdownField(
            [
              ['true', 'true'],
              ['false', 'false'],
            ],
            'true',
            'Value',
          ),
        ),
        EXPR_VALUE_FIELD,
      );
    },
  },
  [EXPR_SHADOW_TYPE.ref]: {
    ...SHADOW_MIXIN,
    init(this: ExprShadowBlock): void {
      initShadow(this).appendField(
        asField(new B2cSymbolRefField(null, { kinds: 'variables' })),
        EXPR_VALUE_FIELD,
      );
    },
  },
  [EXPR_SHADOW_TYPE.tokens]: {
    ...SHADOW_MIXIN,
    init(this: ExprShadowBlock): void {
      initShadow(this)
        .appendField(new Blockly.FieldLabel(''), TOKENS_FIELD.before)
        .appendField(new Blockly.FieldLabel('', B2C_CSS_CLASS.tokenHighlight), TOKENS_FIELD.mark)
        .appendField(new Blockly.FieldLabel(''), TOKENS_FIELD.after)
        .appendField(new Blockly.FieldLabel('(draft)', B2C_CSS_CLASS.draft), TOKENS_FIELD.draft);
      renderTokens(this);
    },
  },
};

/** Registers the expression shadow block types. Idempotent. */
export function registerExpressionShadows(): void {
  for (const [type, definition] of Object.entries(DEFINITIONS)) {
    if (!Object.hasOwn(Blockly.Blocks, type)) {
      Blockly.Blocks[type] = definition;
    }
  }
}
