import { act, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { createBannerRegistry, WindowBanners } from './banners';

function First() {
  return <p>first</p>;
}
function Second() {
  return <p>second</p>;
}
function Nothing() {
  return null;
}

describe('the banner registry', () => {
  it('orders banners by order, then by registration, one per ID', () => {
    const registry = createBannerRegistry();
    const removeSecond = registry.registerBanner('second', Second, { order: 2 });
    registry.registerBanner('first', First, { order: 1 });
    registry.registerBanner('also-first', Nothing, { order: 1 });
    expect(registry.banners().map((banner) => banner.id)).toEqual([
      'first',
      'also-first',
      'second',
    ]);

    const replace = registry.registerBanner('first', Nothing, { order: 1 });
    expect(registry.banners().find((banner) => banner.id === 'first')?.component).toBe(Nothing);
    replace();
    replace();
    expect(registry.banners().find((banner) => banner.id === 'first')?.component).toBe(First);

    removeSecond();
    expect(registry.banners().map((banner) => banner.id)).toEqual(['first', 'also-first']);
  });

  it('refuses an order that is not a finite number', () => {
    const registry = createBannerRegistry();
    expect(() => registry.registerBanner('bad', First, { order: Number.NaN })).toThrow(RangeError);
    expect(registry.banners()).toEqual([]);
  });

  it('renders the banners in a polite live region and follows registrations', () => {
    const registry = createBannerRegistry();
    render(<WindowBanners registry={registry} />);
    const region = screen.getByTestId('window-banners');
    expect(region.getAttribute('aria-live')).toBe('polite');
    expect(region.textContent).toBe('');

    let remove: () => void = () => undefined;
    act(() => {
      remove = registry.registerBanner('second', Second, { order: 2 });
      registry.registerBanner('first', First);
    });
    expect(region.textContent).toBe('firstsecond');

    act(() => {
      remove();
    });
    expect(region.textContent).toBe('first');
  });
});
