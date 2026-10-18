#!/usr/bin/env node
// Merges the UI-only strings into the UI's i18n JSON files.
//
//   node scripts/merge-i18n-extras.mjs
//
// Reads `app/ui/src/lib/i18n/{en,hu}.json` and the UI-only strings in
// `app/ui/src/lib/i18n/extra.{en,hu}.json`, and writes the catalogs back with the extras merged in.
//
// - The key stays the English source string, printf placeholders included (`%@ here`).
// - Extra values use `{0}`, `{1}`… in argument order; a positional `%2$@` becomes `{1}`; `%%` becomes `%`.
// - Plural values are `{ "one": "…", "other": "…" }`; the count is the first numeric argument.
//
// No dependencies; run it again whenever an extra file changes.

import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const i18nDir = join(root, 'app/ui/src/lib/i18n');
const locales = ['en', 'hu'];

/** printf placeholders, plus `%%`. */
const PLACEHOLDER = /%%|%(?:(\d+)\$)?(?:ll|l|h)?[@dDuUxXfeEgGcs]/g;

/** `%@ of %lld` → `{0} of {1}`; `%2$@ %1$@` → `{1} {0}`. */
function convertPlaceholders(text) {
  let next = 0;
  return text.replace(PLACEHOLDER, (match, position) => {
    if (match === '%%') return '%';
    const index = position ? Number(position) - 1 : next++;
    return `{${index}}`;
  });
}

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

function sorted(object) {
  return Object.fromEntries(Object.entries(object).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
}

function merge() {
  const extras = Object.fromEntries(locales.map((l) => [l, readJson(join(i18nDir, `extra.${l}.json`))]));
  for (const key of Object.keys(extras.en)) {
    if (!(key in extras.hu)) throw new Error(`extra key without Hungarian: ${JSON.stringify(key)}`);
  }
  for (const key of Object.keys(extras.hu)) {
    if (!(key in extras.en)) throw new Error(`Hungarian extra key without English: ${JSON.stringify(key)}`);
  }
  let count = 0;
  for (const locale of locales) {
    const path = join(i18nDir, `${locale}.json`);
    const catalog = readJson(path);
    for (const [key, value] of Object.entries(extras[locale])) {
      catalog[key] =
        typeof value === 'string'
          ? convertPlaceholders(value)
          : Object.fromEntries(Object.entries(value).map(([f, v]) => [f, convertPlaceholders(v)]));
    }
    writeFileSync(path, JSON.stringify(sorted(catalog), null, 2) + '\n');
    count = Object.keys(catalog).length;
  }
  console.log(`wrote ${count} keys to ${i18nDir}/{en,hu}.json`);
}

merge();
