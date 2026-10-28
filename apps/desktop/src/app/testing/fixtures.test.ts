import { describe, expect, it } from 'vitest';

import { documentFixture } from './fixtures';

describe('documentFixture', () => {
  it('is a format 1 document without Textbook style', () => {
    const document = documentFixture();
    expect(document.formatVersion).toBe(1);
    expect(document.project.options.usingNamespaceStd).toBeUndefined();
    expect(document.project.options.checkedIndexing).toBe(true);
  });

  it('turns Textbook style on as a format 2 document', () => {
    const document = documentFixture('Textbook', { usingNamespaceStd: true });
    expect(document.project.name).toBe('Textbook');
    expect(document.formatVersion).toBe(2);
    expect(document.project.options).toMatchObject({
      usingNamespaceStd: true,
      checkedIndexing: true,
    });
  });
});
