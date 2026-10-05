/** The internal expression shadows (src/shadows/). */
import * as Blockly from 'blockly/core';
import { beforeEach, describe, expect, it } from 'vitest';

import {
  B2C_CSS_CLASS,
  B2cSymbolRefField,
  EXPR_SHADOW_TYPE,
  ExprShadowError,
  ZELOS_OUTPUT_SHAPE,
  catalogBlock,
  exprShadowState,
  getTokenHighlight,
  isCatalogBlockType,
  isExprShadowType,
  readExprShadow,
  refreshSymbolNames,
  setTokenHighlight,
  tokensDisplay,
  type TokenJson,
  type TypeClass,
} from '../src';
import { fakeSymbols, headlessWorkspace, renderedWorkspace, setUpBlocks, symbol } from './helpers';

beforeEach(() => {
  setUpBlocks();
});

/** A `control.repeat` block whose TIMES input shows the shadow for `tokens`; returns the shadow. */
function shadowFor(
  tokens: readonly TokenJson[],
  {
    draft = false,
    check = 'integer',
    absent = false,
    workspace = headlessWorkspace(),
  }: {
    draft?: boolean;
    check?: TypeClass;
    absent?: boolean;
    workspace?: Blockly.Workspace;
  } = {},
): Blockly.Block {
  const parent = Blockly.serialization.blocks.append(
    {
      type: 'control.repeat',
      inputs: { TIMES: { shadow: exprShadowState(tokens, draft, check, absent) } },
    },
    workspace,
  );
  const shadow = parent.getInputTargetBlock('TIMES');
  if (shadow === null) {
    throw new Error('no shadow');
  }
  return shadow;
}

describe('exprShadowState', () => {
  it('makes an editable literal for one literal token and a dropdown for one reference', () => {
    expect(exprShadowState([{ num: '42' }], false, 'number', false)).toMatchObject({
      type: EXPR_SHADOW_TYPE.num,
      fields: { VALUE: '42' },
    });
    expect(exprShadowState([{ str: 'hi' }], false, 'any', false).type).toBe(EXPR_SHADOW_TYPE.str);
    expect(exprShadowState([{ chr: 'a' }], false, 'any', false).type).toBe(EXPR_SHADOW_TYPE.chr);
    expect(exprShadowState([{ kw: 'true' }], false, 'bool', true).type).toBe(EXPR_SHADOW_TYPE.kw);
    expect(exprShadowState([{ ref: 's_x' }], false, 'any', false)).toMatchObject({
      type: EXPR_SHADOW_TYPE.ref,
      fields: { VALUE: { ref: 's_x' } },
    });
  });

  it('makes a read-only shadow for everything else', () => {
    for (const tokens of [
      [],
      [{ op: '+' }],
      [{ text: 'x +' }],
      [{ ref: 's_a' }, { op: '<' }, { ref: 's_b' }],
    ] as TokenJson[][]) {
      expect(exprShadowState(tokens, false, 'any', false).type).toBe(EXPR_SHADOW_TYPE.tokens);
    }
    expect(exprShadowState([{ num: '1' }], true, 'any', false).type).toBe(EXPR_SHADOW_TYPE.tokens);
  });

  it('refuses token lists a project file could not hold', () => {
    expect(() =>
      exprShadowState([{ bogus: 'x' } as unknown as TokenJson], false, 'any', false),
    ).toThrow(ExprShadowError);
    expect(() => exprShadowState([{ ref: 'bad id!' }], false, 'any', false)).toThrow(
      ExprShadowError,
    );
    expect(() => exprShadowState([{ str: 'a\u0000' }], false, 'any', false)).toThrow(
      ExprShadowError,
    );
    expect(() =>
      exprShadowState(
        Array.from({ length: 513 }, () => ({ op: '+' })),
        false,
        'any',
        false,
      ),
    ).toThrow(ExprShadowError);
    expect(
      exprShadowState(
        Array.from({ length: 512 }, () => ({ op: '+' })),
        false,
        'any',
        false,
      ).type,
    ).toBe(EXPR_SHADOW_TYPE.tokens);
  });

  it('copies the tokens it is given', () => {
    const tokens: TokenJson[] = [{ num: '1' }, { op: '+' }];
    const state = exprShadowState(tokens, false, 'any', false);
    (tokens[0] as { num: string }).num = '2';
    expect((state.extraState as { tokens: TokenJson[] }).tokens[0]).toEqual({ num: '1' });
  });
});

describe('shadows in a workspace', () => {
  it('are internal: never catalog blocks, and shadows in their input', () => {
    for (const type of Object.values(EXPR_SHADOW_TYPE)) {
      expect(isExprShadowType(type)).toBe(true);
      expect(isCatalogBlockType(type)).toBe(false);
    }
    expect(isExprShadowType('math.number')).toBe(false);
    expect(shadowFor([{ num: '10' }]).isShadow()).toBe(true);
  });

  it('read back what they were made from, including absent and draft', () => {
    const cases: { tokens: TokenJson[]; draft: boolean; absent: boolean }[] = [
      { tokens: [{ num: '007' }], draft: false, absent: true },
      { tokens: [{ str: 'Hello, world!' }], draft: false, absent: false },
      { tokens: [{ chr: 'x' }], draft: false, absent: false },
      { tokens: [{ kw: 'false' }], draft: false, absent: true },
      { tokens: [{ ref: 's_guess' }], draft: false, absent: false },
      {
        tokens: [{ ref: 's_guess' }, { op: '<' }, { ref: 's_secret' }],
        draft: false,
        absent: false,
      },
      { tokens: [{ num: '1' }, { op: '+' }], draft: true, absent: false },
      { tokens: [{ text: 'guess <' }], draft: true, absent: false },
      { tokens: [], draft: false, absent: false },
    ];
    for (const { tokens, draft, absent } of cases) {
      const shadow = shadowFor(tokens, { draft, absent });
      expect(readExprShadow(shadow), JSON.stringify(tokens)).toEqual({ tokens, draft, absent });
      // And again after Blockly saves and loads the parent (copy, duplicate, undo of a delete).
      const parent = shadow.getParent();
      const saved = parent === null ? null : Blockly.serialization.blocks.save(parent);
      if (saved === null) {
        throw new Error('nothing saved');
      }
      const copy = Blockly.serialization.blocks.append(saved, shadow.workspace);
      const copied = copy.getInputTargetBlock('TIMES');
      expect(copied === null ? null : readExprShadow(copied)).toEqual({ tokens, draft, absent });
    }
  });

  it('round-trip every expression slot of the example projects', () => {
    const files = import.meta.glob('../../../examples/*.b2c', {
      query: '?raw',
      import: 'default',
      eager: true,
    });
    const slots: { tokens: TokenJson[]; draft: boolean }[] = [];
    const visit = (value: unknown): void => {
      if (Array.isArray(value)) {
        value.forEach(visit);
      } else if (typeof value === 'object' && value !== null) {
        const record = value as Record<string, unknown>;
        if (Array.isArray(record['expr'])) {
          slots.push({ tokens: record['expr'] as TokenJson[], draft: record['draft'] === true });
        }
        Object.values(record).forEach(visit);
      }
    };
    for (const text of Object.values(files)) {
      visit(JSON.parse(text));
    }
    expect(Object.keys(files).length).toBeGreaterThanOrEqual(15);
    expect(slots.length).toBeGreaterThan(50);
    const workspace = headlessWorkspace();
    for (const { tokens, draft } of slots) {
      const shadow = shadowFor(tokens, { draft, workspace });
      expect(readExprShadow(shadow)).toEqual({ tokens, draft, absent: false });
    }
  });

  it('stay absent until the value changes', () => {
    const shadow = shadowFor([{ num: '10' }], { absent: true });
    expect(readExprShadow(shadow)?.absent).toBe(true);
    shadow.setFieldValue('12', 'VALUE');
    expect(readExprShadow(shadow)).toEqual({
      tokens: [{ num: '12' }],
      draft: false,
      absent: false,
    });
    shadow.setFieldValue('10', 'VALUE');
    expect(readExprShadow(shadow)?.absent).toBe(true);
  });

  it('take the outline of their input: hexagonal for bool, round otherwise', () => {
    expect(shadowFor([{ kw: 'true' }], { check: 'bool' }).getOutputShape()).toBe(
      ZELOS_OUTPUT_SHAPE.hexagonal,
    );
    expect(shadowFor([{ num: '1' }], { check: 'integer' }).getOutputShape()).toBe(
      ZELOS_OUTPUT_SHAPE.round,
    );
    expect(shadowFor([{ op: '+' }], { check: 'any' }).getOutputShape()).toBe(
      ZELOS_OUTPUT_SHAPE.round,
    );
  });

  it('list the variables in scope at their slot (the parent block and input)', () => {
    const workspace = headlessWorkspace();
    const shadow = shadowFor([{ ref: 's_a' }], { workspace });
    const parentId = shadow.getParent()?.id ?? '';
    const queries = fakeSymbols(
      { [`${parentId}/TIMES`]: [symbol('s_a', 'a'), symbol('s_f', 'f', 'function')] },
      { s_a: 'a' },
    );
    const field = shadow.getField('VALUE');
    expect(field instanceof B2cSymbolRefField && field.menuOptions()).toEqual([['a', 's_a']]);
    expect(queries).toEqual([`${parentId}/TIMES`]);
  });

  it('ignore malformed extra state', () => {
    const workspace = headlessWorkspace();
    const block = Blockly.serialization.blocks.append(
      {
        type: EXPR_SHADOW_TYPE.tokens,
        extraState: { check: 'nonsense', absent: false, draft: false, tokens: [] },
      },
      workspace,
    );
    expect(readExprShadow(block)).toEqual({ tokens: [], draft: false, absent: false });
    expect(readExprShadow(workspace.newBlock('math.number'))).toBeNull();
  });
});

describe('read-only expressions', () => {
  it('show tokens with references by their current names', () => {
    fakeSymbols({}, { s_guess: 'guess', s_secret: 'secret' });
    const tokens: TokenJson[] = [
      { ref: 's_guess' },
      { op: '<' },
      { ref: 's_secret' },
      { op: '+' },
      { num: '1' },
    ];
    expect(tokensDisplay(tokens)).toBe('guess < secret + 1');
    expect(
      tokensDisplay([
        { op: '(' },
        { str: 'a\u200b' },
        { op: ')' },
        { chr: 'c' },
        { ref: 's_gone' },
      ]),
    ).toBe(`("a⟨U+200B⟩") 'c' missing (s_gone)`);
  });

  it('follow renames when the names are refreshed', () => {
    const names: Record<string, string> = { s_guess: 'guess', s_secret: 'secret' };
    fakeSymbols({}, names);
    const shadow = shadowFor([{ ref: 's_guess' }, { op: '<' }, { ref: 's_secret' }]);
    expect(shadow.getFieldValue('TEXT_BEFORE')).toBe('guess < secret');
    names['s_guess'] = 'attempt';
    refreshSymbolNames(shadow.workspace);
    expect(shadow.getFieldValue('TEXT_BEFORE')).toBe('attempt < secret');
    expect(shadow.getTooltip()).toBe('attempt < secret');
  });

  it('mark the tokens a diagnostic points at', () => {
    fakeSymbols({}, { s_a: 'a', s_b: 'b' });
    const shadow = shadowFor([
      { ref: 's_a' },
      { op: '<' },
      { ref: 's_b' },
      { op: '+' },
      { num: '1' },
    ]);
    setTokenHighlight(shadow, { start: 2, end: 3 });
    expect(getTokenHighlight(shadow)).toEqual({ start: 2, end: 3 });
    expect([
      shadow.getFieldValue('TEXT_BEFORE'),
      shadow.getFieldValue('TEXT_MARK'),
      shadow.getFieldValue('TEXT_AFTER'),
    ]).toEqual(['a < ', 'b', ' + 1']);
    setTokenHighlight(shadow, { start: 0, end: 9 });
    expect(getTokenHighlight(shadow)).toBeNull();
    expect(shadow.getFieldValue('TEXT_BEFORE')).toBe('a < b + 1');
    expect(shadow.getField('TEXT_MARK')?.isVisible()).toBe(false);
    setTokenHighlight(shadow, null);
    expect(getTokenHighlight(shadow)).toBeNull();
  });

  it('show drafts as such and keep long text short in the block', () => {
    const long: TokenJson[] = Array.from({ length: 60 }, (_, index) => ({ num: String(index) }));
    const shadow = shadowFor(long, { draft: true });
    expect(shadow.getField('DRAFT')?.isVisible()).toBe(true);
    expect(String(shadow.getFieldValue('TEXT_BEFORE')).length).toBeLessThanOrEqual(48);
    expect(shadow.getTooltip()).toContain('An unfinished expression');
    expect(shadow.getTooltip()).toContain('59');
  });

  it('mark a one-token shadow as a whole when rendered', () => {
    const workspace = renderedWorkspace();
    const shadow = shadowFor([{ num: '5' }], { workspace });
    setTokenHighlight(shadow, { start: 0, end: 1 });
    expect(
      shadow instanceof Blockly.BlockSvg &&
        shadow.getSvgRoot().classList.contains(B2C_CSS_CLASS.tokenHighlight),
    ).toBe(true);
    setTokenHighlight(shadow, null);
    expect(
      shadow instanceof Blockly.BlockSvg &&
        shadow.getSvgRoot().classList.contains(B2C_CSS_CLASS.tokenHighlight),
    ).toBe(false);
    setTokenHighlight(workspace.newBlock('math.number'), { start: 0, end: 1 });
  });
});

describe('catalog defaults', () => {
  it('can all be shown as shadows', () => {
    for (const type of ['io.print', 'control.repeat', 'logic.not', 'math.random_int']) {
      for (const input of catalogBlock(type)?.inputs ?? []) {
        if (input.default.length > 0) {
          expect(() => exprShadowState(input.default, false, input.check, true)).not.toThrow();
        }
      }
    }
  });
});
