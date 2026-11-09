import { describe, expect, it } from 'vitest';
import { safeEncodeURIComponent } from './encoding';

describe('safeEncodeURIComponent', () => {
  it.each([
    ['empty text', ''],
    ['URI punctuation', "a b%/?:#&=+!~*'()\n"],
    ['BMP text', '\u00E9\u4E2D\u0000\uFFFF'],
    ['lowest valid pair', '\uD800\uDC00'],
    ['highest valid pair', '\uDBFF\uDFFF'],
    ['adjacent valid pairs', '\uD800\uDC00\uDBFF\uDFFF'],
  ])('matches native URI encoding for %s', (_, input) => {
    expect(safeEncodeURIComponent(input)).toBe(encodeURIComponent(input));
    expect(decodeURIComponent(safeEncodeURIComponent(input))).toBe(input);
  });

  it.each([
    ['lone high surrogate', '\uD800', '\uFFFD'],
    ['lone low surrogate', '\uDC00', '\uFFFD'],
    ['high surrogate before BMP text', '\uDBFFx', '\uFFFDx'],
    ['low surrogate after BMP text', 'x\uDFFF', 'x\uFFFD'],
    ['consecutive high surrogates', '\uD800\uDBFF', '\uFFFD\uFFFD'],
    ['consecutive low surrogates', '\uDC00\uDFFF', '\uFFFD\uFFFD'],
    ['reversed pair', '\uDC00\uD800', '\uFFFD\uFFFD'],
    ['lone high before a valid pair', '\uD800\uD800\uDC00', '\uFFFD\uD800\uDC00'],
    ['lone low after a valid pair', '\uD800\uDC00\uDC00', '\uD800\uDC00\uFFFD'],
    ['mixed text', 'a\uDC00\uD840\uDC00\uD800z', 'a\uFFFD\uD840\uDC00\uFFFDz'],
  ])('replaces only unpaired surrogates in %s', (_, input, expected) => {
    expect(safeEncodeURIComponent(input)).toBe(encodeURIComponent(expected));
    expect(decodeURIComponent(safeEncodeURIComponent(input))).toBe(expected);
  });
});
