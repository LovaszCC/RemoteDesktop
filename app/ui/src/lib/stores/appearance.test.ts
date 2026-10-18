import { describe, expect, it, vi } from 'vitest';
import {
  ACCENTS,
  accentById,
  applyAccent,
  applyTheme,
  createAppearance,
  loadAccent,
  loadTheme,
  resolveTheme,
} from './appearance.svelte';

function memoryStorage() {
  const store = new Map<string, string>();
  return {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
  };
}

describe('appearance', () => {
  it('offers distinct accents and falls back to Ocean', () => {
    expect(new Set(ACCENTS.map((a) => a.id)).size).toBe(ACCENTS.length);
    expect(accentById('nope').id).toBe('ocean');
    expect(accentById('graphite').chroma).toBe(0);
  });

  it('applies the accent to the root and remembers it', () => {
    const storage = memoryStorage();
    const store = createAppearance(storage);
    store.setAccent('rose');
    const root = document.documentElement;
    expect(root.style.getPropertyValue('--accent-hue')).toBe('10');
    expect(root.dataset.accent).toBe('rose');
    expect(loadAccent(storage).id).toBe('rose');
    expect(createAppearance(storage).accent.id).toBe('rose');
    applyAccent(accentById('ocean'));
    expect(root.style.getPropertyValue('--accent-chroma')).toBe('0.15');
  });

  it('survives storage failures', () => {
    const broken = {
      getItem: () => {
        throw new Error('denied');
      },
      setItem: () => {
        throw new Error('denied');
      },
    };
    expect(loadAccent(broken).id).toBe('ocean');
    const store = createAppearance(broken);
    expect(() => store.setAccent('teal')).not.toThrow();
    expect(store.accent.id).toBe('teal');
  });

  it('resolves, applies and remembers the theme', () => {
    expect(resolveTheme('system', true)).toBe('dark');
    expect(resolveTheme('system', false)).toBe('light');
    expect(resolveTheme('light', true)).toBe('light');
    const storage = memoryStorage();
    expect(loadTheme(storage)).toBe('system');
    const native = vi.fn();
    const store = createAppearance(storage, native);
    store.setTheme('dark');
    const root = document.documentElement;
    expect(root.dataset.theme).toBe('dark');
    expect(root.style.colorScheme).toBe('dark');
    expect(native).toHaveBeenLastCalledWith('dark');
    expect(loadTheme(storage)).toBe('dark');
    store.setTheme('system');
    expect(native).toHaveBeenLastCalledWith(null);
    expect(root.dataset.theme).toBe('light');
    applyTheme('light');
    expect(root.dataset.theme).toBe('light');
  });

  it('ignores an unknown stored theme', () => {
    const storage = memoryStorage();
    storage.setItem('rd.theme', 'neon');
    expect(loadTheme(storage)).toBe('system');
  });
});
