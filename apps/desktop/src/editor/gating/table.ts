/**
 * What each gated block, dropdown option, type and toolbox category needs (./gate.ts). Anything
 * not listed is always offered.
 *
 * The rows come from the plans until the catalog carries them itself (the W4 contract,
 * docs/plans/m3-contracts/w4-appearance-expert.md §6.4): then this table is generated from the
 * catalog and keeps only what the catalog does not describe. A row may name a block or an option
 * that does not exist yet; it is listed in {@link AWAITED} with the work that brings it, so that a
 * row left behind by a renamed block shows up in the tests.
 */
import type { Requirement } from './gate';

/** What a requirement is about. */
export type RequirementKind = 'block' | 'option' | 'category' | 'type';

/**
 * The table's key: `block:<id>`, `option:<id>.<FIELD>=<value>`, `category:<id>` or
 * `type:<key>`.
 */
export function requirementKey(kind: RequirementKind, ...parts: string[]): string {
  if (kind === 'option') {
    const [block = '', field = '', value = ''] = parts;
    return `option:${block}.${field}=${value}`;
  }
  return `${kind}:${parts.join('')}`;
}

const ADVANCED: Requirement = Object.freeze({ advanced: true });
/** C++20's form, with an older one codegen writes for C++17: offered whatever the standard. */
const CPP20_WITH_FALLBACK: Requirement = Object.freeze({ standard: 'c++20', fallback: true });

/** The rows, as [kind, key parts, requirement]. */
const ROWS: readonly (readonly [RequirementKind, readonly string[], Requirement])[] = [
  // The bitwise compound assignments of *change* (M3-8, w2.md §1.9).
  ...['and', 'or', 'xor', 'shl', 'shr'].map(
    (value) => ['option', ['var.change', 'OP', value], ADVANCED] as const,
  ),
  // Rarely needed blocks (decision W4E-1, docs/plans/m3-contracts/w4-editor-tools.md §2.1): the
  // blocks add their headers themselves, `static_assert` is an organisation block, and a case
  // without falling through is the beginner's form.
  ['block', ['program.use_header'], ADVANCED],
  ['block', ['program.check'], ADVANCED],
  ['block', ['control.fallthrough'], ADVANCED],
  // C++20 members with a C++17 form (`b2c::starts_with`, `count`): documented, never hidden.
  ['option', ['text.find', 'OP', 'starts'], CPP20_WITH_FALLBACK],
  ['option', ['text.find', 'OP', 'ends'], CPP20_WITH_FALLBACK],
  ['block', ['coll.contains'], CPP20_WITH_FALLBACK],
  // *format … with* (03 §3.7.4, w2.md §1.10): std::format, with no older form.
  ['block', ['text.format'], { standard: 'c++20', capability: 'stdFormat' }],
  // The advanced categories of 03 §3.7.
  ['category', ['memory'], ADVANCED],
  ['category', ['concurrency'], ADVANCED],
  ['category', ['generics'], ADVANCED],
  ['category', ['raw'], ADVANCED],
];

/** Every requirement, by {@link requirementKey}. */
export const REQUIREMENTS: ReadonlyMap<string, Requirement> = new Map(
  ROWS.map(([kind, parts, requirement]) => [requirementKey(kind, ...parts), requirement]),
);

/**
 * Rows whose block, option or category is not in the catalog yet, with the work that brings it.
 * Once it lands the row simply applies; the test only checks that every other row names something
 * that exists.
 */
export const AWAITED: ReadonlyMap<string, string> = new Map([
  ['block:text.format', 'W4, the coordinator: format … with (w4-editor-tools.md §7)'],
  ['category:memory', 'M4: the Memory category'],
  ['category:concurrency', 'M4: the Concurrency category'],
  ['category:generics', 'M4: the Generics category'],
  ['category:raw', 'M4: Raw C++'],
]);

/**
 * The audit of the catalog (w4-editor-tools.md §2.1): every block, dropdown option and type field
 * type that has no row was checked and is offered always. A test fails for a catalog block, option
 * or type that is neither here nor in a row, so a block added later needs a decision; the catalog's
 * own `advanced` and `requires` keys will replace this list (§6.1).
 */
export const BASIC_BLOCKS: ReadonlySet<string> = new Set([
  // Collections.
  'coll.add',
  'coll.clear',
  'coll.for_each',
  'coll.for_each_pair',
  'coll.is_empty',
  'coll.item',
  'coll.length',
  'coll.list_of',
  'coll.next',
  'coll.remove_at',
  'coll.remove_next',
  'coll.remove_value',
  'coll.set_item',
  'coll.sort',
  // Control and loops.
  'control.break',
  'control.continue',
  'control.do_while',
  'control.for_range',
  'control.forever',
  'control.if',
  'control.repeat',
  'control.switch',
  'control.wait',
  'control.while',
  // Errors and files.
  'error.message',
  'error.rethrow',
  'error.throw',
  'error.try',
  'file.exists',
  'file.for_each_line',
  'file.open',
  'file.read_line',
  'file.write',
  // Functions, input and output, logic.
  'func.call',
  'func.call_stmt',
  'func.define',
  'func.return',
  'io.ask',
  'io.decimals',
  'io.print',
  'logic.boolean',
  'logic.not',
  'logic.operation',
  'logic.ternary',
  // Maths.
  'math.abs',
  'math.arithmetic',
  'math.clamp',
  'math.compare',
  'math.convert',
  'math.fn',
  'math.fn_two',
  'math.minmax',
  'math.number',
  'math.random_int',
  'math.random_real',
  // Program.
  'program.exit',
  'program.main',
  // Text.
  'text.as_text',
  'text.case',
  'text.char',
  'text.find',
  'text.join',
  'text.length',
  'text.letter',
  'text.literal',
  'text.part',
  'text.replace',
  'text.split',
  'text.to_number',
  'text.trim',
  // Types and variables.
  'type.enum',
  'type.enum_value',
  'type.field',
  'type.new',
  'type.set_field',
  'type.struct',
  'var.change',
  'var.declare',
  'var.flip',
  'var.get',
  'var.global',
  'var.set',
]);

/** The dropdown options checked and offered always, by `<block>.<FIELD>`. */
export const BASIC_OPTIONS: Readonly<Record<string, readonly string[]>> = {
  'coll.for_each.MODE': ['read_only', 'copy', 'editable'],
  'coll.sort.ORDER': ['ascending', 'descending'],
  'control.do_while.MODE': ['while', 'until'],
  'control.for_range.DIRECTION': ['to', 'through', 'down_to'],
  'control.while.MODE': ['while', 'until'],
  'file.open.MODE': ['read', 'write', 'append'],
  'io.ask.MODE': ['keep_asking', 'simple'],
  'io.print.SEP': ['none', 'space', 'comma'],
  'io.print.STREAM': ['out', 'err'],
  'logic.boolean.VALUE': ['true', 'false'],
  'logic.operation.OP': ['and', 'or'],
  'math.arithmetic.OP': ['add', 'sub', 'mul', 'div', 'mod'],
  'math.compare.OP': ['lt', 'le', 'gt', 'ge', 'eq', 'ne'],
  'math.fn.FN': [
    'floor',
    'ceil',
    'round',
    'sqrt',
    'cbrt',
    'exp',
    'log',
    'log10',
    'sin',
    'cos',
    'tan',
    'asin',
    'acos',
    'atan',
  ],
  'math.fn_two.FN': ['pow', 'atan2', 'hypot'],
  'math.minmax.FN': ['max', 'min'],
  'text.case.CASE': ['lower', 'upper'],
  'text.find.OP': ['contains'],
  'var.change.OP': ['add', 'sub', 'mul', 'div', 'mod', 'append'],
  'var.global.KIND': ['variable', 'constant'],
};

/**
 * The types the catalog's type fields and rows list that were checked and are offered always, as
 * the fields store them (a name, or a term's key). The type picker's other types (the collections
 * of 03 §3.5.1, the module's structs and enums) are C++17 and basic too.
 */
export const BASIC_TYPES: ReadonlySet<string> = new Set([
  'void',
  'int',
  'double',
  'bool',
  'char',
  'std::string',
  'auto',
  '{"std::vector":"int"}',
  '{"std::vector":"double"}',
  '{"std::vector":"std::string"}',
  'std::exception',
  'std::runtime_error',
  'std::invalid_argument',
  'std::out_of_range',
]);
