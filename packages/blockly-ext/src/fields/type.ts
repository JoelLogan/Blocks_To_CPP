/**
 * `b2c_type`: a type picked from the block's catalog `types` (docs/spec/03-block-language.md §3.5).
 * In M2 the labels are the Friendly names: `std::string` reads `string`; the stored value is the
 * catalog's type name, so files do not change when the C++ label mode arrives (M3).
 */
import type * as Blockly from 'blockly/core';

import { B2cOptionsFieldBase } from './options-base';
import { displayTypeName } from './values';

/** Options of a type field. */
export interface TypeFieldConfig {
  /** The type names offered, as the catalog lists them. */
  readonly types: readonly string[];
  /** The initial type (the first one when absent). */
  readonly value?: string;
  /** The accessible name of the open menu. */
  readonly ariaLabel?: string;
}

function isTypeList(types: unknown): types is readonly string[] {
  return (
    Array.isArray(types) && types.length > 0 && types.every((type) => typeof type === 'string')
  );
}

/** A type field. */
export class B2cTypeField extends B2cOptionsFieldBase {
  constructor(types: readonly string[], value: string | null = null, ariaLabel = 'Type') {
    super(
      types.map((type) => [displayTypeName(type), type] as const),
      value,
      ariaLabel,
    );
  }

  /**
   * Builds the field from a block definition: `{type: 'b2c_type', types: [...], value?}`.
   *
   * @throws TypeError when `types` is not a non-empty list of text.
   */
  static override fromJson(
    options: Blockly.FieldConfig & {
      readonly types?: unknown;
      readonly value?: unknown;
      readonly ariaLabel?: unknown;
    },
  ): B2cTypeField {
    if (!isTypeList(options.types)) {
      throw new TypeError('b2c_type needs "types": a non-empty list of type names.');
    }
    return new this(
      options.types,
      typeof options.value === 'string' ? options.value : null,
      typeof options.ariaLabel === 'string' ? options.ariaLabel : undefined,
    );
  }
}
