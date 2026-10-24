/**
 * The explicit limits of the document sync, from the project format (docs/spec/05-project-format.md
 * §5.6) and the editor's own bounds.
 */

/** The largest absolute canvas coordinate a project stores (05 §5.6, `MAX_COORDINATE`). */
export const MAX_COORDINATE = 10_000_000;

/** The smallest zoom a project stores (`viewport.scale`, 05 §5.3). */
export const MIN_ZOOM = 0.1;

/** The largest zoom a project stores (`viewport.scale`, 05 §5.3). */
export const MAX_ZOOM = 4;

/** How long the editor waits after the last change before it reads the document (02 §2.4.1). */
export const SYNC_DEBOUNCE_MS = 50;

/**
 * The most remembered ID replacements of duplicated blocks (so that redoing a duplicate gives the
 * same IDs again). The memory is emptied when it is full, and when a document is loaded.
 */
export const MAX_REMAP_MEMO = 100_000;

/**
 * A canvas coordinate as a project stores it: a whole number within ±{@link MAX_COORDINATE}.
 * Anything that is not a finite number becomes 0.
 */
export function clampCoordinate(value: number): number {
  if (!Number.isFinite(value)) {
    return 0;
  }
  const rounded = Math.round(value);
  // `+ 0` turns -0 into 0, which JSON writes the same way but `Object.is` does not.
  return Math.min(MAX_COORDINATE, Math.max(-MAX_COORDINATE, rounded)) + 0;
}

/** A zoom as a project stores it: within {@link MIN_ZOOM} to {@link MAX_ZOOM}; 1 when unusable. */
export function clampZoom(value: number): number {
  if (!Number.isFinite(value) || value <= 0) {
    return 1;
  }
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, value));
}
