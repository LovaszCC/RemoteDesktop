import type {
  AppStatusDto,
  ConnectionDto,
  CredentialDto,
  EntryDto,
  FolderDto,
  LibraryDto,
  SettingsDto,
  ShortcutDto,
} from '../types';

export function status(overrides: Partial<AppStatusDto> = {}): AppStatusDto {
  return {
    phase: { kind: 'locked', needsSetup: false },
    databasePath: '/data/connections.sqlite',
    readOnly: false,
    rememberMasterKey: true,
    platform: 'macos',
    handoverNotice: false,
    ...overrides,
  };
}

/** An SSH connection that is its own one-connection entry (`entryId === id`, as upgraded libraries). */
export function connection(id: string, name = id): ConnectionDto {
  return {
    id,
    entryId: id,
    label: '',
    displayLabel: 'SSH',
    showAsButton: true,
    sortOrder: 0,
    webScheme: 'https',
    webPath: '',
    folderId: null,
    credentialId: null,
    type: 'ssh',
    name,
    host: `${id}.example.test`,
    port: null,
    effectivePort: 22,
    webUrl: null,
    username: '',
    domain: '',
    hasPassword: false,
    sshKeyPath: null,
    hasPassphrase: false,
    resolved: { username: '', domain: '', hasPassword: false, sshKeyPath: null, hasPassphrase: false },
    isFavorite: false,
    rdpAdminSession: false,
    browserBundleId: null,
    mikrotikMode: 'winbox',
    sshTerminal: 'embedded',
    rdpClient: 'embedded',
    color: null,
    notes: '',
    folderPath: '',
    createdAt: '2026-01-01 00:00:00.000',
    updatedAt: '2026-01-01 00:00:00.000',
  };
}

/** The entry of `connections` (its shared fields taken from the first); the first is the default. */
export function entry(
  id: string,
  connections: readonly ConnectionDto[],
  overrides: Partial<EntryDto> = {},
): EntryDto {
  const first = connections[0];
  return {
    id,
    folderId: first?.folderId ?? null,
    folderPath: first?.folderPath ?? '',
    name: first?.name ?? id,
    host: first?.host ?? '',
    notes: first?.notes ?? '',
    color: first?.color ?? null,
    isFavorite: first?.isFavorite ?? false,
    defaultConnectionId: first?.id ?? null,
    connectionIds: connections.map((c) => c.id),
    vpn: null,
    ...overrides,
  };
}

/** One entry per connection, keyed by `entryId`, in the connections' order. */
export function entriesOf(connections: readonly ConnectionDto[]): EntryDto[] {
  const ids = [...new Set(connections.map((c) => c.entryId))];
  return ids.map((id) =>
    entry(
      id,
      connections.filter((c) => c.entryId === id),
    ),
  );
}

export function libraryOf(connectionIds: string[], folderIds: string[] = []): LibraryDto {
  return {
    folders: folderIds.map((id, i) => ({
      id,
      parentId: null,
      name: id,
      sortOrder: i,
      color: null,
      path: id,
    })),
    entries: connectionIds.map((id) => entry(id, [connection(id)])),
    connections: connectionIds.map((id) => connection(id)),
    credentials: [],
    portForwards: [],
    savedSplits: [],
    tree: connectionIds.map((id) => ({ kind: 'entry' as const, id })),
    favorites: [],
    flattenedFolders: folderIds.map((id) => ({ id, depth: 0 })),
  };
}

export function settingsDto(overrides: Partial<SettingsDto> = {}): SettingsDto {
  return {
    databasePath: '/data/connections.sqlite',
    language: 'en',
    rememberMasterKey: true,
    externalTerminal: null,
    checkForUpdatesAutomatically: true,
    lastUpdateCheck: null,
    expandedFolders: [],
    winboxPath: null,
    selectionShows: 'details',
    showInSystemSearch: false,
    ...overrides,
  };
}

export function shortcutRow(command: ShortcutDto['command'], customized = false): ShortcutDto {
  return {
    command,
    group: 'file',
    shortcut: { key: 'n', modifiers: 1 },
    display: '⌘N',
    defaultDisplay: '⌘N',
    customized,
  };
}

/**
 * A small library with a folder tree, every connection type, favourites, credentials and saved splits:
 *
 *   Office (f1)            ← red
 *     Servers (f2)
 *       db (c3, ssh)
 *     web01 (c2, rdp, favourite)
 *   Lab (f3)
 *   router (c4, mikrotik, root)
 *   portal (c5, web, root)
 *   box (c1, ssh, root, favourite)
 *
 * Every connection is its own one-connection entry with the same id.
 */
export function sidebarLibrary(): LibraryDto {
  const folder = (id: string, name: string, parentId: string | null, path: string): FolderDto => ({
    id,
    parentId,
    name,
    sortOrder: 0,
    color: null,
    path,
  });
  const conn = (
    id: string,
    name: string,
    type: ConnectionDto['type'],
    folderId: string | null,
    path = '',
  ): ConnectionDto => ({
    ...connection(id, name),
    type,
    displayLabel: typeLabel(type),
    folderId,
    folderPath: path,
  });
  const credential = (id: string, name: string, usageCount: number): CredentialDto => ({
    id,
    name,
    username: 'admin',
    domain: '',
    displayUser: 'admin',
    hasPassword: true,
    sshKeyPath: null,
    hasPassphrase: false,
    hasUnlockedKey: false,
    notes: '',
    usageCount,
    createdAt: '2026-01-01 00:00:00.000',
    updatedAt: '2026-01-01 00:00:00.000',
  });
  const connections: ConnectionDto[] = [
    { ...conn('c1', 'box', 'ssh', null), isFavorite: true },
    { ...conn('c2', 'web01', 'rdp', 'f1', 'Office'), isFavorite: true, color: 'blue' },
    { ...conn('c3', 'db', 'ssh', 'f2', 'Office / Servers'), credentialId: 'k1' },
    conn('c4', 'router', 'mikrotik', null),
    conn('c5', 'portal', 'web', null),
  ];
  return {
    folders: [
      { ...folder('f1', 'Office', null, 'Office'), color: 'red' },
      folder('f2', 'Servers', 'f1', 'Office / Servers'),
      folder('f3', 'Lab', null, 'Lab'),
    ],
    entries: entriesOf(connections),
    connections,
    credentials: [credential('k1', 'Domain admin', 1), credential('k2', 'Spare', 0)],
    portForwards: [],
    savedSplits: [
      {
        id: 's1',
        name: 'Pair',
        connectionIds: ['c1', 'c3'],
        sortOrder: 0,
        createdAt: '2026-01-01 00:00:00.000',
      },
      { id: 's2', name: 'Gone', connectionIds: ['zz'], sortOrder: 1, createdAt: '2026-01-01 00:00:00.000' },
    ],
    tree: [
      {
        kind: 'folder',
        id: 'f1',
        children: [
          { kind: 'folder', id: 'f2', children: [{ kind: 'entry', id: 'c3' }] },
          { kind: 'entry', id: 'c2' },
        ],
      },
      { kind: 'folder', id: 'f3', children: [] },
      { kind: 'entry', id: 'c1' },
      { kind: 'entry', id: 'c5' },
      { kind: 'entry', id: 'c4' },
    ],
    favorites: ['c1', 'c2'],
    flattenedFolders: [
      { id: 'f3', depth: 0 },
      { id: 'f1', depth: 0 },
      { id: 'f2', depth: 1 },
    ],
  };
}

function typeLabel(type: ConnectionDto['type']): string {
  return { rdp: 'RDP', ssh: 'SSH', web: 'Web', mikrotik: 'MikroTik' }[type];
}

/**
 * `sidebarLibrary()` plus "Home Server" (e1, root, between "box" and "portal") with two connections:
 * h1 (SSH as root, the default) and h2 (RDP labelled "Console").
 */
export function multiEntryLibrary(): LibraryDto {
  const base = sidebarLibrary();
  const shared = { ...connection('h1', 'Home Server'), entryId: 'e1', host: 'home.example.test' };
  const h1: ConnectionDto = {
    ...shared,
    username: 'root',
    displayLabel: 'SSH · root',
    resolved: { ...shared.resolved, username: 'root' },
  };
  const h2: ConnectionDto = {
    ...shared,
    id: 'h2',
    type: 'rdp',
    effectivePort: 3389,
    label: 'Console',
    displayLabel: 'Console',
    sortOrder: 1,
  };
  const tree = [...base.tree];
  tree.splice(
    tree.findIndex((n) => n.id === 'c5'),
    0,
    { kind: 'entry', id: 'e1' },
  );
  return {
    ...base,
    entries: [...base.entries, entry('e1', [h1, h2])],
    connections: [...base.connections, h1, h2],
    tree,
  };
}
