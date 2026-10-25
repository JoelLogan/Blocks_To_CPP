/**
 * `b2c_type`: a type picked from the block's catalog `types` (docs/spec/03-block-language.md §3.5).
 * In M2 the labels are the Friendly names: `std::string` reads `string`; the stored value is the
 * catalog's type name, so files do not change when the C++ label mode arrives (M3).
 *
 * A type field whose pattern's class no listed type fits may list none (`types = []`,
 * docs/plans/m3-contracts/w2.md §2.1, *new … with*'s struct): its menu then holds only the
 * module's own types, which W2's package C1 adds, and until one is chosen it shows
 * {@link NO_TYPE_LABEL} and holds no value (the empty text, which a project file never stores).
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
  return Array.isArray(types) && types.every((type) => typeof type === 'string');
}

/** What a type field that lists no types shows while nothing is chosen. */
export const NO_TYPE_LABEL = 'choose a struct first';

/** A type field. */
export class B2cTypeField extends B2cOptionsFieldBase {
  /** Whether the catalog lists types for the field (w2.md §2.1: a field may list none). */
  readonly listsTypes: boolean;

  constructor(types: readonly string[], value: string | null = null, ariaLabel = 'Type') {
    super(
      types.length === 0
        ? [[NO_TYPE_LABEL, '']]
        : types.map((type) => [displayTypeName(type), type] as const),
      value,
      ariaLabel,
    );
    this.listsTypes = types.length > 0;
  }

  /**
   * Whether the field holds a type: always for a field that lists types (a project value is kept
   * as it is); for one that lists none, when something other than the empty text is chosen.
   */
  hasType(): boolean {
    return this.listsTypes || (this.getValue() ?? '') !== '';
  }

  /**
   * Builds the field from a block definition: `{type: 'b2c_type', types: [...], value?}`.
   *
   * @throws TypeError when `types` is not a list of text.
   */
  static override fromJson(
    options: Blockly.FieldConfig & {
      readonly types?: unknown;
      readonly value?: unknown;
      readonly ariaLabel?: unknown;
    },
  ): B2cTypeField {
    if (!isTypeList(options.types)) {
      throw new TypeError('b2c_type needs "types": a list of type names.');
    }
    return new this(
      options.types,
      typeof options.value === 'string' ? options.value : null,
      typeof options.ariaLabel === 'string' ? options.ariaLabel : undefined,
    );
  }
}
