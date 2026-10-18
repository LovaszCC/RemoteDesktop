// Port of the Swift LocalizationCatalogTests on the generated catalogs (extras included).

import { describe, expect, it } from 'vitest';
import en from './en.json';
import extraEn from './extra.en.json';
import extraHu from './extra.hu.json';
import hu from './hu.json';
import { convertPrintf, type Entry } from './index.svelte';

const PRINTF = /%(?:\d+\$)?(?:ll|l|h)?[@dDuUxXfeEgGcs]/g;

/** `{0}`, `{1}`… sorted. */
function placeholders(text: string): string[] {
  return [...text.matchAll(/\{\d+\}/g)].map((m) => m[0]).sort();
}

function values(entry: Entry): string[] {
  return typeof entry === 'string'
    ? [entry]
    : Object.values(entry).filter((v): v is string => v !== undefined);
}

const enCatalog: Record<string, Entry> = en;
const huCatalog: Record<string, Entry> = hu;

describe('localization catalogs', () => {
  // en/hu.json carry the extras (scripts/merge-i18n-extras.mjs); an extra string missing there shows in English.
  it('contain every extra string (run scripts/merge-i18n-extras.mjs after editing extra.*.json)', () => {
    for (const [extra, catalog] of [
      [extraEn, enCatalog],
      [extraHu, huCatalog],
    ] as const) {
      const missing = Object.keys(extra).filter((key) => !(key in catalog));
      expect(missing).toEqual([]);
      for (const [key, value] of Object.entries(extra as Record<string, Entry>)) {
        if (typeof value === 'string') expect(catalog[key], key).toBe(convertPrintf(value));
      }
    }
  });

  it('are not empty and have the same keys', () => {
    expect(Object.keys(enCatalog).length).toBeGreaterThan(400);
    expect(Object.keys(huCatalog).sort()).toEqual(Object.keys(enCatalog).sort());
  });

  it('keep no printf placeholders in values', () => {
    for (const catalog of [enCatalog, huCatalog]) {
      for (const [key, entry] of Object.entries(catalog)) {
        for (const value of values(entry)) expect(value.match(PRINTF), key).toBeNull();
      }
    }
  });

  it('have a complete Hungarian translation with the same placeholders for every key', () => {
    for (const [key, entry] of Object.entries(enCatalog)) {
      const expected = placeholders(convertPrintf(key));
      const hungarian = huCatalog[key];
      expect(hungarian, `no Hungarian for ${key}`).toBeDefined();
      for (const value of values(hungarian!)) {
        expect(value.trim(), key).not.toBe('');
        expect(placeholders(value), `hu placeholders differ: ${key}`).toEqual(expected);
      }
      for (const value of values(entry)) {
        expect(placeholders(value), `en placeholders differ: ${key}`).toEqual(expected);
      }
    }
  });

  it('turn plural variations into one/other objects', () => {
    expect(enCatalog['%lld connections · %@']).toEqual({
      one: '{0} connection · {1}',
      other: '{0} connections · {1}',
    });
  });
});
