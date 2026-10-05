/** The entry point (src/main.tsx) mounts the app into index.html's root element. */
import { act, screen } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('../app/App', () => ({
  App: () => <p>the app</p>,
}));

beforeEach(() => {
  // Each test imports the entry point afresh, as a page load would.
  vi.resetModules();
});

afterEach(() => {
  document.getElementById('root')?.remove();
});

describe('main', () => {
  it('renders the app into the element with the id "root"', async () => {
    const root = document.createElement('div');
    root.id = 'root';
    document.body.append(root);

    await act(async () => {
      await import('../main');
    });

    expect(root.contains(screen.getByText('the app'))).toBe(true);
  });

  it('stops with a clear error when index.html has no root element', async () => {
    await expect(import('../main')).rejects.toThrow('index.html has no element with the id "root"');
  });
});
