/** The limits a person can reach by editing, and what Save and the banner say about them. */
import { describe, expect, it } from 'vitest';

import { LIMIT_CODES, limitMessage, reachedLimit } from './limits';

describe('reachable limits', () => {
  it('are found among the loader’s codes, the first limit winning', () => {
    expect(reachedLimit(['B2C-E0104'])).toBe('nesting');
    expect(reachedLimit(['B2C-E0101'])).toBe('size');
    expect(reachedLimit(['B2C-E0121'])).toBe('blocks');
    expect(reachedLimit(['B2C-E0110', 'B2C-E0121', 'B2C-E0104'])).toBe('nesting');
    // Anything else in the editor's own document is a bug, not a limit.
    expect(reachedLimit(['B2C-E0110', 'B2C-E0114'])).toBeNull();
    expect(reachedLimit([])).toBeNull();
  });

  it('are named with their code and how to get back within them', () => {
    for (const limit of ['nesting', 'size', 'blocks'] as const) {
      const message = limitMessage(limit);
      expect(message).toContain(`(${LIMIT_CODES[limit]})`);
      expect(message).toContain('Undo the last change (Ctrl+Z)');
      expect(message).not.toContain('bug');
    }
    expect(limitMessage('size')).toContain('32 MiB');
    expect(limitMessage('blocks')).toContain('100,000 blocks');
    expect(limitMessage('nesting')).toContain('into a function');
  });
});
