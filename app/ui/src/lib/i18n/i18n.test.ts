import { afterEach, describe, expect, it } from 'vitest';
import { convertPrintf, has, i18n, setLocale, t, tn } from './index.svelte';

afterEach(() => setLocale('en'));

describe('i18n', () => {
  it('converts printf placeholders in order and by position', () => {
    expect(convertPrintf('%@ of %lld (%d%%)')).toBe('{0} of {1} ({2}%)');
    expect(convertPrintf('%2$@ then %1$@')).toBe('{1} then {0}');
  });

  it('translates live when the locale changes', () => {
    expect(t('Cancel')).toBe('Cancel');
    setLocale('hu');
    expect(i18n.locale).toBe('hu');
    expect(t('Cancel')).toBe('Mégse');
    expect(document.documentElement.lang).toBe('hu');
  });

  it('falls back to English for an unknown locale', () => {
    setLocale('de');
    expect(i18n.locale).toBe('en');
  });

  it('fills arguments', () => {
    expect(t("Can't open the database: %@", 'disk full')).toBe("Can't open the database: disk full");
    setLocale('hu');
    expect(t('Already used by: %@', 'Lock')).toBe('Már foglalt: Lock');
  });

  it('returns a missing key with its arguments substituted', () => {
    expect(has('Not in the catalog %@')).toBe(false);
    expect(t('Not in the catalog %@ (%lld)', 'x', 2)).toBe('Not in the catalog x (2)');
  });

  it('picks plural forms from the count', () => {
    expect(tn('%lld connections · %@', 1, 'Office')).toBe('1 connection · Office');
    expect(tn('%lld connections · %@', 3, 'Office')).toBe('3 connections · Office');
    expect(t('%lld connections · %@', 1, 'Office')).toBe('1 connection · Office');
    expect(tn('%lld sessions are still running. Quitting closes all of them.', 2)).toBe(
      '2 sessions are still running. Quitting closes all of them.',
    );
    setLocale('hu');
    expect(tn('%lld connections · %@', 1, 'Iroda')).toBe('1 kapcsolat · Iroda');
  });

  it('inserts the count at the first numeric placeholder', () => {
    expect(tn('%@ has %lld items', 4, 'Box')).toBe('Box has 4 items');
  });
});
