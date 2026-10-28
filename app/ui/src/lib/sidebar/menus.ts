// The sidebar's context menus and the toolbar "New" menu (Swift `SidebarView.contextMenu`,
// `ItemContextMenu`, `MoveMenu`, `LinkCredentialMenu`, `MainView` toolbar). Pure builders over the
// library so the item sets are testable; read-only mode leaves out everything that writes.

import { t } from '../i18n/index.svelte';
import type { MenuItem } from '../components/menu.svelte';
import { connectMenuItems } from '../detail/model';
import { connectionOfTag, isEntryKind, parseTag } from '../stores/library.svelte';
import {
  LABEL_COLORS,
  type ConnectionDto,
  type EntryDto,
  type ImportKind,
  type LibraryDto,
  type Platform,
} from '../types';
import { MAX_PANES } from '../terminal/split';
import type { SidebarActions } from './actions.svelte';
import {
  CONNECTION_TYPES,
  NEW_TITLES,
  colorTitle,
  connectionIdsForTags,
  sshConnections,
  subtreeFolderIds,
  usesSSH,
} from './model';

export type MenuActions = Pick<
  SidebarActions,
  | 'connect'
  | 'newConnection'
  | 'newFolder'
  | 'newCredential'
  | 'editEntry'
  | 'editCredential'
  | 'renameFolder'
  | 'duplicate'
  | 'merge'
  | 'setFavorite'
  | 'deleteConnection'
  | 'deleteEntry'
  | 'copyPassword'
  | 'copyHost'
  | 'openFiles'
  | 'setupSshKey'
  | 'settingUpKey'
  | 'move'
  | 'setFolderColor'
  | 'deleteFolder'
  | 'deleteCredential'
  | 'setCredential'
  | 'deleteItems'
  | 'openSavedSplit'
  | 'openSplit'
  | 'saveAsSplit'
  | 'renameSavedSplit'
  | 'deleteSavedSplit'
  | 'releaseVpn'
>;

export interface MenuContext {
  library: LibraryDto;
  readOnly: boolean;
  /** The external terminal's name for "Open in …". */
  terminalName: string;
  /** For the system RDP client's name ("Windows App" on macOS). */
  platform?: Platform;
  /** Where new items from the empty-area menu go. */
  selectedFolderId: string | null;
  /** The VPN an external launch of the entry holds (macOS), for "Release VPN". */
  externalVpn?: (entryId: string) => string | null;
  actions: MenuActions;
}

const separator: MenuItem = { kind: 'separator' };

/** Drops leading, trailing and doubled separators (left behind by read-only filtering). */
export function tidy(items: MenuItem[]): MenuItem[] {
  const out: MenuItem[] = [];
  for (const item of items) {
    if (item.kind === 'separator' && (out.length === 0 || out[out.length - 1]?.kind === 'separator'))
      continue;
    out.push(item);
  }
  while (out[out.length - 1]?.kind === 'separator') out.pop();
  return out;
}

/**
 * The toolbar "New" menu and the empty-area menu: the four connection types, then New folder and New
 * credential. New items go into `folderId` (the selected folder, as in Swift `selectedFolderId`).
 */
export function newMenu(
  ctx: Pick<MenuContext, 'readOnly' | 'actions'>,
  folderId: string | null,
  separated = true,
): MenuItem[] {
  if (ctx.readOnly) return [];
  const { actions } = ctx;
  return [
    ...CONNECTION_TYPES.map((type) => ({
      label: t(NEW_TITLES[type]),
      action: () => actions.newConnection(type, folderId),
    })),
    ...(separated ? [separator] : []),
    { label: t('New folder'), action: () => actions.newFolder(folderId) },
    { label: t('New credential'), action: () => actions.newCredential() },
  ];
}

/**
 * The toolbar "Import" menu (Swift `MainView` toolbar): RDM and WinBox, with the effective shortcuts as
 * hints. Empty in read-only mode.
 */
export function importMenu(
  readOnly: boolean,
  start: (kind: ImportKind) => void,
  shortcuts: Partial<Record<ImportKind, string | null>> = {},
  transfer?: { importExport: () => void; importCsv: () => void; exportLibrary: () => void },
): MenuItem[] {
  const own: MenuItem[] = transfer
    ? [
        { label: t('Import from RemoteDesktop export…'), action: transfer.importExport },
        { label: t('Import from CSV…'), action: transfer.importCsv },
        { kind: 'separator' },
        { label: t('Export…'), action: transfer.exportLibrary },
      ]
    : [];
  if (readOnly) return own.slice(-1);
  return [
    { label: t('Import from RDM…'), shortcut: shortcuts.rdm ?? undefined, action: () => start('rdm') },
    {
      label: t('Import from WinBox…'),
      shortcut: shortcuts.winbox ?? undefined,
      action: () => start('winbox'),
    },
    ...own,
  ];
}

/** Right-click on the empty part of the sidebar (no divider, as in Swift). */
export function emptyAreaMenu(ctx: MenuContext): MenuItem[] {
  return newMenu(ctx, ctx.selectedFolderId, false);
}

/**
 * "Move ▸": "(Root)" (disabled when already there), then every folder indented by depth, the current
 * one disabled. `excluding` leaves out a folder's own subtree; `current` is `undefined` for a mixed
 * selection (nothing disabled).
 */
export function moveMenu(
  library: LibraryDto,
  current: string | null | undefined,
  excluding: string | null,
  move: (folderId: string | null) => void,
): MenuItem {
  const skip = excluding ? subtreeFolderIds(library.folders, excluding) : new Set<string>();
  const names = new Map(library.folders.map((f) => [f.id, f.name]));
  const folders = library.flattenedFolders.filter((f) => !skip.has(f.id));
  return {
    label: t('Move'),
    submenu: [
      { label: t('(Root)'), disabled: current === null, action: () => move(null) },
      ...folders.map((f) => ({
        // Non-breaking spaces: the menu collapses ordinary ones.
        label: '   '.repeat(f.depth) + (names.get(f.id) ?? ''),
        disabled: f.id === current,
        action: () => move(f.id),
      })),
    ],
  };
}

/**
 * The menu of a one-connection entry row (`e:`, `fav:`: its connection; Merge…; Delete removes the whole
 * entry) or of a child row (`c:`: that connection; Delete removes only it).
 */
function connectionMenu(ctx: MenuContext, connection: ConnectionDto, tag: string): MenuItem[] {
  const isChild = parseTag(tag)?.kind === 'connection';
  const { actions, readOnly, library } = ctx;
  const items: MenuItem[] = [];
  if (connection.type === 'mikrotik') {
    items.push(
      {
        label: t('Connect – %@', 'WinBox'),
        action: () => void actions.connect(connection, { mode: 'winbox' }),
      },
      { label: t('Connect – %@', 'SSH'), action: () => void actions.connect(connection, { mode: 'ssh' }) },
    );
  } else {
    items.push({ label: t('Connect'), action: () => void actions.connect(connection) });
  }
  if (connection.type === 'rdp') {
    items.push(
      {
        label: t('New embedded RDP session'),
        action: () => void actions.connect(connection, { rdpClient: 'embedded' }),
      },
      {
        label:
          (ctx.platform ?? 'macos') === 'macos' ? t('Open in Windows App') : t('Open in system RDP client'),
        action: () => void actions.connect(connection, { rdpClient: 'windowsApp' }),
      },
    );
  }
  if (usesSSH(connection.type)) {
    items.push(
      {
        label: t('New embedded SSH session'),
        action: () => void actions.connect(connection, { mode: 'ssh', terminal: 'embedded' }),
      },
      {
        label: t('Open in split view (2×)'),
        action: () => actions.openSplit([connection, connection]),
      },
      { label: t('Files (SCP)…'), action: () => void actions.openFiles(connection.id) },
      {
        label: t('Open in %@', ctx.terminalName),
        action: () => void actions.connect(connection, { mode: 'ssh', terminal: 'external' }),
      },
    );
  }
  if (connection.type === 'ssh') {
    items.push({
      label: t('Set up SSH key login'),
      disabled: actions.settingUpKey(connection.id),
      action: () => void actions.setupSshKey(connection),
    });
  }
  items.push(...releaseVpnItems(ctx, connection.entryId), separator);
  if (!readOnly) {
    items.push(
      { label: t('Edit…'), action: () => actions.editEntry(connection.entryId, connection.id) },
      { label: t('Duplicate'), action: () => void actions.duplicate(connection.entryId) },
      ...(isChild ? [] : [{ label: t('Merge…'), action: () => actions.merge(connection.entryId) }]),
      {
        label: connection.isFavorite ? t('Remove from favorites') : t('Add to favorites'),
        action: () => void actions.setFavorite(connection.id, !connection.isFavorite),
      },
      moveMenu(
        library,
        connection.folderId,
        null,
        (to) => void actions.move([`e:${connection.entryId}`], to),
      ),
      separator,
    );
  }
  items.push(
    { label: t('Copy password'), action: () => void actions.copyPassword(connection.id) },
    { label: t('Copy host'), action: () => void actions.copyHost(connection) },
  );
  if (!readOnly) {
    items.push(separator, {
      label: t('Delete'),
      destructive: true,
      action: () =>
        void (isChild ? actions.deleteConnection(connection.id) : actions.deleteEntry(connection.entryId)),
    });
  }
  return items;
}

/** "Release VPN "…"" when an external launch of the entry holds a VPN (macOS). */
function releaseVpnItems(ctx: MenuContext, entryId: string): MenuItem[] {
  if ((ctx.platform ?? 'macos') !== 'macos') return [];
  const profile = ctx.externalVpn?.(entryId) ?? null;
  return profile
    ? [{ label: t('Release VPN "%@"', profile), action: () => void ctx.actions.releaseVpn(entryId) }]
    : [];
}

/**
 * The menu of an entry row with several connections: Connect ▸ (each connection, the default checked),
 * then the entry's own items. The connection-specific items live on the child rows.
 */
function entryMenu(ctx: MenuContext, entry: EntryDto, connections: ConnectionDto[]): MenuItem[] {
  const { actions, readOnly, library } = ctx;
  const items: MenuItem[] = [
    {
      label: t('Connect'),
      submenu: connectMenuItems(connections, entry.defaultConnectionId, (c) => void actions.connect(c)),
    },
    ...releaseVpnItems(ctx, entry.id),
    separator,
  ];
  if (!readOnly) {
    const editId = entry.defaultConnectionId ?? connections[0]!.id;
    items.push(
      { label: t('Edit…'), action: () => actions.editEntry(entry.id) },
      { label: t('Duplicate'), action: () => void actions.duplicate(entry.id) },
      { label: t('Merge…'), action: () => actions.merge(entry.id) },
      {
        label: entry.isFavorite ? t('Remove from favorites') : t('Add to favorites'),
        action: () => void actions.setFavorite(editId, !entry.isFavorite),
      },
      moveMenu(library, entry.folderId, null, (to) => void actions.move([`e:${entry.id}`], to)),
      separator,
    );
  }
  items.push({ label: t('Copy host'), action: () => void actions.copyHost(connections[0]!) });
  if (!readOnly) {
    items.push(separator, {
      label: t('Delete'),
      destructive: true,
      action: () => void actions.deleteEntry(entry.id),
    });
  }
  return items;
}

/** The menu of one row (`f:`, `e:`, `c:`, `fav:`, `k:` or `s:`); empty when the item is gone. */
export function itemMenu(ctx: MenuContext, tag: string): MenuItem[] {
  const parsed = parseTag(tag);
  if (!parsed) return [];
  const { library, actions, readOnly } = ctx;
  switch (parsed.kind) {
    case 'entry':
    case 'favorite':
    case 'connection': {
      const entry = isEntryKind(parsed.kind) ? library.entries.find((e) => e.id === parsed.id) : undefined;
      const connections = (entry?.connectionIds ?? []).flatMap(
        (id) => library.connections.find((c) => c.id === id) ?? [],
      );
      if (entry && connections.length > 1) return tidy(entryMenu(ctx, entry, connections));
      const connection = connectionOfTag(library, tag);
      return connection ? tidy(connectionMenu(ctx, connection, tag)) : [];
    }
    case 'folder': {
      const folder = library.folders.find((f) => f.id === parsed.id);
      if (!folder || readOnly) return [];
      return [
        ...CONNECTION_TYPES.map((type) => ({
          label: t('%@ here', t(NEW_TITLES[type])),
          action: () => actions.newConnection(type, folder.id),
        })),
        { label: t('New subfolder'), action: () => actions.newFolder(folder.id) },
        separator,
        { label: t('Rename…'), action: () => actions.renameFolder(folder.id) },
        {
          label: t('Color'),
          submenu: [
            {
              label: t('None'),
              checked: folder.color === null,
              action: () => void actions.setFolderColor(folder.id, null),
            },
            separator,
            ...LABEL_COLORS.map((color) => ({
              label: colorTitle(color),
              checked: folder.color === color,
              action: () => void actions.setFolderColor(folder.id, color),
            })),
          ],
        },
        moveMenu(library, folder.parentId, folder.id, (to) => void actions.move([`f:${folder.id}`], to)),
        separator,
        { label: t('Delete folder'), destructive: true, action: () => void actions.deleteFolder(folder.id) },
      ];
    }
    case 'credential': {
      const credential = library.credentials.find((c) => c.id === parsed.id);
      if (!credential || readOnly) return [];
      return [
        { label: t('Edit…'), action: () => actions.editCredential(credential.id) },
        separator,
        {
          label: t('Delete credential'),
          destructive: true,
          action: () => void actions.deleteCredential(credential.id),
        },
      ];
    }
    case 'split': {
      const split = library.savedSplits.find((s) => s.id === parsed.id);
      if (!split) return [];
      const open: MenuItem = { label: t('Open'), action: () => actions.openSavedSplit(split.id) };
      if (readOnly) return [open];
      return [
        open,
        { label: t('Rename…'), action: () => void actions.renameSavedSplit(split.id) },
        separator,
        {
          label: t('Delete saved split'),
          destructive: true,
          action: () => void actions.deleteSavedSplit(split.id),
        },
      ];
    }
  }
}

/** The menu of a multi-selection. */
export function multiMenu(ctx: MenuContext, tags: string[]): MenuItem[] {
  const { library, actions, readOnly } = ctx;
  const connections = sshConnections(library, tags);
  const paneCount = Math.min(connections.length, MAX_PANES);
  const items: MenuItem[] = [
    {
      label: t('Open in split view (%lld)', paneCount),
      disabled: connections.length < 2,
      action: () => actions.openSplit(connections),
    },
  ];
  if (readOnly) return items;
  const linkable = connectionIdsForTags(library, tags).length > 0;
  items.push(
    {
      label: t('Save as split…'),
      disabled: connections.length < 2 || connections.length > MAX_PANES,
      action: () => void actions.saveAsSplit(connections),
    },
    moveMenu(library, undefined, null, (to) => void actions.move(tags, to)),
    {
      label: t('Linked credential'),
      disabled: !linkable,
      submenu: tidy([
        { label: t('Own login'), action: () => void actions.setCredential(null, tags) },
        separator,
        ...library.credentials.map((c) => ({
          label: c.name,
          action: () => void actions.setCredential(c.id, tags),
        })),
      ]),
    },
    separator,
    {
      label: t('Delete %lld items…', tags.length),
      destructive: true,
      action: () => void actions.deleteItems(tags),
    },
  );
  return items;
}

/** The context menu for the tags it acts on: none (empty area), one row, or several. */
export function sidebarMenu(ctx: MenuContext, tags: string[]): MenuItem[] {
  if (tags.length > 1) return multiMenu(ctx, tags);
  if (tags.length === 1) return itemMenu(ctx, tags[0]!);
  return emptyAreaMenu(ctx);
}

/** The fixed "Local Terminal" entry's context menu. */
export function localTerminalMenu(actions: Pick<SidebarActions, 'openLocalTerminal'>): MenuItem[] {
  return [{ label: t('Open'), action: () => void actions.openLocalTerminal() }];
}
