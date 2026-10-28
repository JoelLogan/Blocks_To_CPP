/**
 * The scorer of the block search (Quick Insert and toolbox search, docs/plans/m3-contracts/
 * w4-editor-tools.md §2.3): small, with no dependency. Case-insensitive; each word of the query
 * must match one text of an entry: a whole word scores most, then the start of a word, then
 * anywhere in a word, then the letters of one word in order with gaps (a few gaps cost points).
 * Pure.
 */

/** How well one query word matches one text, 0 for not at all. */
export function wordScore(word: string, text: string): number {
  if (word === '' || text === '') {
    return 0;
  }
  const words = text.split(/[^\p{L}\p{N}_+\-*/%<>=!&|^:.]+/u).filter((part) => part !== '');
  let best = 0;
  for (const part of words) {
    if (part === word) {
      return 100;
    }
    if (part.startsWith(word)) {
      best = Math.max(best, 80 - Math.min(20, part.length - word.length));
    } else if (part.includes(word)) {
      best = Math.max(best, 50);
    }
  }
  if (best > 0) {
    return best;
  }
  if (text.includes(word)) {
    return 40;
  }
  if (word.length < 3) {
    return 0;
  }
  // The letters in order inside one word of the text (`prnt` finds *print*).
  return Math.max(0, ...words.map((part) => subsequenceScore(word, part)));
}

/** The letters of `word` in order in `text`: 30 less the gaps, at least 1; 0 when they are not. */
function subsequenceScore(word: string, text: string): number {
  let at = -1;
  let gaps = 0;
  for (const char of word) {
    const next = text.indexOf(char, at + 1);
    if (next < 0) {
      return 0;
    }
    if (at >= 0 && next > at + 1) {
      gaps += 1;
    }
    at = next;
  }
  return Math.max(1, 30 - 6 * gaps);
}

/** One text of an entry and how much a match in it counts (1 for the label). */
export interface WeightedText {
  readonly text: string;
  readonly weight: number;
}

/** The lower-case words of a query, at most eight. */
export function queryWords(query: string): string[] {
  return query
    .toLowerCase()
    .split(/\s+/u)
    .filter((word) => word !== '')
    .slice(0, 8);
}

/**
 * How well `words` match an entry's texts (lower case): the sum, over the words, of each word's
 * best weighted match; 0 when any word matches no text.
 */
export function entryScore(words: readonly string[], texts: readonly WeightedText[]): number {
  let total = 0;
  for (const word of words) {
    let best = 0;
    for (const { text, weight } of texts) {
      best = Math.max(best, wordScore(word, text) * weight);
    }
    if (best === 0) {
      return 0;
    }
    total += best;
  }
  return total;
}
