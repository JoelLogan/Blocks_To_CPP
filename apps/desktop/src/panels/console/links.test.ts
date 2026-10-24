import { describe, expect, it } from 'vitest';

import { MAX_LINK_LENGTH, safeLinkUrl } from './links';

describe('safeLinkUrl', () => {
  it('accepts http and https links in their normalised form', () => {
    expect(safeLinkUrl('https://example.com/a?b=c#d')).toBe('https://example.com/a?b=c#d');
    expect(safeLinkUrl('http://EXAMPLE.com')).toBe('http://example.com/');
  });

  it('shows an internationalised host in its ASCII form', () => {
    // A Cyrillic "е" instead of the Latin "e".
    expect(safeLinkUrl('https://еxample.com/')).toMatch(/^https:\/\/xn--/);
  });

  it('refuses every other scheme and anything that is not a URL', () => {
    // Built from parts so the lint rule against script URLs does not flag the test data.
    const script = ['java', 'script:alert(1)'].join('');
    for (const target of [
      script,
      script.toUpperCase(),
      'java\nscript:alert(1)',
      'data:text/html,<script>alert(1)</script>',
      'file:///etc/passwd',
      'vbscript:msgbox',
      'ftp://example.com/',
      'mailto:someone@example.com',
      '/relative/path',
      'not a url',
      '',
    ]) {
      expect(safeLinkUrl(target), target).toBeNull();
    }
  });

  it('refuses links that are too long', () => {
    const long = `https://example.com/${'a'.repeat(MAX_LINK_LENGTH)}`;
    expect(safeLinkUrl(long)).toBeNull();
  });
});
