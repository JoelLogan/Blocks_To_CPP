/**
 * The preview's compact source map (`CompactSourceMap`, version 2; M3-5): reading it back into the
 * IR's form (`SourceMap`, version 1).
 */
import type { FileMap, MappedRange, Part, SourceMap } from '@blocks2cpp/ipc-types';

import type { CompactSourceMap } from './api';

/** Numbers per range of a compact map. */
export const COMPACT_RANGE_STRIDE = 7;

/**
 * The IR's form of a compact source map, for tests and for any reader that wants it. A range whose
 * module, block or part index is out of range is left out, as is a file's last incomplete range.
 */
export function expandSourceMap(map: CompactSourceMap): SourceMap {
  const files: FileMap[] = map.files.map((file) => {
    const ranges: MappedRange[] = [];
    for (let at = 0; at + COMPACT_RANGE_STRIDE <= file.ranges.length; at += COMPACT_RANGE_STRIDE) {
      const [startLine, startColumn, endLine, endColumn, module, block, part] = file.ranges.slice(
        at,
        at + COMPACT_RANGE_STRIDE,
      );
      const moduleId = map.modules[module ?? -1];
      const blockId = map.blocks[block ?? -1];
      const partOf = map.parts[part ?? -1];
      if (moduleId === undefined || blockId === undefined || partOf === undefined) {
        continue;
      }
      ranges.push({
        start: { line: startLine ?? 0, column: startColumn ?? 0 },
        end: { line: endLine ?? 0, column: endColumn ?? 0 },
        module: moduleId,
        block: blockId,
        part: partOf,
      });
    }
    return file.module === undefined
      ? { path: file.path, ranges }
      : { path: file.path, module: file.module, ranges };
  });
  return { version: 1, files };
}

/**
 * The compact form of a source map in the IR's form, as the core writes it (`compact.rs`): each
 * module ID, block ID and part once, in first use. For tests and fixtures; the core makes the
 * preview's own.
 */
export function compactSourceMap(map: SourceMap): CompactSourceMap {
  const modules: string[] = [];
  const blocks: string[] = [];
  const parts: Part[] = [];
  const keys = new Map<string, number>();
  const intern = <T>(list: T[], kind: string, value: T): number => {
    const key = `${kind}\0${JSON.stringify(value)}`;
    let index = keys.get(key);
    if (index === undefined) {
      index = list.length;
      list.push(value);
      keys.set(key, index);
    }
    return index;
  };
  const files = map.files.map((file) => {
    const ranges = file.ranges.flatMap((range) => [
      range.start.line,
      range.start.column,
      range.end.line,
      range.end.column,
      intern(modules, 'module', range.module),
      intern(blocks, 'block', range.block),
      intern(parts, 'part', range.part),
    ]);
    return file.module === undefined
      ? { path: file.path, ranges }
      : { path: file.path, module: file.module, ranges };
  });
  return { version: 2, modules, blocks, parts, files };
}
