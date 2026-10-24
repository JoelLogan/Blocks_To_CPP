/** The Trusted Types trial's collector counts violations and keeps directive names only. */
import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  directiveOf,
  MAX_DIRECTIVES,
  OTHER_DIRECTIVE,
  startTrustedTypesCollector,
  type TrustedTypesCollector,
} from './trustedTypes';

/** A `securitypolicyviolation` event with the given fields (happy-dom has no such event class). */
function violation(fields: {
  effectiveDirective?: unknown;
  violatedDirective?: unknown;
  disposition?: unknown;
  sample?: string;
}): Event {
  const event = new Event('securitypolicyviolation', { bubbles: true, composed: true });
  Object.assign(event, fields);
  return event;
}

const collectors: TrustedTypesCollector[] = [];

function collect(target: EventTarget, onNewDirective = vi.fn()): TrustedTypesCollector {
  const collector = startTrustedTypesCollector(target, { onNewDirective });
  collectors.push(collector);
  return collector;
}

afterEach(() => {
  for (const collector of collectors.splice(0)) {
    collector.stop();
  }
});

describe('directiveOf', () => {
  it('takes the effective directive, else the violated one without its value', () => {
    expect(directiveOf({ effectiveDirective: 'require-trusted-types-for' })).toBe(
      'require-trusted-types-for',
    );
    expect(directiveOf({ effectiveDirective: '', violatedDirective: "script-src 'self'" })).toBe(
      'script-src',
    );
    expect(directiveOf({ violatedDirective: 'Style-Src-Elem' })).toBe('style-src-elem');
  });

  it('never keeps anything that does not look like a directive name', () => {
    expect(directiveOf({})).toBe(OTHER_DIRECTIVE);
    expect(directiveOf({ effectiveDirective: 42 })).toBe(OTHER_DIRECTIVE);
    expect(directiveOf({ effectiveDirective: '<img src=x onerror=alert(1)>' })).toBe(
      OTHER_DIRECTIVE,
    );
    expect(directiveOf({ effectiveDirective: 'a'.repeat(65) })).toBe(OTHER_DIRECTIVE);
    expect(directiveOf({ effectiveDirective: '1script' })).toBe(OTHER_DIRECTIVE);
  });
});

describe('startTrustedTypesCollector', () => {
  it('counts violations and lists each directive once, sorted, with no sample text', () => {
    const target = new EventTarget();
    const onNew = vi.fn();
    const collector = collect(target, onNew);
    expect(collector.report()).toEqual({ count: 0, directives: [] });

    target.dispatchEvent(
      violation({
        effectiveDirective: 'require-trusted-types-for',
        disposition: 'report',
        sample: 'secret project text',
      }),
    );
    target.dispatchEvent(violation({ effectiveDirective: 'require-trusted-types-for' }));
    target.dispatchEvent(violation({ effectiveDirective: 'img-src', disposition: 'enforce' }));

    const report = collector.report();
    expect(report).toEqual({ count: 3, directives: ['img-src', 'require-trusted-types-for'] });
    expect(JSON.stringify(report)).not.toContain('secret');
    expect(onNew.mock.calls).toEqual([
      ['require-trusted-types-for', 'report'],
      ['img-src', 'enforce'],
    ]);
  });

  it('sees violations fired at elements inside the document (capture phase)', () => {
    const element = document.createElement('div');
    document.body.append(element);
    const collector = collect(document);
    element.dispatchEvent(violation({ effectiveDirective: 'script-src-elem' }));
    expect(collector.report().directives).toEqual(['script-src-elem']);
    element.remove();
  });

  it('remembers at most MAX_DIRECTIVES directives but counts every violation', () => {
    const target = new EventTarget();
    const collector = collect(target);
    for (let index = 0; index < MAX_DIRECTIVES + 5; index += 1) {
      target.dispatchEvent(violation({ effectiveDirective: `directive-${'x'.repeat(index + 1)}` }));
    }
    expect(collector.report().count).toBe(MAX_DIRECTIVES + 5);
    expect(collector.report().directives).toHaveLength(MAX_DIRECTIVES);
  });

  it('keeps counting when the reporter throws, and stops listening when stopped', () => {
    const target = new EventTarget();
    const collector = collect(
      target,
      vi.fn(() => {
        throw new Error('broken reporter');
      }),
    );
    target.dispatchEvent(violation({ effectiveDirective: 'img-src' }));
    expect(collector.report().count).toBe(1);
    collector.stop();
    collector.stop();
    target.dispatchEvent(violation({ effectiveDirective: 'img-src' }));
    expect(collector.report()).toEqual({ count: 1, directives: ['img-src'] });
  });

  it('logs a new directive on the console by default, without the sample', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const target = new EventTarget();
    const collector = startTrustedTypesCollector(target);
    collectors.push(collector);
    target.dispatchEvent(
      violation({
        effectiveDirective: 'require-trusted-types-for',
        disposition: 'report',
        sample: 'x=1',
      }),
    );
    target.dispatchEvent(violation({ effectiveDirective: 'img-src' }));
    expect(warn.mock.calls).toEqual([
      ['Content Security Policy violation (reported only): require-trusted-types-for'],
      ['Content Security Policy violation (blocked): img-src'],
    ]);
  });
});
