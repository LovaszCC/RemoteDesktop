import { describe, expect, it } from 'vitest';
import { loadSidebarCollapsed, saveSidebarCollapsed } from './layout';
import { createSidebarLayout } from './sidebarLayout.svelte';

function memoryStorage() {
  const store = new Map<string, string>();
  return {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
  };
}

describe('sidebar layout', () => {
  it('toggles and remembers whether the sidebar is shown', () => {
    const storage = memoryStorage();
    const layout = createSidebarLayout(storage);
    expect(layout.collapsed).toBe(false);
    layout.toggle();
    expect(layout.collapsed).toBe(true);
    expect(createSidebarLayout(storage).collapsed).toBe(true);
    layout.set(false);
    expect(loadSidebarCollapsed(storage)).toBe(false);
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
    expect(loadSidebarCollapsed(broken)).toBe(false);
    expect(() => saveSidebarCollapsed(true, broken)).not.toThrow();
    const layout = createSidebarLayout(broken);
    layout.toggle();
    expect(layout.collapsed).toBe(true);
  });
});
