/**
 * A catalog block's own project data (docs/spec/05-project-format.md §5.4): its fields, its `extra`
 * (mutator state), its comment and its flags, read from and written to a Blockly block. Nested
 * blocks are not handled here.
 */
import type { BdmBlock, BdmComment, FieldValue, JsonValue } from '@blocks2cpp/b2c-core-wasm';
import {
  type BlockDefJson,
  hasB2cMutator,
  isManuallyDisabled,
  readB2cField,
  setManuallyDisabled,
  writeB2cField,
} from '@blocks2cpp/blockly-ext';
import * as Blockly from 'blockly/core';

import { stableJson } from './bdmTree';
import { extraDef, extraDefault, fieldDef, fieldDefault } from './catalog';
import { type BlockOrigin, originOf } from './origin';

/**
 * The block's fields as a project stores them. A field without a value (a reference with nothing
 * chosen) is left out, and so is a field the block's file left out while it still has its default.
 */
export function readFields(
  block: Blockly.Block,
  def: BlockDefJson,
  origin: BlockOrigin | null,
): Record<string, FieldValue> | undefined {
  const fields: Record<string, FieldValue> = {};
  let any = false;
  for (const definition of def.fields) {
    const field = block.getField(definition.name);
    if (field === null) {
      continue;
    }
    const value = readB2cField(field);
    if (value === null) {
      continue;
    }
    if (
      origin !== null &&
      !origin.fields.has(definition.name) &&
      stableJson(value) === stableJson(fieldDefault(definition))
    ) {
      continue;
    }
    fields[definition.name] = value;
    any = true;
  }
  return any ? fields : undefined;
}

/**
 * The block's `extra` as a project stores it (counts, flags, `params`), or `undefined` for a block
 * without a mutator. A key the block's file left out is left out while it has its default.
 */
export function readExtra(
  block: Blockly.Block,
  def: BlockDefJson,
  origin: BlockOrigin | null,
): Record<string, JsonValue> | undefined {
  if (!hasB2cMutator(block)) {
    return undefined;
  }
  const extra: Record<string, JsonValue> = {};
  let any = false;
  for (const [key, value] of Object.entries(block.b2cGetExtra())) {
    if (origin !== null && !origin.extra.has(key)) {
      const definition = extraDef(def, key);
      if (definition !== null && stableJson(value) === stableJson(extraDefault(definition))) {
        continue;
      }
    }
    extra[key] = value as JsonValue;
    any = true;
  }
  return any ? extra : undefined;
}

/** The block's comment (05 §5.4: `{text, pinned}`, `pinned` only when the bubble is open). */
export function readComment(block: Blockly.Block): BdmComment | undefined {
  const text = block.getCommentText();
  if (text === null) {
    return undefined;
  }
  const icon = block.getIcon(Blockly.icons.IconType.COMMENT);
  return icon?.bubbleIsVisible() === true ? { text, pinned: true } : { text };
}

/** Gives the block the comment of a project, if it has one. */
export function writeComment(block: Blockly.Block, comment: BdmComment | undefined): void {
  if (comment === undefined) {
    return;
  }
  block.setCommentText(comment.text);
  if (comment.pinned === true) {
    const icon = block.getIcon(Blockly.icons.IconType.COMMENT);
    // The flag is set at once; showing the bubble waits for rendering.
    icon?.setBubbleVisible(true).catch((error: unknown) => {
      console.warn('A pinned comment could not be shown', error);
    });
  }
}

/** Whether the user disabled the block (only that reason is saved, M2 decision). */
export function readDisabled(block: Blockly.Block): boolean {
  return isManuallyDisabled(block);
}

/**
 * Gives a new catalog block the `extra` and the fields of `node`. False when the block cannot hold
 * them: a field or an `extra` value the block cannot show, or a key the block does not have. The
 * block may then be partly changed; the caller replaces it.
 */
export function writeOwnData(block: Blockly.Block, def: BlockDefJson, node: BdmBlock): boolean {
  const extra = node.extra ?? {};
  if (Object.keys(extra).length > 0) {
    if (!hasB2cMutator(block)) {
      return false;
    }
    try {
      block.b2cSetExtra(extra);
    } catch {
      // MutatorStateError: a value the block cannot show.
      return false;
    }
  }
  for (const [name, value] of Object.entries(node.fields ?? {})) {
    const field = fieldDef(def, name) === null ? null : block.getField(name);
    if (field === null || !writeB2cField(field, value)) {
      return false;
    }
  }
  return true;
}

/**
 * Whether the block reads back exactly the fields, `extra` and comment of `node`, so that saving it
 * changes nothing. A block that does not (a value a field would normalise, for example) is kept
 * as a placeholder instead.
 */
export function readsBackAs(block: Blockly.Block, def: BlockDefJson, node: BdmBlock): boolean {
  const origin = originOf(block);
  return (
    stableJson(readFields(block, def, origin) ?? {}) === stableJson(node.fields ?? {}) &&
    stableJson(readExtra(block, def, origin) ?? {}) === stableJson(node.extra ?? {}) &&
    stableJson(readComment(block) ?? null) === stableJson(node.comment ?? null)
  );
}

/** Gives a new block the flags and the comment of `node` (not `collapsed`, set after loading). */
export function writeFlagsAndComment(block: Blockly.Block, node: BdmBlock): void {
  if (node.disabled === true) {
    setManuallyDisabled(block, true);
  }
  writeComment(block, node.comment);
}
