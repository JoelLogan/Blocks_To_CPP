/**
 * Checking a name typed into *Make a variable* (docs/spec/03-block-language.md §3.6,
 * docs/spec/08-security.md §8.4.1) before the declaration is created.
 *
 * The rules are the declaration field's entry rule (`NAME_ENTRY_PATTERN` of
 * `@blocks2cpp/blockly-ext`) plus the parts of the analyser's identifier check
 * (`b2c_ir::text::Ident::new`, reported as B2C-E0220) that can be decided from the name alone: it
 * starts with a letter, has no `__`, is not a C++ keyword and is not reserved by Blocks2Cpp. The
 * longer lists the analyser also checks (standard macros, C library names at namespace scope) stay
 * with the analyser, which reports them on the block. Every check here is one the analyser also
 * makes, so a name refused here would never build.
 */
import { MAX_NAME_CHARS, NAME_ENTRY_PATTERN } from '@blocks2cpp/blockly-ext';

/**
 * C++ keywords (up to C++26), alternative tokens and contextual keywords: the same list as
 * `KEYWORDS` in crates/b2c-ir/src/text.rs. identifiers.test.ts compares the two.
 */
export const CPP_KEYWORDS: ReadonlySet<string> = new Set([
  'alignas',
  'alignof',
  'and',
  'and_eq',
  'asm',
  'auto',
  'bitand',
  'bitor',
  'bool',
  'break',
  'case',
  'catch',
  'char',
  'char16_t',
  'char32_t',
  'char8_t',
  'class',
  'co_await',
  'co_return',
  'co_yield',
  'compl',
  'concept',
  'const',
  'const_cast',
  'consteval',
  'constexpr',
  'constinit',
  'continue',
  'contract_assert',
  'decltype',
  'default',
  'delete',
  'do',
  'double',
  'dynamic_cast',
  'else',
  'enum',
  'explicit',
  'export',
  'extern',
  'false',
  'final',
  'float',
  'for',
  'friend',
  'goto',
  'if',
  'import',
  'inline',
  'int',
  'long',
  'module',
  'mutable',
  'namespace',
  'new',
  'noexcept',
  'not',
  'not_eq',
  'nullptr',
  'operator',
  'or',
  'or_eq',
  'override',
  'private',
  'protected',
  'public',
  'register',
  'reinterpret_cast',
  'requires',
  'return',
  'short',
  'signed',
  'sizeof',
  'static',
  'static_assert',
  'static_cast',
  'struct',
  'switch',
  'template',
  'this',
  'thread_local',
  'throw',
  'true',
  'try',
  'typedef',
  'typeid',
  'typename',
  'union',
  'unsigned',
  'using',
  'virtual',
  'void',
  'volatile',
  'wchar_t',
  'while',
  'xor',
  'xor_eq',
]);

/** Names the generated code itself uses (`GENERATOR_RESERVED` in b2c-ir). */
const GENERATOR_RESERVED: ReadonlySet<string> = new Set(['main', 'std']);

/** The prefix of names the generator creates; user names may not start with it (any case). */
const GENERATED_PREFIX = 'b2c';

/** Quotes a name for a message. */
function quoted(name: string): string {
  return `“${name}”`;
}

/**
 * Why `name` cannot be the name of a new variable, or `null` when it can. `taken` holds the names
 * the new variable would clash with where it is inserted.
 */
export function variableNameProblem(name: string, taken: ReadonlySet<string>): string | null {
  if (name.length === 0) {
    return 'Type a name for the variable.';
  }
  if (name.length > MAX_NAME_CHARS) {
    return `A name can be at most ${String(MAX_NAME_CHARS)} characters long.`;
  }
  if (!NAME_ENTRY_PATTERN.test(name)) {
    return 'A name can only contain letters (A–Z, a–z), digits and underscores.';
  }
  if (!/^[A-Za-z]/.test(name)) {
    return 'A name must start with a letter (A–Z or a–z).';
  }
  if (name.includes('__')) {
    return 'A name cannot contain two underscores in a row (C++ reserves those names).';
  }
  if (CPP_KEYWORDS.has(name)) {
    return `${quoted(name)} is a C++ keyword. Choose another name.`;
  }
  if (GENERATOR_RESERVED.has(name) || name.toLowerCase().startsWith(GENERATED_PREFIX)) {
    return `${quoted(name)} is reserved by Blocks2Cpp. Choose another name.`;
  }
  if (taken.has(name)) {
    return `There is already something called ${quoted(name)} here. Choose another name.`;
  }
  return null;
}
