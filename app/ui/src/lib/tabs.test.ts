// Ports of the Swift TabColorOverrideTests and TabRenameTests, plus the SessionTabs open/close/cycle rules.

import { describe, expect, it } from 'vitest';
import { SessionTabs, TabsStore, openTestTab } from './tabs.svelte';

function tabsWith(...ids: string[]) {
  const tabs = new SessionTabs();
  for (const id of ids) tabs.open(id);
  return tabs;
}

describe('TabColorOverride', () => {
  it('is automatic unless overridden', () => {
    const tabs = tabsWith('a');
    expect(tabs.color('a', 'red')).toBe('red');
    tabs.setColor('blue', 'a');
    expect(tabs.color('a', 'red')).toBe('blue');
    tabs.setColor(null, 'a');
    expect(tabs.color('a', 'red')).toBeNull();
    tabs.resetColor('a');
    expect(tabs.color('a', 'red')).toBe('red');
  });

  it('ignores an unknown tab', () => {
    const tabs = new SessionTabs();
    tabs.setColor('blue', 'x');
    expect(tabs.colorOverrides).toEqual({});
  });

  it('drops the override on close and moves it on replace', () => {
    const tabs = tabsWith('a', 'b');
    tabs.setColor('green', 'a');
    tabs.setColor(null, 'b');
    tabs.replace('a', 's');
    expect(tabs.color('s', null)).toBe('green');
    expect('a' in tabs.colorOverrides).toBe(false);
    tabs.close('b');
    expect('b' in tabs.colorOverrides).toBe(false);
    expect(tabs.color('b', 'red')).toBe('red');
  });
});

describe('TabRename', () => {
  it('lets a trimmed custom title win over the automatic one', () => {
    const tabs = tabsWith('a');
    expect(tabs.title('a', 'root@web: ~')).toBe('root@web: ~');
    tabs.rename('a', '  prod db  ');
    expect(tabs.title('a', 'root@web: /var/log')).toBe('prod db');
  });

  it('restores the automatic title for an empty name', () => {
    const tabs = tabsWith('a');
    tabs.rename('a', 'x');
    tabs.rename('a', '   ');
    expect(tabs.customTitles).toEqual({});
    expect(tabs.title('a', 'auto')).toBe('auto');
  });

  it('ignores unknown ids, keeps the title on replace and drops it on close', () => {
    const tabs = new SessionTabs();
    tabs.rename('x', 'nope');
    expect(tabs.customTitles).toEqual({});
    tabs.open('a');
    tabs.open('b');
    tabs.rename('a', 'first');
    tabs.rename('b', 'second');
    tabs.replace('a', 's');
    expect(tabs.title('s', '')).toBe('first');
    tabs.close('b');
    expect(tabs.customTitles).toEqual({ s: 'first' });
  });
});

describe('SessionTabs', () => {
  it('opens once and activates', () => {
    const tabs = tabsWith('a', 'b', 'a');
    expect(tabs.ids).toEqual(['a', 'b']);
    expect(tabs.active).toBe('a');
  });

  it('closing the active tab selects the right neighbour, else the left, else Details', () => {
    const tabs = tabsWith('a', 'b', 'c');
    tabs.select('b');
    tabs.close('b');
    expect(tabs.active).toBe('c');
    tabs.close('c');
    expect(tabs.active).toBe('a');
    tabs.close('a');
    expect(tabs.active).toBeNull();
  });

  it('closing an inactive tab keeps the active one', () => {
    const tabs = tabsWith('a', 'b');
    tabs.close('a');
    expect(tabs.active).toBe('b');
    tabs.close('zzz');
    expect(tabs.ids).toEqual(['b']);
  });

  it('cycles through Details and the sessions, wrapping', () => {
    const tabs = tabsWith('a', 'b');
    tabs.select(null);
    tabs.cycle(true);
    expect(tabs.active).toBe('a');
    tabs.cycle(true);
    tabs.cycle(true);
    expect(tabs.active).toBeNull();
    tabs.cycle(false);
    expect(tabs.active).toBe('b');
  });

  it('select ignores unknown ids', () => {
    const tabs = tabsWith('a');
    tabs.select('nope');
    expect(tabs.active).toBe('a');
  });

  it('replace keeps the position and the active state, and refuses an existing id', () => {
    const tabs = tabsWith('a', 'b');
    tabs.select('a');
    tabs.replace('a', 'b');
    expect(tabs.ids).toEqual(['a', 'b']);
    tabs.replace('a', 'z');
    expect(tabs.ids).toEqual(['z', 'b']);
    expect(tabs.active).toBe('z');
  });
});

describe('TabsStore', () => {
  it('tracks sessions with their tabs', () => {
    const store = new TabsStore();
    expect(store.hasSessions).toBe(false);
    store.openSession({ id: 's1', title: 'web', type: 'ssh', color: 'red' });
    expect(store.activeSession?.id).toBe('s1');
    store.rename('s1', 'prod');
    expect(store.displayTitle(store.session('s1')!)).toBe('prod');
    store.setColor(null, 's1');
    expect(store.displayColor(store.session('s1')!)).toBeNull();
    store.closeActive();
    expect(store.hasSessions).toBe(false);
    expect(store.active).toBeNull();
    store.closeActive();
  });

  it('updates a tab and tells listeners about closed session tabs', () => {
    const store = new TabsStore();
    const closed: string[] = [];
    const off = store.onSessionClosed((id) => closed.push(id));
    store.openSession({ id: 's1', title: 'web', type: 'ssh', color: null });
    const before = store.sessions;
    store.updateSession('s1', { title: 'web' });
    expect(store.sessions).toBe(before);
    store.updateSession('s1', { title: 'root@web', root: true });
    expect(store.session('s1')).toMatchObject({ title: 'root@web', root: true });
    store.updateSession('nope', { title: 'x' });
    store.closeSession('s1');
    store.closeSession('s1');
    expect(closed).toEqual(['s1']);
    off();
    store.openSession({ id: 's2', title: 'db', type: 'ssh', color: null });
    store.closeActive();
    expect(closed).toEqual(['s1']);
  });

  it('opens an ungrouped session right after a given tab', () => {
    const store = new TabsStore();
    for (const id of ['a', 'b']) store.openSession({ id, title: id, type: 'ssh', color: null });
    store.openSession({ id: 'c', title: 'c', type: 'ssh', color: null }, 'a');
    store.openSession({ id: 'd', title: 'd', type: 'ssh', color: null }, 'nope');
    expect(store.ids).toEqual(['a', 'c', 'b', 'd']);
    expect(store.sessions.map((s) => s.id)).toEqual(store.ids);
  });

  it('detaches a session tab without telling the listeners', () => {
    const store = new TabsStore();
    const closed: string[] = [];
    store.onSessionClosed((id) => closed.push(id));
    for (const id of ['a', 'b', 'c']) store.openSession({ id, title: id, type: 'ssh', color: null });
    store.select('b');
    store.rename('b', 'named');
    store.detachSession('b');
    store.detachSession('nope');
    expect(store.ids).toEqual(['a', 'c']);
    expect(store.sessions.map((s) => s.id)).toEqual(['a', 'c']);
    expect(store.active).toBe('c');
    expect(store.customTitles).toEqual({});
    expect(closed).toEqual([]);
  });

  it('opens a connection’s tabs next to each other, numbered with the lowest free number', () => {
    const store = new TabsStore();
    const open = (id: string, group?: string) =>
      store.openSession({ id, title: group ?? id, type: 'ssh', color: null, ...(group ? { group } : {}) });
    open('a1', 'web');
    open('b1', 'db');
    open('x');
    open('a2', 'web');
    expect(store.ids).toEqual(['a1', 'a2', 'b1', 'x']);
    expect(store.sessions.map((s) => s.id)).toEqual(store.ids);
    expect(store.active).toBe('a2');
    expect(store.sessions.map((s) => store.displayTitle(s))).toEqual(['web', 'web (2)', 'db', 'x']);

    open('a3', 'web');
    store.closeSession('a2');
    open('a4', 'web');
    expect(store.ids).toEqual(['a1', 'a3', 'a4', 'b1', 'x']);
    expect(store.sessions.map((s) => store.displayTitle(s)).slice(0, 3)).toEqual([
      'web',
      'web (3)',
      'web (2)',
    ]);

    // A remote title change keeps the number; the user's name replaces it.
    store.updateSession('a3', { title: 'root@web' });
    expect(store.displayTitle(store.session('a3')!)).toBe('root@web (3)');
    store.rename('a3', 'logs');
    expect(store.displayTitle(store.session('a3')!)).toBe('logs');
  });

  it("numbers a group's tabs only among those with the same title", () => {
    const store = new TabsStore();
    const open = (id: string, title: string) =>
      store.openSession({ id, title, type: 'ssh', color: null, group: 'e1' });
    open('a1', 'Home · SSH');
    open('b1', 'Home · Console');
    open('a2', 'Home · SSH');
    expect(store.sessions.map((s) => store.displayTitle(s))).toEqual([
      'Home · SSH',
      'Home · Console',
      'Home · SSH (2)',
    ]);
  });

  it('keeps the group and number through a reconnect, but not when the tab becomes a split', () => {
    const store = new TabsStore();
    store.openSession({ id: 'a1', title: 'web', type: 'ssh', color: null, group: 'web' });
    store.openSession({ id: 'a2', title: 'web', type: 'ssh', color: null, group: 'web' });
    store.replaceSession('a2', { id: 'a2b', title: 'web', type: 'ssh', color: null });
    expect(store.session('a2b')).toMatchObject({ group: 'web', ordinal: 2 });
    expect(store.ids).toEqual(['a1', 'a2b']);
    store.replaceSession('a1', { id: 'split', title: 'Split', type: 'ssh', color: null, split: true });
    expect(store.session('split')?.group).toBeUndefined();
    expect(store.displayTitle(store.session('split')!)).toBe('Split');
  });

  it('openTestTab adds a fake session', () => {
    const store = new TabsStore();
    openTestTab(store);
    openTestTab(store);
    expect(store.sessions).toHaveLength(2);
    expect(store.active).toBe(store.sessions[1]?.id);
  });
});
