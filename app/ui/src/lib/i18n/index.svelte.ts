// The UI's strings. Keys are the English source strings of the Swift catalogs (printf placeholders
// included); values use `{0}`, `{1}`… and plural keys hold `{one, other}` objects. UI-only strings live in
// `extra.{en,hu}.json` and are merged in by `scripts/merge-i18n-extras.mjs`.
//
// `t` and `tn` read the reactive locale, so every template using them re-renders on `setLocale`.

import en from './en.json';
import hu from './hu.json';

export type Locale = 'en' | 'hu';
export type Entry = string | Partial<Record<Intl.LDMLPluralRule, string>>;
export type Catalog = Record<string, Entry>;

export const LOCALES: readonly Locale[] = ['en', 'hu'];
export const catalogs: Record<Locale, Catalog> = { en, hu };

/** The placeholder regex of the Swift LocalizationCatalogTests, plus `%%`. */
const PRINTF = /%%|%(?:(\d+)\$)?(?:ll|l|h)?[@dDuUxXfeEgGcs]/g;
const NUMERIC = /[dDuUxX]$/;

let current = $state<Locale>('en');

export const i18n = {
  get locale(): Locale {
    return current;
  },
};

export function isLocale(value: unknown): value is Locale {
  return value === 'en' || value === 'hu';
}

/** Switches the language live; unknown values fall back to English. */
export function setLocale(locale: string) {
  current = isLocale(locale) ? locale : 'en';
  if (typeof document !== 'undefined') document.documentElement.lang = current;
}

/** Whether `key` is in the catalog. */
export function has(key: string): boolean {
  return key in catalogs.en;
}

/** `%@ of %lld` → `{0} of {1}` (the fallback for keys missing from the catalog). */
export function convertPrintf(text: string): string {
  let next = 0;
  return text.replace(PRINTF, (match, position: string | undefined) =>
    match === '%%' ? '%' : `{${position ? Number(position) - 1 : next++}}`,
  );
}

/** Index of the key's first numeric placeholder (the plural count), or -1. */
function countIndex(key: string): number {
  let next = 0;
  for (const match of key.matchAll(PRINTF)) {
    if (match[0] === '%%') continue;
    const index = match[1] ? Number(match[1]) - 1 : next++;
    if (NUMERIC.test(match[0])) return index;
  }
  return -1;
}

function format(template: string, args: readonly unknown[]): string {
  return template.replace(/\{(\d+)\}/g, (whole, index: string) => {
    const i = Number(index);
    return i < args.length ? String(args[i]) : whole;
  });
}

function lookup(key: string): Entry | undefined {
  return catalogs[current][key] ?? catalogs.en[key];
}

function pick(entry: Entry, count: number | undefined): string {
  if (typeof entry === 'string') return entry;
  const form = count === undefined ? 'other' : new Intl.PluralRules(current).select(count);
  return entry[form] ?? entry.other ?? Object.values(entry)[0] ?? '';
}

/**
 * The translation of `key` with `args` filled in order. A plural key picks its form from the first
 * numeric argument. A missing key returns the key itself with the arguments substituted.
 */
export function t(key: string, ...args: unknown[]): string {
  const entry = lookup(key);
  if (entry === undefined) return format(convertPrintf(key), args);
  const count = args.find((a): a is number => typeof a === 'number');
  return format(pick(entry, count), args);
}

/**
 * A plural string: `count` selects the form and fills the key's first numeric placeholder; `args` fill
 * the other placeholders in order. `tn('%lld connections · %@', 3, 'Office')`.
 */
export function tn(key: string, count: number, ...args: unknown[]): string {
  const all = [...args];
  const at = countIndex(key);
  all.splice(at < 0 ? 0 : at, 0, count);
  const entry = lookup(key);
  if (entry === undefined) return format(convertPrintf(key), all);
  return format(pick(entry, count), all);
}
