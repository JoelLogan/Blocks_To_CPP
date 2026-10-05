/**
 * Links a program prints with OSC 8 (docs/spec/04-user-interface.md §4.5, 08 §8.8). The program
 * controls them completely, so only `http` and `https` links are accepted, and in M2 activating
 * one only shows it, with a button to copy it; nothing is ever opened.
 */

/** The longest link accepted, in UTF-16 code units. */
export const MAX_LINK_LENGTH = 8192;

/**
 * The normalised `http` or `https` URL for a link's target, or `null` when it is anything else,
 * too long or not a URL. The WHATWG parser's form is what is shown and copied: it is what a browser
 * would open, with an internationalised host name in its ASCII (punycode) form so look-alike
 * letters cannot hide the real host.
 */
export function safeLinkUrl(target: string): string | null {
  if (target.length === 0 || target.length > MAX_LINK_LENGTH) {
    return null;
  }
  let url: URL;
  try {
    url = new URL(target);
  } catch {
    return null;
  }
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    return null;
  }
  if (url.hostname === '') {
    return null;
  }
  return url.href.length > MAX_LINK_LENGTH ? null : url.href;
}
