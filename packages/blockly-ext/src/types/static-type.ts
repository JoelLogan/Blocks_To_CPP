/**
 * The editor's copy of the type rules' shapes (docs/plans/m3-contracts/types.md §3.2), mirroring
 * `b2c_ir::types`: a static type as an object, the classes of a catalog `check` and `where`, the
 * type functions of catalog patterns, and what the rules need to know about user types.
 *
 * Declarations only for now: the functions that read and compare these types (`parseType`,
 * `toTypeJson`, `typeKey`, `staticConversion`, `inClass`, `checkAccepts`, `applyTypeFn`,
 * `typeLabel`) come with the editor's type package of milestone M3's first wave, checked against
 * the shared fixture that `b2c-lang` writes. Until then the connection checker keeps the string
 * union of `../checker/types`, which this module does not replace; the package entry exports
 * neither, so the two never clash.
 */
import type { StaticConversion } from '../checker/types';

export type { StaticConversion };

/** The names of the fundamental types and `std::string`, as files write them. */
export type PrimitiveName = 'void' | 'bool' | 'char' | 'int' | 'double' | 'std::string';

/**
 * A static type (`b2c_ir::Type`): a fundamental type or `std::string`, a standard-library type with
 * its arguments (`name` is its qualified C++ name, such as `std::vector`), a user struct or enum by
 * the symbol ID of its definition, or the type of an expression that already has an error, which
 * fits everything.
 */
export type StaticType =
  | { readonly kind: 'prim'; readonly name: PrimitiveName }
  | { readonly kind: 'lib'; readonly name: string; readonly args: readonly StaticType[] }
  | { readonly kind: 'named'; readonly id: string }
  | { readonly kind: 'error' };

/**
 * A class of types (`b2c_ir::TypeClass`, types.md §1.3), as catalog files write it: what an input's
 * `check` accepts and what a signature's `where` allows a type variable to be.
 */
export type TypeClass =
  | 'any'
  | 'number'
  | 'integer'
  | 'bool'
  | 'text'
  | 'value'
  | 'collection'
  | 'sequence'
  | 'indexable'
  | 'insertable'
  | 'searchable'
  | 'iterable'
  | 'keyed'
  | 'adaptor'
  | 'listable'
  | 'sortable'
  | 'switchable'
  | 'comparable'
  | 'printable'
  | 'struct'
  | 'enum'
  | 'input_file'
  | 'output_file'
  | 'exception';

/**
 * A type function of catalog patterns (`b2c_ir::TypeFn`, types.md §1.4): what one item is, what
 * *item of* takes, and what *contains* looks for.
 */
export type TypeFn = 'elem' | 'index' | 'key';

/** A user type as the facts describe it (`b2c_ir::UserType`). */
export interface UserTypeTs {
  readonly name: string;
  readonly kind: 'struct' | 'enum';
  /** Ticked *comparable*, and every field's type is comparable; enums always are. */
  readonly comparable: boolean;
  /** Ticked *printable*, and every field's type is printable; enums always are. */
  readonly printable: boolean;
}

/**
 * What the type rules need to know about user types (`b2c_ir::TypeFacts`), answered from the
 * latest analysis's symbols. `null` when the facts do not know the symbol, or it is not a user
 * type: class membership is then not known, and a value of that type connects.
 */
export interface TypeFactsTs {
  userType(id: string): UserTypeTs | null;
}
