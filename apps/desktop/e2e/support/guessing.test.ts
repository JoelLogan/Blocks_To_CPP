import { readFileSync } from 'node:fs';
import path from 'node:path';

import { describe, expect, it } from 'vitest';

import { REPOSITORY_ROOT } from './env';
import {
  answersIn,
  BinarySearch,
  gameTokens,
  goldenShape,
  HIGHEST,
  LOWEST,
  MAX_GUESSES,
  promptCount,
  withoutEcho,
} from './guessing';

/** Plays the game for `secret` as the test does; returns the guesses. */
function play(secret: number): readonly number[] {
  const search = new BinarySearch();
  while (!search.found) {
    const guess = search.next();
    search.answer(guess < secret ? 'low' : guess > secret ? 'high' : 'correct');
  }
  return search.guesses;
}

describe('BinarySearch', () => {
  it('finds every secret from 1 to 100 within 7 guesses', () => {
    for (let secret = LOWEST; secret <= HIGHEST; secret += 1) {
      const guesses = play(secret);
      expect(guesses.at(-1)).toBe(secret);
      expect(guesses.length).toBeLessThanOrEqual(MAX_GUESSES);
    }
  });

  it('starts in the middle and refuses to go on after the secret is found', () => {
    const search = new BinarySearch();
    expect(search.next()).toBe(50);
    search.answer('correct');
    expect(search.found).toBe(true);
    expect(() => search.next()).toThrow('already found');
  });

  it('notices answers that contradict each other', () => {
    const search = new BinarySearch();
    search.next(); // 50
    search.answer('low');
    search.next(); // 75
    search.answer('high');
    for (let guess = search.next(); guess !== 51; guess = search.next()) {
      search.answer('high');
    }
    search.answer('high');
    expect(() => search.next()).toThrow('contradict');
  });

  it('needs a guess before an answer', () => {
    expect(() => {
      new BinarySearch().answer('low');
    }).toThrow('no guess');
  });
});

describe('reading the transcript', () => {
  const transcript =
    'Guess a number from 1 to 100!\nYour guess: 50\nToo low!\nYour guess: 75\nToo high!\nYour guess: 62\nCorrect!\n';

  it('finds the answers and prompts in order', () => {
    expect(answersIn(transcript)).toEqual(['low', 'high', 'correct']);
    expect(promptCount(transcript)).toBe(3);
    expect(answersIn('Your guess: ')).toEqual([]);
  });

  it('does not need line breaks', () => {
    expect(answersIn('Too low!Too high!Correct!')).toEqual(['low', 'high', 'correct']);
    expect(gameTokens('Guess a number from 1 to 100!Your guess: 5Too low!')).toEqual([
      'Guess a number from 1 to 100!',
      'Your guess: ',
      'Too low!',
    ]);
  });

  it('finds a prompt whose space a repaint dropped (Windows)', () => {
    expect(gameTokens('Too high!\nYour guess:69\nCorrect!')).toEqual([
      'Too high!',
      'Your guess: ',
      'Correct!',
    ]);
    expect(promptCount('Your guess: 50\nToo low!\nYour guess:69')).toBe(2);
  });

  it('matches the golden output shape once the echo is removed', () => {
    const source = readFileSync(
      path.join(REPOSITORY_ROOT, 'tests', 'golden', 'guessing_game', 'stdout.regex'),
      'utf8',
    );
    const shape = goldenShape(source);
    expect(withoutEcho(transcript)).toMatch(shape);
    // The golden run's own output (only "Too low!") still matches.
    expect('Guess a number from 1 to 100!\nYour guess: Too low!\nYour guess: Correct!\n').toMatch(
      shape,
    );
    // A missing answer or extra text does not.
    expect('Guess a number from 1 to 100!\nYour guess: \n').not.toMatch(shape);
    expect(`${withoutEcho(transcript)}extra`).not.toMatch(shape);
    expect(transcript).not.toMatch(shape);
  });

  it('refuses a golden pattern it cannot widen', () => {
    expect(() => goldenShape('Correct!\\n')).toThrow('update the oracle');
  });
});
