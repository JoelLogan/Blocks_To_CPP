/**
 * Small colour helpers for the theme: hex parsing, mixing and the WCAG 2.2 contrast ratio. Only
 * `#rrggbb` colours are used anywhere in the theme.
 */

const HEX_COLOUR = /^#[0-9a-f]{6}$/;

/** Whether `value` is a lower-case `#rrggbb` colour. */
export function isHexColour(value: string): boolean {
  return HEX_COLOUR.test(value);
}

function channels(colour: string): [number, number, number] {
  if (!isHexColour(colour)) {
    throw new RangeError(`Not a #rrggbb colour: ${colour.slice(0, 16)}`);
  }
  return [
    Number.parseInt(colour.slice(1, 3), 16),
    Number.parseInt(colour.slice(3, 5), 16),
    Number.parseInt(colour.slice(5, 7), 16),
  ];
}

function toHex(value: number): string {
  return Math.round(Math.min(255, Math.max(0, value)))
    .toString(16)
    .padStart(2, '0');
}

/** `colour` moved toward `target` by `amount` (0 keeps it, 1 gives `target`), per sRGB channel. */
export function mix(colour: string, target: string, amount: number): string {
  const from = channels(colour);
  const to = channels(target);
  return `#${from.map((channel, index) => toHex(channel + ((to[index] ?? 0) - channel) * amount)).join('')}`;
}

function linear(channel: number): number {
  const value = channel / 255;
  return value <= 0.04045 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
}

/** The relative luminance of a colour (WCAG 2.2). */
export function relativeLuminance(colour: string): number {
  const [red, green, blue] = channels(colour);
  return 0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue);
}

/** The WCAG 2.2 contrast ratio of two colours, from 1 to 21. */
export function contrastRatio(first: string, second: string): number {
  const a = relativeLuminance(first);
  const b = relativeLuminance(second);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}
