import { afterEach, describe, expect, it } from 'vitest';

import { focusIfLost, focusIsLost } from './focus';

afterEach(() => {
  document.body.replaceChildren();
});

function button(text: string, parent: HTMLElement = document.body): HTMLButtonElement {
  const element = document.createElement('button');
  element.textContent = text;
  parent.append(element);
  return element;
}

describe('focusIsLost', () => {
  it('is true on the body', () => {
    expect(document.activeElement).toBe(document.body);
    expect(focusIsLost()).toBe(true);
  });

  it('is false on a control that is shown', () => {
    button('Run').focus();
    expect(focusIsLost()).toBe(false);
  });

  it('is true inside something hidden', () => {
    const page = document.createElement('div');
    document.body.append(page);
    const back = button('Back to the project', page);
    back.focus();
    page.hidden = true;
    expect(document.activeElement).toBe(back);
    expect(focusIsLost()).toBe(true);
  });
});

describe('focusIfLost', () => {
  it('moves a lost focus to the target', () => {
    const heading = document.createElement('h2');
    heading.tabIndex = -1;
    document.body.append(heading);
    expect(focusIfLost(heading)).toBe(true);
    expect(document.activeElement).toBe(heading);
  });

  it('leaves a focus that is somewhere visible alone', () => {
    const run = button('Run');
    run.focus();
    const heading = document.createElement('h2');
    heading.tabIndex = -1;
    document.body.append(heading);
    expect(focusIfLost(heading)).toBe(false);
    expect(document.activeElement).toBe(run);
  });

  it('moves a lost focus to an SVG target, such as a block of the canvas', () => {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    const block = document.createElementNS('http://www.w3.org/2000/svg', 'g');
    block.setAttribute('tabindex', '-1');
    svg.append(block);
    document.body.append(svg);
    expect(focusIfLost(block)).toBe(true);
    expect(document.activeElement).toBe(block);
  });

  it('does nothing without a target', () => {
    expect(focusIfLost(null)).toBe(false);
    expect(document.activeElement).toBe(document.body);
  });
});
