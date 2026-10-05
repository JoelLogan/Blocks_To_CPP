import { render } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { expectNoAxeViolations } from './axe';

/**
 * Attaches deliberately inaccessible markup. It is built with the DOM API so that the JSX
 * accessibility lint rules do not reject the test itself.
 */
function mountBroken(...children: HTMLElement[]): HTMLElement {
  const container = document.createElement('div');
  container.append(...children);
  document.body.append(container);
  return container;
}

/** An element of the given kind with the given attributes. */
function element(tag: string, attributes: Record<string, string> = {}): HTMLElement {
  const node = document.createElement(tag);
  for (const [name, value] of Object.entries(attributes)) {
    node.setAttribute(name, value);
  }
  return node;
}

afterEach(() => {
  document.body.replaceChildren();
});

describe('expectNoAxeViolations', () => {
  it('passes accessible markup', async () => {
    const { container } = render(
      <form aria-label="Rename">
        <label htmlFor="name">Name</label>
        <input id="name" />
        <button type="submit">Save</button>
      </form>,
    );
    await expect(expectNoAxeViolations(container)).resolves.toBeUndefined();
  });

  it('names each problem and where it is', async () => {
    const container = mountBroken(
      element('button', { type: 'button' }),
      element('input', { type: 'text' }),
    );

    const failure = expectNoAxeViolations(container);
    await expect(failure).rejects.toThrow(/button-name \(critical\)/);
    await expect(failure).rejects.toThrow(/button-name.*\n.*\n {4}at button\n/);
    await expect(failure).rejects.toThrow(/label \(critical\)/);
  });

  it('counts the problems', async () => {
    const container = mountBroken(element('img', { src: '/icon.png' }));
    await expect(expectNoAxeViolations(container)).rejects.toThrow(
      /found 1 accessibility problem\(s\):\n {2}image-alt \(critical\)/,
    );
  });

  it('skips the rules a test names', async () => {
    const container = mountBroken(element('button', { type: 'button' }));
    await expect(
      expectNoAxeViolations(container, { skipRules: ['button-name'] }),
    ).resolves.toBeUndefined();
  });
});
