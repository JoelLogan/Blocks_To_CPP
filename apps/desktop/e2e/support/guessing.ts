/**
 * The guessing game's side of the exit test (docs/spec/03-block-language.md §3.13.1): reading the
 * program's answers from the console transcript, guessing by binary search, and checking the
 * transcript against the golden output's shape (tests/golden/guessing_game/stdout.regex).
 */

/** What the game says about a guess. */
export type Answer = 'low' | 'high' | 'correct';

/** The game's first line. */
export const INTRO = 'Guess a number from 1 to 100!';
/** The game's prompt. */
export const PROMPT = 'Your guess: ';
/** The game's answers, as printed. */
export const ANSWER_TEXT: Readonly<Record<Answer, string>> = {
  low: 'Too low!',
  high: 'Too high!',
  correct: 'Correct!',
};

/** The secret's range and the most guesses a binary search over it needs (⌈log₂ 100⌉ = 7). */
export const LOWEST = 1;
export const HIGHEST = 100;
export const MAX_GUESSES = 7;

const ANSWER_PATTERN = /Too low!|Too high!|Correct!/g;

/** The answers in a transcript, in order. Line breaks are not relied on. */
export function answersIn(transcript: string): Answer[] {
  const answers: Answer[] = [];
  for (const match of transcript.matchAll(ANSWER_PATTERN)) {
    answers.push(match[0] === 'Too low!' ? 'low' : match[0] === 'Too high!' ? 'high' : 'correct');
  }
  return answers;
}

/** How many prompts a transcript has. */
export function promptCount(transcript: string): number {
  return transcript.split(PROMPT).length - 1;
}

/** A binary search for the secret from the answers so far. */
export class BinarySearch {
  #low = LOWEST;
  #high = HIGHEST;
  readonly #guesses: number[] = [];
  #found = false;

  /** The guesses made so far. */
  get guesses(): readonly number[] {
    return this.#guesses;
  }

  /** Whether the last answer was *Correct!*. */
  get found(): boolean {
    return this.#found;
  }

  /** The next guess (the middle of what is left). */
  next(): number {
    if (this.#found) {
      throw new Error('The secret was already found');
    }
    if (this.#low > this.#high) {
      throw new Error(`The answers contradict each other after ${this.#guesses.join(', ')}`);
    }
    const guess = Math.floor((this.#low + this.#high) / 2);
    this.#guesses.push(guess);
    return guess;
  }

  /** Narrows the range with the answer to the last guess. */
  answer(answer: Answer): void {
    const last = this.#guesses.at(-1);
    if (last === undefined) {
      throw new Error('There was no guess to answer');
    }
    switch (answer) {
      case 'low':
        this.#low = last + 1;
        break;
      case 'high':
        this.#high = last - 1;
        break;
      case 'correct':
        this.#found = true;
        break;
    }
  }
}

/**
 * The golden output's shape as a regular expression for the whole transcript. The golden run's
 * input only ever guesses too low, so its pattern has only *Too low!*; a binary search also hears
 * *Too high!*, so either answer is accepted there.
 */
export function goldenShape(regexSource: string): RegExp {
  const source = regexSource.trim();
  if (!source.includes('Too low!')) {
    throw new Error('The golden pattern no longer has the "Too low!" answer; update the oracle');
  }
  return new RegExp(`^${source.replace('Too low!', 'Too (?:low|high)!')}$`);
}

/**
 * The transcript as the program's standard output would be: the console echoes what was typed
 * after each prompt (and the Enter as a line break), which the golden output, made with input from
 * a file, does not have.
 */
export function withoutEcho(transcript: string): string {
  return transcript.replace(/Your guess: [0-9]*\n/g, PROMPT);
}

/**
 * The game's lines in order (intro, prompts and answers), for a check that does not rely on line
 * breaks: Windows' pseudoconsole may move the cursor instead of writing them.
 */
export function gameTokens(transcript: string): string[] {
  const tokens = /Guess a number from 1 to 100!|Your guess: |Too low!|Too high!|Correct!/g;
  return Array.from(transcript.matchAll(tokens), (match) => match[0]);
}
