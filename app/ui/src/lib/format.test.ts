import { describe, expect, it } from 'vitest';
import { middleTruncate, relativeAge } from './format';
import { clampSidebarWidth, loadSidebarWidth, saveSidebarWidth, SIDEBAR_DEFAULT } from './stores/layout';

describe('format', () => {
  it('truncates in the middle', () => {
    expect(middleTruncate('short', 10)).toBe('short');
    const text = middleTruncate('/Users/someone/Library/Application Support/connections.sqlite', 21);
    expect(text).toHaveLength(21);
    expect(text).toBe('/Users/som…ons.sqlite');
  });

  it('formats lock ages', () => {
    expect(relativeAge(30, 'en')).toBe('30 seconds ago');
    expect(relativeAge(300, 'en')).toBe('5 minutes ago');
    expect(relativeAge(7200, 'en')).toBe('2 hours ago');
  });
});

describe('sidebar width', () => {
  it('clamps', () => {
    expect(clampSidebarWidth(10)).toBe(200);
    expect(clampSidebarWidth(9999)).toBe(480);
    expect(clampSidebarWidth(Number.NaN)).toBe(SIDEBAR_DEFAULT);
  });

  it('round-trips through storage and survives failures', () => {
    const store = new Map<string, string>();
    const storage = {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
    };
    expect(loadSidebarWidth(storage)).toBe(SIDEBAR_DEFAULT);
    saveSidebarWidth(321.4, storage);
    expect(loadSidebarWidth(storage)).toBe(321);
    const broken = {
      getItem: () => {
        throw new Error('denied');
      },
      setItem: () => {
        throw new Error('denied');
      },
    };
    expect(loadSidebarWidth(broken)).toBe(SIDEBAR_DEFAULT);
    expect(() => saveSidebarWidth(300, broken)).not.toThrow();
  });
});
