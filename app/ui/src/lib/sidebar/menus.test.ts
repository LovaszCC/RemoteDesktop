import { describe, expect, it, vi } from 'vitest';
import type { MenuItem } from '../components/menu.svelte';
import { multiEntryLibrary, sidebarLibrary } from '../testing/fixtures';
import {
  emptyAreaMenu,
  importMenu,
  itemMenu,
  multiMenu,
  newMenu,
  sidebarMenu,
  tidy,
  type MenuActions,
  type MenuContext,
} from './menus';

function fakeActions(): MenuActions {
  const names = [
    'connect',
    'newConnection',
    'newFolder',
    'newCredential',
    'editEntry',
    'editCredential',
    'renameFolder',
    'duplicate',
    'merge',
    'setFavorite',
    'deleteConnection',
    'deleteEntry',
    'copyPassword',
    'copyHost',
    'openFiles',
    'setupSshKey',
    'settingUpKey',
    'move',
    'setFolderColor',
    'deleteFolder',
    'deleteCredential',
    'setCredential',
    'deleteItems',
    'openSavedSplit',
    'openSplit',
    'saveAsSplit',
    'renameSavedSplit',
    'deleteSavedSplit',
  ];
  return Object.fromEntries(names.map((n) => [n, vi.fn()])) as unknown as MenuActions;
}

function context(overrides: Partial<MenuContext> = {}): MenuContext {
  return {
    library: sidebarLibrary(),
    readOnly: false,
    terminalName: 'Terminal',
    selectedFolderId: null,
    actions: fakeActions(),
    ...overrides,
  };
}

/** Labels with `-` for separators, `!` for disabled items and `✓` for checked ones. */
function labels(items: MenuItem[]): string[] {
  return items.map((item) =>
    item.kind === 'separator' ? '-' : `${item.disabled ? '!' : ''}${item.checked ? '✓' : ''}${item.label}`,
  );
}

function find(items: MenuItem[], label: string) {
  const item = items.find((i) => i.kind !== 'separator' && i.label === label);
  if (!item || item.kind === 'separator') throw new Error(`no item ${label}`);
  return item;
}

describe('sidebar menus', () => {
  it('empty area and toolbar offer the new items', () => {
    const ctx = context({ selectedFolderId: 'f1' });
    expect(labels(emptyAreaMenu(ctx))).toEqual([
      'New RDP connection',
      'New SSH connection',
      'New web connection',
      'New MikroTik connection',
      'New folder',
      'New credential',
    ]);
    find(emptyAreaMenu(ctx), 'New web connection').action!();
    expect(ctx.actions.newConnection).toHaveBeenCalledWith('web', 'f1');
    const toolbar = newMenu(ctx, null);
    expect(labels(toolbar)[4]).toBe('-');
    find(toolbar, 'New folder').action!();
    expect(ctx.actions.newFolder).toHaveBeenCalledWith(null);
    expect(newMenu({ ...ctx, readOnly: true }, null)).toEqual([]);
    expect(labels(sidebarMenu(ctx, []))).toEqual(labels(emptyAreaMenu(ctx)));
  });

  it('an RDP connection', () => {
    const ctx = context();
    find(itemMenu(ctx, 'fav:c2'), 'Open in Windows App').action!();
    expect(ctx.actions.connect).toHaveBeenCalledWith(expect.objectContaining({ id: 'c2' }), {
      rdpClient: 'windowsApp',
    });
    find(itemMenu(ctx, 'fav:c2'), 'New embedded RDP session').action!();
    expect(ctx.actions.connect).toHaveBeenLastCalledWith(expect.objectContaining({ id: 'c2' }), {
      rdpClient: 'embedded',
    });
    expect(labels(itemMenu(context({ platform: 'windows' }), 'fav:c2'))[2]).toBe('Open in system RDP client');
    expect(labels(itemMenu(context({ platform: 'linux' }), 'fav:c2'))[2]).toBe('Open in system RDP client');
    expect(labels(itemMenu(context(), 'fav:c2'))).toEqual([
      'Connect',
      'New embedded RDP session',
      'Open in Windows App',
      '-',
      'Edit…',
      'Duplicate',
      'Merge…',
      'Remove from favorites',
      'Move',
      '-',
      'Copy password',
      'Copy host',
      '-',
      'Delete',
    ]);
  });

  it('a MikroTik connection has a Connect per mode and the SSH items', () => {
    expect(labels(itemMenu(context(), 'e:c4')).slice(0, 7)).toEqual([
      'Connect – WinBox',
      'Connect – SSH',
      'New embedded SSH session',
      'Open in split view (2×)',
      'Files (SCP)…',
      'Open in Terminal',
      '-',
    ]);
    expect(labels(itemMenu(context(), 'e:c4'))).toContain('Add to favorites');
  });

  it('the connect items pass the MikroTik mode and the SSH terminal', () => {
    const ctx = context();
    const menu = itemMenu(ctx, 'e:c4');
    const connection = ctx.library.connections.find((c) => c.id === 'c4');
    find(menu, 'Connect – WinBox').action!();
    expect(ctx.actions.connect).toHaveBeenLastCalledWith(connection, { mode: 'winbox' });
    find(menu, 'Connect – SSH').action!();
    expect(ctx.actions.connect).toHaveBeenLastCalledWith(connection, { mode: 'ssh' });
    find(menu, 'New embedded SSH session').action!();
    expect(ctx.actions.connect).toHaveBeenLastCalledWith(connection, { mode: 'ssh', terminal: 'embedded' });
    find(menu, 'Open in Terminal').action!();
    expect(ctx.actions.connect).toHaveBeenLastCalledWith(connection, { mode: 'ssh', terminal: 'external' });
    find(menu, 'Files (SCP)…').action!();
    expect(ctx.actions.openFiles).toHaveBeenCalledWith('c4');
  });

  it('a read-only connection keeps connect and copy only', () => {
    expect(labels(itemMenu(context({ readOnly: true }), 'e:c5'))).toEqual([
      'Connect',
      '-',
      'Copy password',
      'Copy host',
    ]);
  });

  it('Move disables the current folder, or the root when already there', () => {
    const ctx = context();
    const atRoot = find(itemMenu(ctx, 'e:c1'), 'Move').submenu!;
    expect(labels(atRoot)).toEqual(['!(Root)', 'Lab', 'Office', '   Servers']);
    const inOffice = find(itemMenu(ctx, 'e:c2'), 'Move').submenu!;
    expect(labels(inOffice)).toEqual(['(Root)', 'Lab', '!Office', '   Servers']);
    find(inOffice, 'Lab').action!();
    expect(ctx.actions.move).toHaveBeenCalledWith(['e:c2'], 'f3');
  });

  it('a folder: new items here, rename, colour with a check, move without its subtree, delete', () => {
    const ctx = context();
    const menu = itemMenu(ctx, 'f:f1');
    expect(labels(menu)).toEqual([
      'New RDP connection here',
      'New SSH connection here',
      'New web connection here',
      'New MikroTik connection here',
      'New subfolder',
      '-',
      'Rename…',
      'Color',
      'Move',
      '-',
      'Delete folder',
    ]);
    expect(labels(find(menu, 'Color').submenu!)).toEqual([
      'None',
      '-',
      '✓Red',
      'Orange',
      'Yellow',
      'Green',
      'Blue',
      'Purple',
      'Pink',
      'Gray',
    ]);
    expect(labels(find(menu, 'Move').submenu!)).toEqual(['!(Root)', 'Lab']);
    find(menu, 'New subfolder').action!();
    expect(ctx.actions.newFolder).toHaveBeenCalledWith('f1');
    find(find(menu, 'Color').submenu!, 'None').action!();
    expect(ctx.actions.setFolderColor).toHaveBeenCalledWith('f1', null);
    expect(itemMenu(context({ readOnly: true }), 'f:f1')).toEqual([]);
  });

  it('a credential and a saved split', () => {
    expect(labels(itemMenu(context(), 'k:k1'))).toEqual(['Edit…', '-', 'Delete credential']);
    expect(labels(itemMenu(context(), 's:s1'))).toEqual(['Open', 'Rename…', '-', 'Delete saved split']);
    expect(labels(itemMenu(context({ readOnly: true }), 's:s1'))).toEqual(['Open']);
    expect(itemMenu(context(), 'c:missing')).toEqual([]);
  });

  it('a multi-selection', () => {
    const ctx = context();
    const tags = ['fav:c1', 'e:c3', 'e:c2'];
    const menu = multiMenu(ctx, tags);
    expect(labels(menu)).toEqual([
      'Open in split view (2)',
      'Save as split…',
      'Move',
      'Linked credential',
      '-',
      'Delete 3 items…',
    ]);
    expect(labels(find(menu, 'Move').submenu!)[0]).toBe('(Root)');
    expect(labels(find(menu, 'Linked credential').submenu!)).toEqual([
      'Own login',
      '-',
      'Domain admin',
      'Spare',
    ]);
    find(find(menu, 'Linked credential').submenu!, 'Spare').action!();
    expect(ctx.actions.setCredential).toHaveBeenCalledWith('k2', tags);
    find(menu, 'Save as split…').action!();
    expect(vi.mocked(ctx.actions.saveAsSplit).mock.calls[0]![0].map((c) => c.id)).toEqual(['c1', 'c3']);
    find(menu, 'Open in split view (2)').action!();
    expect(vi.mocked(ctx.actions.openSplit).mock.calls[0]![0].map((c) => c.id)).toEqual(['c1', 'c3']);
    find(menu, 'Delete 3 items…').action!();
    expect(ctx.actions.deleteItems).toHaveBeenCalledWith(tags);
    expect(sidebarMenu(ctx, tags)).toHaveLength(menu.length);
  });

  it('disables Save as split outside 2–4 SSH connections and Linked credential without connections', () => {
    const one = multiMenu(context(), ['e:c1', 'e:c2']);
    expect(labels(one).slice(0, 2)).toEqual(['!Open in split view (1)', '!Save as split…']);
    const none = multiMenu(context(), ['f:f3', 'k:k1']);
    expect(labels(none)).toContain('!Linked credential');
    expect(labels(multiMenu(context({ readOnly: true }), ['e:c1', 'e:c3']))).toEqual([
      'Open in split view (2)',
    ]);
  });

  it('tidies separators', () => {
    const sep = { kind: 'separator' } as const;
    expect(labels(tidy([sep, { label: 'a' }, sep, sep, { label: 'b' }, sep]))).toEqual(['a', '-', 'b']);
  });
});

describe('importMenu', () => {
  it('offers RDM and WinBox with their shortcuts, nothing read-only', () => {
    const start = vi.fn();
    const items = importMenu(false, start, { rdm: '⌘I', winbox: null });
    expect(items.map((i) => (i.kind === 'separator' ? '-' : [i.label, i.shortcut]))).toEqual([
      ['Import from RDM…', '⌘I'],
      ['Import from WinBox…', undefined],
    ]);
    for (const item of items) if (item.kind !== 'separator') item.action?.();
    expect(start.mock.calls).toEqual([['rdm'], ['winbox']]);
    expect(importMenu(true, start)).toEqual([]);
  });

  it('adds the own format and CSV imports, and export (which read-only keeps)', () => {
    const transfer = { importExport: vi.fn(), importCsv: vi.fn(), exportLibrary: vi.fn() };
    const labels = (items: ReturnType<typeof importMenu>) =>
      items.map((i) => (i.kind === 'separator' ? '-' : i.label));
    const items = importMenu(false, vi.fn(), {}, transfer);
    expect(labels(items)).toEqual([
      'Import from RDM…',
      'Import from WinBox…',
      'Import from RemoteDesktop export…',
      'Import from CSV…',
      '-',
      'Export…',
    ]);
    for (const item of items.slice(2)) if (item.kind !== 'separator') item.action?.();
    expect(transfer.importExport).toHaveBeenCalledOnce();
    expect(transfer.importCsv).toHaveBeenCalledOnce();
    expect(transfer.exportLibrary).toHaveBeenCalledOnce();
    expect(labels(importMenu(true, vi.fn(), {}, transfer))).toEqual(['Export…']);
  });

  it('an SSH connection offers SSH key setup, disabled while it runs', () => {
    const ctx = context();
    const connection = ctx.library.connections.find((c) => c.id === 'c1');
    const menu = itemMenu(ctx, 'e:c1');
    expect(labels(menu).slice(4, 7)).toEqual(['Open in Terminal', 'Set up SSH key login', '-']);
    find(menu, 'Set up SSH key login').action!();
    expect(ctx.actions.setupSshKey).toHaveBeenCalledWith(connection);
    expect(ctx.actions.settingUpKey).toHaveBeenCalledWith('c1');
    vi.mocked(ctx.actions.settingUpKey).mockReturnValue(true);
    expect(labels(itemMenu(ctx, 'e:c1'))).toContain('!Set up SSH key login');
    expect(labels(itemMenu(context({ readOnly: true }), 'e:c1'))).toContain('Set up SSH key login');
    // MikroTik, RDP and web connections don't get it.
    for (const tag of ['e:c4', 'fav:c2', 'e:c5']) {
      expect(labels(itemMenu(ctx, tag))).not.toContain('Set up SSH key login');
    }
  });

  it('an entry row acts on its default connection and deletes the entry, a child row only itself', () => {
    const ctx = context({ library: multiEntryLibrary() });
    const entry = itemMenu(ctx, 'e:e1');
    find(entry, 'Duplicate').action?.();
    expect(ctx.actions.duplicate).toHaveBeenCalledWith('e1');
    find(entry, 'Delete').action?.();
    expect(ctx.actions.deleteEntry).toHaveBeenCalledWith('e1');
    const child = itemMenu(ctx, 'c:h2');
    expect(labels(child)).toContain('Open in Windows App');
    expect(labels(child)).not.toContain('Merge…');
    find(child, 'Delete').action?.();
    expect(ctx.actions.deleteConnection).toHaveBeenCalledWith('h2');
    const move = find(child, 'Move');
    const root = move.submenu?.[0];
    if (root && root.kind !== 'separator') root.action?.();
    expect(ctx.actions.move).toHaveBeenCalledWith(['e:e1'], null);
  });

  it('entry_menu_has_merge_and_connect_submenu', () => {
    const ctx = context({ library: multiEntryLibrary() });
    const menu = itemMenu(ctx, 'e:e1');
    expect(labels(menu)).toEqual([
      'Connect',
      '-',
      'Edit…',
      'Duplicate',
      'Merge…',
      'Add to favorites',
      'Move',
      '-',
      'Copy host',
      '-',
      'Delete',
    ]);
    const connect = find(menu, 'Connect').submenu!;
    expect(labels(connect)).toEqual(['✓SSH · root', 'Console']);
    find(connect, 'Console').action!();
    expect(ctx.actions.connect).toHaveBeenCalledWith(expect.objectContaining({ id: 'h2' }));
    find(menu, 'Merge…').action!();
    expect(ctx.actions.merge).toHaveBeenCalledWith('e1');
    // The entry row edits the entry on General; a child row or a one-connection entry on its tab.
    find(menu, 'Edit…').action!();
    expect(ctx.actions.editEntry).toHaveBeenCalledWith('e1');
    find(itemMenu(ctx, 'c:h2'), 'Edit…').action!();
    expect(ctx.actions.editEntry).toHaveBeenLastCalledWith('e1', 'h2');
    const single = context();
    find(itemMenu(single, 'e:c1'), 'Edit…').action!();
    expect(single.actions.editEntry).toHaveBeenCalledWith('c1', 'c1');
    expect(labels(itemMenu(context({ library: multiEntryLibrary(), readOnly: true }), 'fav:e1'))).toEqual([
      'Connect',
      '-',
      'Copy host',
    ]);
  });
});
