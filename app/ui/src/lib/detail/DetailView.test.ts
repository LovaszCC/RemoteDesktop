import { flushSync } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { contextMenu } from '../components/menu.svelte';
import { multiEntryLibrary, sidebarLibrary } from '../testing/fixtures';
import { buttonByText, check, cleanupView, click, renderView, settle, text, type } from '../testing/views';
import type { LibraryDto } from '../types';
import DetailView from './DetailView.svelte';

afterEach(cleanupView);

function render(select: string[], options: Parameters<typeof renderView>[2] = {}) {
  const view = renderView(DetailView, {}, options);
  view.store.setSelection(select);
  flushSync();
  return view;
}

/** box (c1) uses the credential k1 with a password; its own login has none. */
function linkedLibrary(): LibraryDto {
  const library = sidebarLibrary();
  library.connections[0] = {
    ...library.connections[0]!,
    credentialId: 'k1',
    notes: 'rack 4',
    resolved: { username: 'admin', domain: '', hasPassword: true, sshKeyPath: null, hasPassphrase: false },
  };
  library.entries[0] = { ...library.entries[0]!, notes: 'rack 4' };
  library.portForwards = [
    {
      id: 'p1',
      connectionId: 'c1',
      name: '',
      localPort: 8080,
      remoteHost: 'intranet',
      remotePort: 80,
      createdAt: '',
    },
  ];
  return library;
}

describe('DetailView open tabs', () => {
  const tab = (id: string, title: string, split = false) => ({ id, title, type: 'ssh' as const, split });

  it('lists the tabs a connection is open in when there are several, and goes to one', () => {
    const view = render(['c:c1'], {
      openTabs: { c1: [tab('S1', 'box'), tab('S2', 'box (2)'), tab('SP', 'Split', true)] },
    });
    expect(text()).toContain('Open tabs');
    buttonByText('box (2)').click();
    expect(view.selected).toEqual(['S2']);
  });

  it('hides the list with a single tab', () => {
    render(['c:c1'], { openTabs: { c1: [tab('S1', 'box')] } });
    expect(text()).not.toContain('Open tabs');
  });
});

describe('DetailView', () => {
  it('shows a calm placeholder without a selection', () => {
    render([]);
    expect(text()).toContain('Select a connection');
  });

  it('shows a connection with its resolved login, forwards and notes', () => {
    render(['c:c1'], { library: linkedLibrary() });
    const page = text();
    expect(page).toContain('box');
    expect(page).toContain('SSH · embedded terminal');
    expect(page).toContain('••••••••');
    expect(page).toContain('localhost:8080 → intranet:80');
    expect(page).toContain('rack 4');
    expect(buttonByText('Copy password')).toBeTruthy();
    expect(buttonByText('Files (SCP)…').disabled).toBe(false);
  });

  it('opens the file browser of the connection', async () => {
    const { bridge } = render(['c:c1'], {
      replies: { files_open: { browserId: 'files-c1', connectionName: 'box', created: true } },
    });
    click(buttonByText('Files (SCP)…'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('files_open', {
      connectionId: 'c1',
      startPath: null,
      sessionId: null,
    });
  });

  it('hides Copy password without a resolved password', () => {
    render(['c:c2']);
    expect(() => buttonByText('Copy password')).toThrow();
    expect(text()).toContain('Admin session');
  });

  it('copies a field with a toast and links to the credential', async () => {
    const { bridge, toast, store } = render(['fav:c1'], { library: linkedLibrary() });
    click(document.querySelector('[aria-label="Copy"]')!);
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('copy_text', { text: 'c1.example.test' });
    expect(toast.current?.message).toBe('Copied');
    click(buttonByText('Domain admin'));
    expect(store.selection).toEqual(['k:k1']);
  });

  it('shows one prominent button per MikroTik mode', () => {
    render(['c:c4']);
    expect(buttonByText('WinBox').className).toContain('primary');
    expect(buttonByText('SSH').className).not.toContain('primary');
  });

  it('names the browser of a web connection and opens it on Connect', async () => {
    const library = sidebarLibrary();
    library.connections[4] = { ...library.connections[4]!, browserBundleId: 'org.mozilla.firefox' };
    const { bridge, toast } = render(['c:c5'], {
      library,
      replies: {
        installed_browsers: [{ id: 'org.mozilla.firefox', name: 'Firefox' }],
        connect_web: { kind: 'loginCopied' },
      },
    });
    await settle();
    expect(text()).toContain('Web · Firefox');
    click(buttonByText('Connect'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('connect_web', { connectionId: 'c5' });
    expect(toast.current?.message).toBe('Opened: portal · password, then username on the clipboard');
  });

  it('starts WinBox and the system RDP client from Connect', async () => {
    const library = sidebarLibrary();
    library.connections[1] = { ...library.connections[1]!, rdpClient: 'windowsApp' };
    const { bridge } = render(['c:c4'], {
      library,
      replies: { connect_winbox: { kind: 'launched' }, connect_rdp_external: { kind: 'launched' } },
    });
    click(buttonByText('WinBox'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('connect_winbox', { connectionId: 'c4' });
    cleanupView();
    const second = render(['c:c2'], {
      library,
      replies: { connect_rdp_external: { kind: 'launched' } },
    });
    expect(text()).toContain('RDP · Windows App');
    click(buttonByText('Connect'));
    await settle();
    expect(second.bridge.invoke).toHaveBeenCalledWith('connect_rdp_external', { connectionId: 'c2' });
  });

  it('disables editing when read-only', () => {
    render(['c:c1'], { readOnly: true });
    expect(buttonByText('Edit').disabled).toBe(true);
  });

  it('shows a folder with New buttons', () => {
    const { editor } = render(['f:f1']);
    expect(text()).toContain('1 connection · Office');
    click(buttonByText('New SSH'));
    expect(editor.entry).toEqual({ mode: 'new', type: 'ssh', folderId: 'f1' });
  });

  it('shows a saved split', () => {
    render(['s:s2']);
    expect(text()).toContain('1 deleted connections are left out.');
    expect(buttonByText('Open').disabled).toBe(true);
  });

  it('applies a credential to a multi-selection', async () => {
    const { bridge } = render(['c:c1', 'f:f1']);
    expect(text()).toContain('2 items selected');
    expect(buttonByText('Apply to 3 connections')).toBeTruthy();
    const select = document.querySelector('select')!;
    select.value = 'k2';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();
    click(buttonByText('Apply to 3 connections'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('set_credential', {
      credentialId: 'k2',
      tags: ['c:c1', 'f:f1'],
      connectionIds: null,
    });
  });
});

describe('EntryDetail', () => {
  const groupTitles = () => [...document.querySelectorAll('h3')].map((h) => h.textContent?.trim());

  it('entry_with_buttons_shows_one_per_pinned_connection_default_primary', () => {
    render(['e:e1'], { library: multiEntryLibrary() });
    expect(text()).toContain('Home Server');
    expect(text()).toContain('home.example.test');
    expect(buttonByText('SSH · root').className).toContain('primary');
    expect(buttonByText('Console').className).not.toContain('primary');
    expect(() => buttonByText('Files (SCP)…')).not.toThrow();
  });

  it('connect_menu_lists_every_connection', () => {
    const library = multiEntryLibrary();
    library.connections = library.connections.map((c) => (c.id === 'h2' ? { ...c, showAsButton: false } : c));
    render(['e:e1'], { library });
    expect(() => buttonByText('Console')).toThrow();
    click(document.querySelector('[aria-haspopup="menu"]')!);
    const items = contextMenu.open?.items ?? [];
    expect(items.map((i) => (i.kind === 'separator' ? '-' : [i.label, i.checked]))).toEqual([
      ['SSH · root', true],
      ['Console', false],
    ]);
  });

  it('single_connection_entry_looks_like_today', () => {
    render(['e:c1']);
    expect(buttonByText('Connect').className).toContain('primary');
    expect(document.querySelector('[aria-haspopup="menu"]')).toBeNull();
    expect(groupTitles()).toEqual([]);
    expect(text()).toContain('SSH · embedded terminal');
  });

  it('fields_are_grouped_per_connection', () => {
    render(['c:h2'], { library: multiEntryLibrary() });
    expect(groupTitles()).toEqual(['SSH · root', 'Console']);
    const groups = [...document.querySelectorAll('.connection-group')];
    expect(groups[0]!.querySelector('[aria-label="Default connection"]')).toBeTruthy();
    expect(groups[1]!.querySelector('[aria-label="Default connection"]')).toBeNull();
    expect(groups[1]!.classList.contains('selected')).toBe(true);
    expect(groups[0]!.classList.contains('selected')).toBe(false);
    expect(groups[1]!.textContent).toContain('3389');
    // Side by side in one scrollable row, so the sections below stay in view.
    expect(document.querySelector('.connection-groups')!.classList.contains('row')).toBe(true);
  });

  it('names the connection in each port forward heading when several SSH connections exist', () => {
    const lib = multiEntryLibrary();
    lib.connections = lib.connections.map((c) =>
      c.id === 'h2' ? { ...c, type: 'ssh' as const, displayLabel: 'SSH · admin' } : c,
    );
    render(['e:e1'], { library: lib });
    const headings = [...document.querySelectorAll('h2')].map((h) => h.textContent?.trim());
    expect(headings.filter((h) => h?.startsWith('Port forwards'))).toEqual([
      'Port forwards · SSH · root',
      'Port forwards · SSH · admin',
    ]);
  });

  it('a single connection keeps the plain stacked layout', () => {
    render(['e:c1'], { library: multiEntryLibrary() });
    expect(document.querySelector('.connection-groups')!.classList.contains('row')).toBe(false);
  });

  it('Edit opens the selected connection’s tab, General for an entry row, its tab for one connection', () => {
    const { editor, store } = render(['c:h2'], { library: multiEntryLibrary() });
    click(buttonByText('Edit'));
    expect(editor.entry).toEqual({ mode: 'edit', id: 'e1', connectionId: 'h2' });
    store.setSelection(['e:e1']);
    flushSync();
    click(buttonByText('Edit'));
    expect(editor.entry).toEqual({ mode: 'edit', id: 'e1' });
    store.setSelection(['e:c1']);
    flushSync();
    click(buttonByText('Edit'));
    expect(editor.entry).toEqual({ mode: 'edit', id: 'c1', connectionId: 'c1' });
  });

  it('offers Merge… unless read-only', () => {
    const { deps } = render(['e:e1'], { library: multiEntryLibrary() });
    click(buttonByText('Merge…'));
    expect(deps.actions.mergeTarget).toBe('e1');
    cleanupView();
    render(['e:e1'], { library: multiEntryLibrary(), readOnly: true });
    expect(() => buttonByText('Merge…')).toThrow();
  });
});

describe('CredentialDetail', () => {
  /** k1 is used by db (c3) and router (c4). */
  function credentialLibrary(): LibraryDto {
    const library = sidebarLibrary();
    library.connections[3] = { ...library.connections[3]!, credentialId: 'k1' };
    library.credentials[0] = { ...library.credentials[0]!, usageCount: 2, hasUnlockedKey: true };
    return library;
  }

  it('filters "Used by" with matching, selects the visible rows and unlinks them', async () => {
    let filter: string[] | null = null;
    const { bridge, store } = render(['k:k1'], {
      library: credentialLibrary(),
      replies: {
        matching: (args?: Record<string, unknown>) => filter ?? (args?.connectionIds as string[]),
      },
    });
    await settle();
    expect(text()).toContain('Used by (2)');
    expect(text()).toContain('Unlocked key');

    filter = ['c4'];
    type(document.querySelector<HTMLInputElement>('input[placeholder="Name, host, user or folder"]')!, 'rou');
    await settle();
    expect(bridge.invoke).toHaveBeenLastCalledWith('matching', { connectionIds: ['c3', 'c4'], query: 'rou' });
    const selectAll = [...document.querySelectorAll<HTMLInputElement>('label.checkbox input')][0]!;
    check(selectAll, true);
    expect(buttonByText('Unlink 1')).toBeTruthy();

    click(buttonByText('Unlink 1'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('set_credential', {
      credentialId: null,
      tags: ['c:c4'],
      connectionIds: null,
    });

    click(buttonByText('router'));
    expect(store.selection).toEqual(['e:c4']);
  });

  it('shows the empty state of an unused credential', async () => {
    render(['k:k2'], { replies: { matching: [] } });
    await settle();
    expect(text()).toContain('Used by (0)');
    expect(text()).toContain('No connections');
  });
});
