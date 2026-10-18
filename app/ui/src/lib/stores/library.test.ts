import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fakeBridge } from '../testing/bridge';
import { libraryOf } from '../testing/fixtures';
import { createLibraryStore, makeTag, parseTag } from './library.svelte';

describe('sidebar tags', () => {
  it('parse and format', () => {
    expect(parseTag('fav:1')).toEqual({ kind: 'favorite', id: '1' });
    expect(parseTag('f:2')).toEqual({ kind: 'folder', id: '2' });
    expect(parseTag('k:3')).toEqual({ kind: 'credential', id: '3' });
    expect(parseTag('s:4')).toEqual({ kind: 'split', id: '4' });
    expect(parseTag('c:5')).toEqual({ kind: 'connection', id: '5' });
    expect(parseTag('x:5')).toBeNull();
    expect(makeTag('favorite', '1')).toBe('fav:1');
  });
});

describe('library store', () => {
  it('loads and replaces the library on library-changed', async () => {
    const bridge = fakeBridge({ library: libraryOf(['a', 'b']) });
    const store = createLibraryStore(bridge);
    await store.start();
    expect(store.connection('a')?.name).toBe('a');
    bridge.emit('library-changed', libraryOf(['c']));
    expect(store.connection('a')).toBeUndefined();
    expect(store.connection('c')).toBeDefined();
    store.stop();
    expect(bridge.listenerCount('library-changed')).toBe(0);
  });

  it('selects, toggles and extends', () => {
    const store = createLibraryStore(fakeBridge());
    store.replace(libraryOf(['a', 'b', 'c', 'd']));
    const order = ['c:a', 'c:b', 'c:c', 'c:d'];
    store.select('c:b');
    expect(store.single).toBe('c:b');
    store.extendTo('c:d', order);
    expect(store.selection).toEqual(['c:b', 'c:c', 'c:d']);
    expect(store.single).toBeNull();
    expect(store.primary).toBe('c:b');
    store.toggle('c:c');
    expect(store.selection).toEqual(['c:b', 'c:d']);
    expect(store.primary).toBe('c:b');
    store.toggle('c:a');
    expect(store.primary).toBe('c:a');
    store.extendTo('c:c', order);
    expect(store.selection).toEqual(['c:a', 'c:b', 'c:c']);
    store.clearSelection();
    expect(store.selection).toEqual([]);
    expect(store.primary).toBeNull();
  });

  it('extends from nothing as a plain select', () => {
    const store = createLibraryStore(fakeBridge());
    store.extendTo('c:b', ['c:a', 'c:b']);
    expect(store.selection).toEqual(['c:b']);
  });

  it('collects selected connections without duplicates', () => {
    const store = createLibraryStore(fakeBridge());
    store.replace(libraryOf(['a', 'b'], ['f1']));
    store.setSelection(['c:a', 'fav:a', 'f:f1', 'c:b']);
    expect(store.selectedConnections.map((c) => c.id)).toEqual(['a', 'b']);
  });

  it('drops selected items that disappeared', () => {
    const store = createLibraryStore(fakeBridge());
    store.replace(libraryOf(['a', 'b']));
    store.setSelection(['c:a', 'c:b']);
    store.toggle('c:b');
    store.toggle('c:b');
    store.replace(libraryOf(['a']));
    expect(store.selection).toEqual(['c:a']);
    expect(store.anchor).toBeNull();
  });

  describe('search', () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it('debounces and ignores stale answers', async () => {
      let answer = 0;
      const bridge = fakeBridge({
        search: (args?: Record<string, unknown>) => ({
          connections: [`${args?.query}-${++answer}`],
          credentials: [],
        }),
      });
      const store = createLibraryStore({ ...bridge, searchDelay: 100 });
      store.setQuery('a');
      store.setQuery('ab');
      expect(store.isSearching).toBe(true);
      await vi.advanceTimersByTimeAsync(100);
      expect(bridge.invoke).toHaveBeenCalledTimes(1);
      expect(store.results).toEqual({ connections: ['ab-1'], credentials: [] });
      store.setQuery('  ');
      expect(store.results).toBeNull();
      expect(store.isSearching).toBe(false);
    });

    it('drops a late answer after the query was cleared', async () => {
      let resolve: (value: unknown) => void = () => {};
      const bridge = fakeBridge({ search: () => new Promise((r) => (resolve = r)) });
      const store = createLibraryStore({ ...bridge, searchDelay: 0 });
      store.setQuery('x');
      await vi.advanceTimersByTimeAsync(0);
      store.setQuery('');
      resolve({ connections: ['late'], credentials: [] });
      await vi.advanceTimersByTimeAsync(0);
      expect(store.results).toBeNull();
    });

    it('refreshes the results when the library changes', async () => {
      const bridge = fakeBridge({ search: { connections: ['a'], credentials: [] } });
      const store = createLibraryStore({ ...bridge, searchDelay: 0 });
      store.setQuery('a');
      await vi.advanceTimersByTimeAsync(0);
      store.replace(libraryOf(['a']));
      await vi.advanceTimersByTimeAsync(0);
      expect(bridge.invoke).toHaveBeenCalledTimes(2);
    });

    it('shows no hits when the search fails', async () => {
      const store = createLibraryStore({ ...fakeBridge(), searchDelay: 0 });
      store.setQuery('x');
      await vi.advanceTimersByTimeAsync(0);
      expect(store.results).toEqual({ connections: [], credentials: [] });
    });
  });
});
