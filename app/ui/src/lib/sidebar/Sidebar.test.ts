import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { closeContextMenu, contextMenu } from '../components/menu.svelte';
import { createApi } from '../api';
import { createAlertStore } from '../stores/alert.svelte';
import { createDialogStore } from '../stores/dialogs.svelte';
import { createEditorStore } from '../stores/editor.svelte';
import { createLibraryStore } from '../stores/library.svelte';
import { createSettingsStore } from '../stores/settings.svelte';
import { createToastStore } from '../stores/toast.svelte';
import { fakeBridge } from '../testing/bridge';
import { multiEntryLibrary, settingsDto, sidebarLibrary } from '../testing/fixtures';
import { EMPTY_LIBRARY, type ConnectionDto, type Platform } from '../types';
import { createForwardStore } from '../forwards/forwards.svelte';
import { TabsStore } from '../tabs.svelte';
import { createSessionsStore, type SessionsStore } from '../terminal/sessions.svelte';
import { createSidebarActions } from './actions.svelte';
import Sidebar from './Sidebar.svelte';

let component: ReturnType<typeof mount> | undefined;

afterEach(() => {
  if (component) unmount(component);
  component = undefined;
  closeContextMenu();
  document.body.innerHTML = '';
});

function render({
  platform = 'macos' as Platform,
  readOnly = false,
  forwards = createForwardStore(fakeBridge()),
  sessions = undefined as SessionsStore | undefined,
  live = [] as string[],
  library = sidebarLibrary(),
  rdpSessions = undefined as { open: (connection: ConnectionDto) => Promise<string> } | undefined,
} = {}) {
  const bridge = fakeBridge({
    settings_set: (args?: Record<string, unknown>) => settingsDto(args?.patch as object),
  });
  const store = createLibraryStore({ ...bridge, searchDelay: 0 });
  store.replace(library);
  const toast = createToastStore();
  const actions = createSidebarActions({
    api: createApi(bridge.invoke),
    library: store,
    settings: createSettingsStore({ ...bridge, onLanguage: () => {} }),
    toast,
    alerts: createAlertStore(),
    editor: createEditorStore(),
    dialogs: createDialogStore(),
    ...(sessions ? { sessions } : {}),
    ...(rdpSessions ? { rdpSessions } : {}),
  });
  component = mount(Sidebar, {
    target: document.body,
    props: {
      store,
      actions,
      platform,
      readOnly,
      forwards,
      connectionTabs: { isLive: (id: string) => live.includes(id) },
    },
  });
  flushSync();
  return { store, actions, toast, bridge };
}

const headers = () => [...document.querySelectorAll('h3')].map((h) => h.textContent?.trim());
const rowTags = () => [...document.querySelectorAll<HTMLElement>('[data-tag]')].map((r) => r.dataset.tag);
const row = (tag: string) => document.querySelector<HTMLElement>(`[data-tag="${tag}"]`)!;

function click(tag: string, init: MouseEventInit = {}) {
  row(tag).dispatchEvent(new MouseEvent('click', { bubbles: true, ...init }));
  flushSync();
}

function key(k: string, init: KeyboardEventInit = {}) {
  document
    .querySelector('.sidebar-list')!
    .dispatchEvent(new KeyboardEvent('keydown', { key: k, bubbles: true, ...init }));
  flushSync();
}

describe('Sidebar', () => {
  it('renders the sections and rows in order', () => {
    render();
    expect(headers()).toEqual(['Favorites', 'Saved splits', 'Connections', 'Credentials']);
    expect(rowTags()).toEqual([
      'fav:c1',
      'fav:c2',
      's:s1',
      's:s2',
      'f:f1',
      'f:f3',
      'e:c1',
      'e:c5',
      'e:c4',
      'k:k1',
      'k:k2',
    ]);
    expect(row('s:s1').textContent).toContain('box + db');
    expect(row('s:s2').textContent).toContain('no connections');
    expect(row('k:k1').querySelector('.count')?.textContent).toBe('1');
    expect(row('k:k2').querySelector('.count')).toBeNull();
    expect(row('fav:c2').querySelector('.swatch')?.getAttribute('title')).toBe('Blue');
    expect(row('fav:c2').querySelector('.star')).not.toBeNull();
    expect(row('e:c1').title).toBe('SSH · c1.example.test');
  });

  it('shows the empty state for an empty library', () => {
    const { store } = render();
    store.replace(EMPTY_LIBRARY);
    flushSync();
    expect(document.querySelector('.empty-state')?.textContent).toContain('No connections');
    expect(rowTags()).toEqual([]);
  });

  it('selects with click, toggles with ⌘ on macOS and Ctrl elsewhere, and extends with Shift', () => {
    const { store } = render();
    click('fav:c2');
    expect(store.selection).toEqual(['fav:c2']);
    click('f:f1', { metaKey: true });
    expect(store.selection).toEqual(['fav:c2', 'f:f1']);
    click('f:f1', { ctrlKey: true });
    expect(store.selection).toEqual(['fav:c2', 'f:f1']);
    click('e:c5', { shiftKey: true });
    expect(store.selection).toEqual(['f:f1', 'f:f3', 'e:c1', 'e:c5']);
    expect(row('e:c1').getAttribute('aria-selected')).toBe('true');
  });

  it('toggles with Ctrl on Windows', () => {
    const { store } = render({ platform: 'windows' });
    click('e:c1');
    click('e:c4', { ctrlKey: true });
    expect(store.selection).toEqual(['e:c1', 'e:c4']);
  });

  it('moves with the arrow keys and opens folders with → and Return', async () => {
    const { store, actions } = render();
    key('ArrowDown');
    expect(store.selection).toEqual(['fav:c1']);
    click('f:f1');
    key('ArrowDown');
    expect(store.selection).toEqual(['f:f3']);
    key('ArrowUp');
    key('ArrowRight');
    expect([...actions.expanded]).toEqual(['f1']);
    flushSync();
    expect(rowTags()).toContain('f:f2');
    key('Enter');
    expect([...actions.expanded]).toEqual([]);
  });

  it('double-click runs the primary action', () => {
    const { bridge } = render();
    click('e:c4');
    row('e:c4').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    expect(bridge.invoke).toHaveBeenCalledWith('connect_winbox', { connectionId: 'c4' });
  });

  it('opens the context menu for the clicked row or the selection', () => {
    const { store } = render();
    row('k:k1').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    expect(contextMenu.open?.items.map((i) => (i.kind === 'separator' ? '-' : i.label))).toEqual([
      'Edit…',
      '-',
      'Delete credential',
    ]);
    store.setSelection(['e:c1', 'e:c4']);
    flushSync();
    row('e:c4').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    const first = contextMenu.open?.items[0];
    expect(first?.kind !== 'separator' && first?.label).toBe('Open in split view (2)');
    document.querySelector('.sidebar-list')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true }));
    const empty = contextMenu.open?.items[0];
    expect(empty?.kind !== 'separator' && empty?.label).toBe('New RDP connection');
  });

  it('renders search results with folder paths', async () => {
    const { store } = render();
    store.results = { entries: ['c3'], credentials: ['k2'] };
    store.query = 'db';
    flushSync();
    expect(headers()).toEqual(['Results', 'Credentials']);
    expect(rowTags()).toEqual(['e:c3', 'k:k2']);
    expect(row('e:c3').querySelector('.subtitle')?.textContent).toBe('Office / Servers');
  });
});

describe('Local Terminal entry', () => {
  function withSessions() {
    let n = 0;
    const bridge = fakeBridge({ open_local_terminal: () => ({ sessionId: `L${++n}` }) });
    const tabs = new TabsStore();
    const sessions = createSessionsStore({
      api: createApi(bridge.invoke),
      listen: bridge.listen,
      tabs,
      createTerminal: () => ({
        write: vi.fn(),
        clearLocal: vi.fn(),
        attach: vi.fn(),
        detach: vi.fn(),
        fit: vi.fn(),
        size: () => ({ cols: 80, rows: 24 }),
        focus: vi.fn(),
        dispose: vi.fn(),
      }),
      createChannel: () => ({ onmessage: () => {} }),
    });
    return { ...render({ sessions }), bridge, tabs, sessions };
  }

  const entry = () => document.querySelector<HTMLElement>('[data-local-terminal]')!;

  it('is a fixed entry above the library', () => {
    const { store } = render();
    expect(entry().textContent).toContain('Local Terminal');
    expect(entry().dataset.tag).toBeUndefined();
    expect(entry().draggable).toBe(false);
    expect(document.querySelector('.sidebar-list')!.contains(entry())).toBe(false);
    // It stays while searching and with an empty library.
    store.query = 'zzz';
    flushSync();
    expect(entry()).toBeTruthy();
    store.replace(EMPTY_LIBRARY);
    store.query = '';
    flushSync();
    expect(entry()).toBeTruthy();
  });

  it('opens a local terminal tab on double-click and Enter', async () => {
    const { tabs, bridge, store, actions } = withSessions();
    const primary = vi.spyOn(actions, 'primaryAction');
    store.select('e:c1');
    entry().dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    await vi.waitFor(() => expect(tabs.session('L1')?.title).toBe('Local Terminal'));
    expect(bridge.invoke).toHaveBeenCalledWith('open_local_terminal', expect.objectContaining({ cols: 80 }));

    entry().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await vi.waitFor(() => expect(tabs.session('L2')?.title).toBe('Local Terminal 2'));
    expect(primary).not.toHaveBeenCalled();
    expect(tabs.active).toBe('L2');
  });

  it('has a context menu with just Open', async () => {
    const { tabs } = withSessions();
    entry().dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 5, clientY: 5 }));
    flushSync();
    const items = contextMenu.open?.items ?? [];
    expect(items.map((i) => (i.kind === 'separator' ? '-' : i.label))).toEqual(['Open']);
    const open = items[0];
    if (open?.kind !== 'separator') open?.action?.();
    await vi.waitFor(() => expect(tabs.session('L1')).toBeTruthy());
  });
});

describe('open session dot', () => {
  const dot = (el: Element) => el.querySelector('[aria-label="Open session"]');

  it('marks connections with a live embedded session, favourites too', () => {
    render({ live: ['c1'] });
    expect(dot(row('e:c1'))).toBeTruthy();
    for (const tag of rowTags()) {
      if (tag !== 'e:c1' && tag !== 'fav:c1') expect(dot(row(tag!)), tag).toBeNull();
    }
  });

  it('marks the Local Terminal row while a local terminal runs', () => {
    const local = () => document.querySelector('[data-local-terminal]')!;
    render();
    expect(dot(local())).toBeNull();
    unmount(component!);
    render({ live: ['local-terminal'] });
    expect(dot(local())).toBeTruthy();
  });
});

describe('port forward indicator', () => {
  it('marks SSH connections with a starting or active forward', () => {
    const forwards = createForwardStore(fakeBridge());
    render({ forwards });
    const indicator = () => row('e:c1').querySelector('[aria-label="Port forward active"]');
    expect(indicator()).toBeNull();
    forwards.apply({ id: 'p1', connectionId: 'c1', state: 'starting' });
    flushSync();
    expect(indicator()).toBeTruthy();
    forwards.apply({ id: 'p1', connectionId: 'c1', state: 'active' });
    flushSync();
    expect(indicator()?.querySelector('svg')).toBeTruthy();
    forwards.apply({ id: 'p1', connectionId: 'c1', state: 'failed', code: 'timeout', error: 'late' });
    flushSync();
    expect(indicator()).toBeNull();
  });
});

describe('entries with several connections', () => {
  const chevron = (tag: string) => row(tag).querySelector<HTMLButtonElement>('.disclosure')!;

  it('expands an entry with a chevron to its connections, the default marked', async () => {
    render({ library: multiEntryLibrary() });
    expect(chevron('e:e1')).toBeTruthy();
    // A one-connection entry keeps the chevron's place, hidden, so all entries line up.
    expect(row('e:c1').querySelector('.disclosure.hidden')).toBeTruthy();
    expect(row('e:e1').querySelector('.disclosure.hidden')).toBeNull();
    expect(rowTags()).not.toContain('c:h1');
    chevron('e:e1').click();
    flushSync();
    await vi.waitFor(() => expect(rowTags()).toContain('c:h1'));
    const tags = rowTags();
    expect(tags.slice(tags.indexOf('e:e1'), tags.indexOf('e:e1') + 3)).toEqual(['e:e1', 'c:h1', 'c:h2']);
    expect(row('c:h1').textContent).toContain('SSH · root');
    // One step inside the entry: the child keeps a (hidden) disclosure place before its icon.
    expect(row('c:h1').querySelector('.disclosure.hidden')).toBeTruthy();
    expect(row('c:h2').textContent).toContain('Console');
    expect(row('c:h1').querySelector('[aria-label="Default connection"]')).toBeTruthy();
    expect(row('c:h2').querySelector('[aria-label="Default connection"]')).toBeNull();
  });

  it('double_click_on_a_child_connects_that_connection', async () => {
    const rdpSessions = { open: vi.fn(async () => 'R1') };
    const { actions } = render({ library: multiEntryLibrary(), rdpSessions });
    await actions.setExpanded('e1', true);
    flushSync();
    row('c:h2').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    await vi.waitFor(() =>
      expect(rdpSessions.open).toHaveBeenCalledWith(expect.objectContaining({ id: 'h2' })),
    );
  });

  it('double-click on the entry connects its default connection', async () => {
    const rdpSessions = { open: vi.fn(async () => 'R1') };
    const library = multiEntryLibrary();
    library.entries = library.entries.map((e) => (e.id === 'e1' ? { ...e, defaultConnectionId: 'h2' } : e));
    render({ library, rdpSessions });
    row('e:e1').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    await vi.waitFor(() =>
      expect(rdpSessions.open).toHaveBeenCalledWith(expect.objectContaining({ id: 'h2' })),
    );
  });

  it('opens and closes an entry with → and ←', async () => {
    const { store, actions } = render({ library: multiEntryLibrary() });
    click('e:e1');
    key('ArrowRight');
    expect([...actions.expanded]).toEqual(['e1']);
    flushSync();
    expect(rowTags()).toContain('c:h2');
    key('ArrowLeft');
    expect([...actions.expanded]).toEqual([]);
    expect(store.selection).toEqual(['e:e1']);
  });

  it('shows the dot on the entry when a child is live, and on each live child', async () => {
    const dot = (tag: string) => row(tag).querySelector('[aria-label="Open session"]');
    const { actions } = render({ library: multiEntryLibrary(), live: ['h2'] });
    expect(dot('e:e1')).toBeTruthy();
    await actions.setExpanded('e1', true);
    flushSync();
    expect(dot('c:h2')).toBeTruthy();
    expect(dot('c:h1')).toBeNull();
    expect(dot('e:c1')).toBeNull();
  });
});
