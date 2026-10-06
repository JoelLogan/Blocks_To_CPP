/** Reading the flood protection's "… N lines skipped" markers from a console transcript. */
import { describe, expect, it } from 'vitest';

import { skippedCounts } from './console';

describe('skippedCounts', () => {
  it('reads every marker, with its thousands separators, in order', () => {
    const transcript = [
      'line',
      ' … 1,204,331 lines skipped ',
      'line',
      ' … 1 line skipped ',
      ' … output skipped ',
      ' … 98283 lines skipped ',
    ].join('\n');
    expect(skippedCounts(transcript)).toEqual([1_204_331, 1, 0, 98_283]);
  });

  it('finds nothing in ordinary output', () => {
    expect(skippedCounts('B2C flood line\nlines skipped\n… lines skipped')).toEqual([]);
  });
});
