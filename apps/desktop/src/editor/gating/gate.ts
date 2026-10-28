/**
 * Feature gating (docs/spec/06-compiler-pipeline.md §6.10, 03 §3.7, 10 §10.3 Q8): whether the
 * editor offers a block, a dropdown option, a type or a toolbox category, given the editor's mode
 * (*Show advanced blocks*), the project's C++ standard and what the toolchain a build would use was
 * probed to support (07 §7.3). Pure: no Blockly, no store. The requirements are in ./table.ts.
 *
 * Gating only decides what is *offered*. A block already in a project is never refused, hidden or
 * changed for it: it loads, shows and generates as it is, and the build reports what the standard
 * or the compiler cannot do (`B2C-E0703`).
 */
import type { CppStandard } from '@blocks2cpp/b2c-core-wasm';
import type { LabelMode } from '@blocks2cpp/blockly-ext';
import type { ToolchainCapabilities } from '@blocks2cpp/ipc-types';

import { REQUIREMENTS, requirementKey, type RequirementKind } from './table';

export type { LabelMode };

/** What the editor shows, and for which project and compiler. */
export interface EditorMode {
  /** The block labels. */
  readonly labels: LabelMode;
  /** Whether advanced categories, blocks and options are offered. */
  readonly showAdvanced: boolean;
  /** The project's C++ standard. */
  readonly standard: CppStandard;
  /** The probed capabilities of the toolchain a build would use, or `null` while none is known. */
  readonly toolchain: ToolchainCapabilities | null;
}

/** A capability a toolchain is probed for that a requirement can name. */
export type Capability = 'stdFormat';

/** What a block, an option, a type or a category needs. */
export interface Requirement {
  /** Offered only with *Show advanced blocks*. */
  readonly advanced?: true;
  /** The oldest C++ standard that has it. */
  readonly standard?: CppStandard;
  /** A capability the toolchain must have been probed to have. */
  readonly capability?: Capability;
  /** The code generator writes an older form when the standard or the toolchain lacks it. */
  readonly fallback?: true;
}

/** Whether something is offered, and if not, why. */
export type Gate =
  | { readonly allowed: true }
  | { readonly allowed: false; readonly reason: 'advanced' }
  | { readonly allowed: false; readonly reason: 'standard'; readonly needs: CppStandard }
  | { readonly allowed: false; readonly reason: 'toolchain'; readonly needs: Capability };

const ALLOWED: Gate = Object.freeze({ allowed: true });

/** The standards in order, oldest first. */
const STANDARDS: readonly CppStandard[] = ['c++17', 'c++20', 'c++23', 'c++26'];

/** Whether `standard` is `needed` or newer. */
export function standardAtLeast(standard: CppStandard, needed: CppStandard): boolean {
  return STANDARDS.indexOf(standard) >= STANDARDS.indexOf(needed);
}

/**
 * Whether `requirement` is met in `mode`. A requirement with a fallback is met whatever the
 * standard and toolchain (codegen writes the older form); without one, an older standard or a
 * toolchain without the capability refuses it. A toolchain that is not known yet (discovery has
 * not finished, or none is usable) refuses nothing: the build reports the problem rather than the
 * toolbox hiding blocks that may well work.
 */
export function gate(requirement: Requirement | undefined, mode: EditorMode): Gate {
  if (requirement === undefined) {
    return ALLOWED;
  }
  if (requirement.advanced === true && !mode.showAdvanced) {
    return { allowed: false, reason: 'advanced' };
  }
  if (requirement.fallback === true) {
    return ALLOWED;
  }
  if (requirement.standard !== undefined && !standardAtLeast(mode.standard, requirement.standard)) {
    return { allowed: false, reason: 'standard', needs: requirement.standard };
  }
  if (
    requirement.capability !== undefined &&
    mode.toolchain !== null &&
    !mode.toolchain[requirement.capability]
  ) {
    return { allowed: false, reason: 'toolchain', needs: requirement.capability };
  }
  return ALLOWED;
}

function lookUp(kind: RequirementKind, ...parts: string[]): Requirement | undefined {
  return REQUIREMENTS.get(requirementKey(kind, ...parts));
}

/** Whether the block `blockType` is offered (in the toolbox, Quick Insert, toolbox search). */
export function blockGate(blockType: string, mode: EditorMode): Gate {
  return gate(lookUp('block', blockType), mode);
}

/** Whether the option `value` of the dropdown `field` of `blockType` is offered in its menu. */
export function optionGate(
  blockType: string,
  field: string,
  value: string,
  mode: EditorMode,
): Gate {
  const block = blockGate(blockType, mode);
  return block.allowed ? gate(lookUp('option', blockType, field, value), mode) : block;
}

/** Whether the toolbox category `category` (its catalog ID) is shown. */
export function categoryGate(category: string, mode: EditorMode): Gate {
  return gate(lookUp('category', category), mode);
}

/** Whether the type picker offers the type `typeKey` (a type field's value, 05 §5.4). */
export function typeGate(typeKey: string, mode: EditorMode): Gate {
  return gate(lookUp('type', typeKey), mode);
}

/** The GCC version each capability arrived with, for the messages. */
const CAPABILITY_TEXT: Readonly<Record<Capability, string>> = {
  stdFormat: 'a compiler with std::format (GCC 13 or newer)',
};

/** What a refusal means, for a tooltip or a note; `null` when it is allowed. */
export function gateText(result: Gate): string | null {
  if (result.allowed) {
    return null;
  }
  switch (result.reason) {
    case 'advanced':
      return 'An advanced block: turn on “Show advanced blocks” in Settings to use it.';
    case 'standard':
      return `Needs ${result.needs.replace('c++', 'C++')} or newer (the project's C++ standard).`;
    case 'toolchain':
      return `Needs ${CAPABILITY_TEXT[result.needs]}.`;
  }
}
