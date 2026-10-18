import type {
  AppStatusDto,
  ConnectionDto,
  CredentialDto,
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
    ...overrides,
  };
}

export function connection(id: string, name = id): ConnectionDto {
  return {
    id,
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
    connections: connectionIds.map((id) => connection(id)),
    credentials: [],
    portForwards: [],
    savedSplits: [],
    tree: connectionIds.map((id) => ({ kind: 'connection' as const, id })),
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
  ) => ({
    ...connection(id, name),
    type,
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
  return {
    folders: [
      { ...folder('f1', 'Office', null, 'Office'), color: 'red' },
      folder('f2', 'Servers', 'f1', 'Office / Servers'),
      folder('f3', 'Lab', null, 'Lab'),
    ],
    connections: [
      { ...conn('c1', 'box', 'ssh', null), isFavorite: true },
      { ...conn('c2', 'web01', 'rdp', 'f1', 'Office'), isFavorite: true, color: 'blue' },
      { ...conn('c3', 'db', 'ssh', 'f2', 'Office / Servers'), credentialId: 'k1' },
      conn('c4', 'router', 'mikrotik', null),
      conn('c5', 'portal', 'web', null),
    ],
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
          { kind: 'folder', id: 'f2', children: [{ kind: 'connection', id: 'c3' }] },
          { kind: 'connection', id: 'c2' },
        ],
      },
      { kind: 'folder', id: 'f3', children: [] },
      { kind: 'connection', id: 'c1' },
      { kind: 'connection', id: 'c5' },
      { kind: 'connection', id: 'c4' },
    ],
    favorites: ['c1', 'c2'],
    flattenedFolders: [
      { id: 'f3', depth: 0 },
      { id: 'f1', depth: 0 },
      { id: 'f2', depth: 1 },
    ],
  };
}
