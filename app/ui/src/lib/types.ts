// TypeScript mirrors of `app/src-tauri/src/dto.rs` (the authoritative shapes) and the command inputs.

export type ConnectionType = 'rdp' | 'ssh' | 'web' | 'mikrotik';
export type SshTerminal = 'embedded' | 'external';
export type RdpClient = 'embedded' | 'windowsApp';
export type MikrotikMode = 'winbox' | 'ssh';
export type LabelColor = 'red' | 'orange' | 'yellow' | 'green' | 'blue' | 'purple' | 'pink' | 'gray';
export type Platform = 'macos' | 'windows' | 'linux';
/** `yyyy-MM-dd HH:mm:ss.SSS` in UTC, as the Swift app stores it. */
export type Timestamp = string;

export const LABEL_COLORS: readonly LabelColor[] = [
  'red',
  'orange',
  'yellow',
  'green',
  'blue',
  'purple',
  'pink',
  'gray',
];

export interface ResolvedLoginDto {
  username: string;
  domain: string;
  hasPassword: boolean;
  sshKeyPath: string | null;
  hasPassphrase: boolean;
}

/** One connection of an entry; the shared fields (name, host, folder, notes, colour, favourite) are the
 * entry's, copied onto it. */
export interface ConnectionDto {
  id: string;
  /** The owning entry. */
  entryId: string;
  /** Optional label among the entry's connections; empty when unset. */
  label: string;
  /** The label, else `<TYPE> · <username>`, else `<TYPE>`. */
  displayLabel: string;
  /** Gets its own button on the entry's Details page. */
  showAsButton: boolean;
  /** Position among the entry's connections. */
  sortOrder: number;
  /** Web: the URL scheme. */
  webScheme: string;
  /** Web: the path after host and port; empty for none. */
  webPath: string;
  folderId: string | null;
  credentialId: string | null;
  type: ConnectionType;
  name: string;
  host: string;
  port: number | null;
  effectivePort: number;
  webUrl: string | null;
  username: string;
  domain: string;
  hasPassword: boolean;
  sshKeyPath: string | null;
  hasPassphrase: boolean;
  resolved: ResolvedLoginDto;
  isFavorite: boolean;
  rdpAdminSession: boolean;
  browserBundleId: string | null;
  mikrotikMode: MikrotikMode;
  sshTerminal: SshTerminal;
  rdpClient: RdpClient;
  color: LabelColor | null;
  notes: string;
  folderPath: string;
  createdAt: Timestamp;
  updatedAt: Timestamp;
}

/** A sidebar entry: the shared fields and its connections. */
export interface EntryDto {
  id: string;
  folderId: string | null;
  /** `Parent / Child`; empty at the root. */
  folderPath: string;
  name: string;
  /** Host shared by every connection. */
  host: string;
  notes: string;
  color: LabelColor | null;
  isFavorite: boolean;
  /** The connection double-click opens; `null` only for an entry without connections. */
  defaultConnectionId: string | null;
  /** Its connections in order. */
  connectionIds: string[];
  /** The MacVPN profile and its test; `null` for no VPN. */
  vpn: EntryVpnDto | null;
}

/** What the VPN gate pings: the entry's host or its test address. */
export type VpnTestMode = 'host' | 'address';

/** An entry's VPN setting (`rd-core::draft::EntryVpnDraft`). */
export interface EntryVpnDto {
  /** The MacVPN connection id, exactly as its control API lists it. */
  connectionId: string;
  testMode: VpnTestMode;
  testAddress: string;
}

export interface CredentialDto {
  id: string;
  name: string;
  username: string;
  domain: string;
  displayUser: string;
  hasPassword: boolean;
  sshKeyPath: string | null;
  hasPassphrase: boolean;
  hasUnlockedKey: boolean;
  notes: string;
  usageCount: number;
  createdAt: Timestamp;
  updatedAt: Timestamp;
}

export interface FolderDto {
  id: string;
  parentId: string | null;
  name: string;
  sortOrder: number;
  color: LabelColor | null;
  path: string;
}

export interface PortForwardDto {
  id: string;
  connectionId: string;
  name: string;
  localPort: number;
  remoteHost: string;
  remotePort: number;
  createdAt: Timestamp;
}

/** The port forward editor's fields (`port_forward_save`); ports as typed. */
export interface PortForwardDraft {
  id: string | null;
  connectionId: string;
  name: string;
  localPort: string;
  remoteHost: string;
  remotePort: string;
}

/** A tunnel's state; `failed` carries a stable `code` and the English `error`. */
export type ForwardState =
  | { state: 'starting' }
  | { state: 'active' }
  | { state: 'failed'; code: string; error: string }
  | { state: 'stopped' };

/** Payload of `port-forward-status` and the rows of `port_forward_statuses`. */
export type ForwardStatusDto = { id: string; connectionId: string } & ForwardState;

/** Payload of `host-key-prompt`. */
export interface HostKeyPromptDto {
  requestId: string;
  forwardId: string;
  host: string;
  port: number;
  keyType: string;
  fingerprint: string;
  /** The old key's entry when the key changed (accepting replaces it). */
  changed?: StoredKey | null;
}

/** Where the old key of a changed host key is stored. */
export interface StoredKey {
  file: string;
  line: number;
}

/** Payload of `host-key-closed`. */
export interface HostKeyClosedDto {
  requestId: string;
}

export interface SavedSplitDto {
  id: string;
  name: string;
  connectionIds: string[];
  sortOrder: number;
  createdAt: Timestamp;
}

export type TreeNodeDto =
  { kind: 'folder'; id: string; children: TreeNodeDto[] } | { kind: 'entry'; id: string };

export interface FlatFolderDto {
  id: string;
  depth: number;
}

export interface LibraryDto {
  folders: FolderDto[];
  entries: EntryDto[];
  connections: ConnectionDto[];
  credentials: CredentialDto[];
  portForwards: PortForwardDto[];
  savedSplits: SavedSplitDto[];
  tree: TreeNodeDto[];
  /** Favourite entry ids. */
  favorites: string[];
  flattenedFolders: FlatFolderDto[];
}

export const EMPTY_LIBRARY: LibraryDto = Object.freeze({
  folders: [],
  entries: [],
  connections: [],
  credentials: [],
  portForwards: [],
  savedSplits: [],
  tree: [],
  favorites: [],
  flattenedFolders: [],
}) as LibraryDto;

export interface SearchResultDto {
  /** Matching entries. */
  entries: string[];
  credentials: string[];
}

export interface LockHolderDto {
  host: string;
  pid: number;
  ageSeconds: number;
}

export type NoDatabaseCode = 'openFailed' | 'noPath' | 'lockCancelled' | 'readOnlyNeedsUpgrade';

export type Phase =
  | { kind: 'noDatabase'; code: NoDatabaseCode; error: string }
  | { kind: 'lockConflict'; holder: LockHolderDto }
  | { kind: 'locked'; needsSetup: boolean }
  | { kind: 'unlocked'; readOnly: boolean }
  /** A newer app version has used the database: update first. */
  | { kind: 'needsUpdate'; databaseVersion: string; appVersion: string };

export interface AppStatusDto {
  phase: Phase;
  databasePath: string | null;
  readOnly: boolean;
  rememberMasterKey: boolean;
  platform: Platform;
  /** First launch after the Swift app: show the welcome banner. */
  handoverNotice: boolean;
}

export type Language = 'en' | 'hu';

export interface SettingsDto {
  databasePath: string | null;
  language: Language;
  rememberMasterKey: boolean;
  externalTerminal: string | null;
  checkForUpdatesAutomatically: boolean;
  lastUpdateCheck: Timestamp | null;
  expandedFolders: string[];
  /** WinBox set in Settings; `null` means auto-detect. */
  winboxPath: string | null;
  selectionShows: SelectionShows;
  /** Entries have launchers in Spotlight / the Start menu / the desktop search. */
  showInSystemSearch: boolean;
  /** macOS embedded RDP: swap the key left of 1 with the key next to left Shift; `null` = automatic. */
  swapIsoKeys: boolean | null;
  /** A connected keyboard is ISO (what automatic follows; always `false` off macOS). */
  isoKeyboard: boolean;
}

/** What a single click on a sidebar entry with an open connection shows: Details, or its only plain tab. */
export type SelectionShows = 'details' | 'tab';

export interface SettingsPatch {
  language?: Language;
  rememberMasterKey?: boolean;
  /** `""` means the OS default. */
  externalTerminal?: string;
  checkForUpdatesAutomatically?: boolean;
  expandedFolders?: string[];
  /** `""` means auto-detect. */
  winboxPath?: string;
  selectionShows?: SelectionShows;
  showInSystemSearch?: boolean;
  swapIsoKeys?: boolean;
}

export type ShortcutCommand =
  | 'newRDP'
  | 'newSSH'
  | 'newWeb'
  | 'newMikrotik'
  | 'newFolder'
  | 'newCredential'
  | 'importRDM'
  | 'importWinbox'
  | 'connect'
  | 'newLocalTerminal'
  | 'typeClipboard'
  | 'splitTerminal'
  | 'broadcastAll'
  | 'clearTerminal'
  | 'recordOutput'
  | 'openFiles'
  | 'closeTab'
  | 'nextTab'
  | 'previousTab'
  | 'reconnect'
  | 'lock'
  | 'toggleSidebar';

/** Modifier bits: command 1, shift 2, option 4, control 8. */
export const MODIFIER = { command: 1, shift: 2, option: 4, control: 8 } as const;

export interface KeyShortcut {
  /** A lowercased character, or return, tab, delete, escape, space, left, right, up, down, f1–f12. */
  key: string;
  modifiers: number;
}

export interface ShortcutDto {
  command: ShortcutCommand;
  group: 'file' | 'connection';
  shortcut: KeyShortcut | null;
  display: string | null;
  defaultDisplay: string;
  customized: boolean;
}

/** The entry editor's shared fields (`rd-core::draft::EntryDraft`). */
export interface EntryDraft {
  name: string;
  host: string;
  folderId?: string | null;
  notes?: string;
  color?: LabelColor | null;
  isFavorite?: boolean;
  vpn?: EntryVpnDto | null;
}

/** One connection's own fields (`rd-core::draft::ConnectionDraft`); `port` is the typed text. */
export interface ConnectionDraft {
  type: ConnectionType;
  credentialId?: string | null;
  port?: string;
  username?: string;
  domain?: string;
  sshKeyPath?: string;
  rdpAdminSession?: boolean;
  rdpClient?: RdpClient;
  browserBundleId?: string | null;
  mikrotikMode?: MikrotikMode;
  sshTerminal?: SshTerminal;
  webScheme?: string;
  webPath?: string;
  label?: string;
  showAsButton?: boolean;
}

/** One connection of `save_entry`: no `id` creates it; secrets `null` keep, `""` clear, a value sets. */
export interface ConnectionSave {
  id?: string | null;
  draft: ConnectionDraft;
  password?: string | null;
  passphrase?: string | null;
  /** Becomes the entry's default (at most one per save). */
  isDefault: boolean;
}

/** What `merge_entries` answers. */
export interface MergeResultDto {
  movedConnections: number;
}

export interface CredentialInput {
  name: string;
  username: string;
  domain: string;
  /** Empty means none. */
  sshKeyPath: string;
  notes: string;
}

export interface FolderInput {
  /** Existing folder to rename; absent creates one. */
  id?: string | null;
  /** Parent of a new folder (ignored for an existing one). */
  parentId?: string | null;
  name: string;
  color?: LabelColor | null;
}

export interface DeleteCountsDto {
  /** Entries deleted through their `e:` / `fav:` tags. */
  entries: number;
  /** Connections deleted, those of deleted entries included. */
  connections: number;
  folders: number;
  credentials: number;
  splits: number;
}

export interface MoveResultDto {
  moved: number;
  cycleRefused: number;
}

export type LockChoice = 'readOnly' | 'openAnyway' | 'cancel';
export type PickMode = 'open' | 'create';

// Import (docs: import_preview / import_commit)

export type ImportKind = 'rdm' | 'winbox';

/** An entry the importer left out. */
export type SkippedDto = { name: string } & ({ reason: 'unsupported'; kind: string } | { reason: 'noHost' });

/** One preview row; secret-free (only whether the export has a password). */
export interface ImportRowDto {
  /** `c<n>` for connections, `k<n>` for credentials; stable within one file. */
  id: string;
  type: ConnectionType | 'credential';
  name: string;
  host: string;
  port: number | null;
  user: string;
  groupPath: string[];
  hasPassword: boolean;
  /** Already in the library: unchecked by default. */
  duplicate: boolean;
}

export interface ImportPreviewDto {
  kind: ImportKind;
  fileName: string;
  rows: ImportRowDto[];
  skipped: SkippedDto[];
  withoutPassword: number;
  emptyGroups: number;
}

export interface ImportResultDto {
  connections: number;
  credentials: number;
  foldersCreated: number;
  skipped: number;
  withoutPassword: number;
}

// Export and import of the app's own format (docs: export_library / transfer_preview / transfer_commit)

export interface ExportResultDto {
  entries: number;
  credentials: number;
  /** Encrypted, with the passwords. */
  encrypted: boolean;
}

/** One row of an export's preview; secret-free (only whether the file has a password for it). */
export interface TransferRowDto {
  /** `e<n>` for entries, `k<n>` for credentials; stable within one file. */
  id: string;
  type: 'entry' | 'credential';
  name: string;
  host: string;
  user: string;
  folderPath: string[];
  connectionTypes: ConnectionType[];
  /** Rows of the credentials the entry's connections link to. */
  credentialIds: string[];
  hasPassword: boolean;
  /** A matching record exists: the user picks overwrite or keep both. */
  conflict: boolean;
}

export interface TransferPreviewDto {
  fileName: string;
  encrypted: boolean;
  exportedAt: string;
  rows: TransferRowDto[];
}

export type TransferAction = 'new' | 'overwrite' | 'skip';

export interface TransferDecisionDto {
  id: string;
  action: TransferAction;
}

/** A CSV row left out; `line` is 1-based with the header on line 1. */
export type CsvSkippedDto = { line: number; name: string } & (
  { reason: 'noName' | 'noHost' } | { reason: 'unknownType' | 'badUrl'; detail: string }
);

export interface CsvPreviewDto {
  fileName: string;
  rows: TransferRowDto[];
  skipped: CsvSkippedDto[];
}

export interface TransferResultDto {
  entriesCreated: number;
  entriesUpdated: number;
  credentialsCreated: number;
  credentialsUpdated: number;
  foldersCreated: number;
}

// Embedded SSH

export type RecordPhase = 'idle' | 'armed' | 'capturing';
export type RecordOutcome = 'copied' | 'empty' | 'alternateScreen' | 'tooLarge';
export type ClearAction = 'none' | 'localOnly' | 'localAndRedraw';

/** A session event's kind and fields. */
export type SshEvent =
  | { kind: 'title'; title: string }
  | { kind: 'altScreen'; active: boolean }
  | { kind: 'bracketedPaste'; enabled: boolean }
  | { kind: 'root'; root: boolean }
  | { kind: 'recording'; phase: RecordPhase }
  | { kind: 'recorded'; outcome: RecordOutcome; text: string }
  | { kind: 'exit'; code: number | null; authFailed: boolean; hostKeyChanged?: ChangedHostKey };

/** The changed host key the system `ssh` refused (an `exit` event's `hostKeyChanged`). */
export interface ChangedHostKey {
  /** The name the key is stored under: `host` or `[host]:port`. */
  host: string;
  /** `ssh`'s short key type: `ED25519`, `RSA`, `ECDSA`, … */
  keyType: string;
  /** The new key's fingerprint. */
  fingerprint: string;
  /** The `known_hosts` file with the old key, and its line. */
  file: string;
  line: number;
}

/** Payload of `ssh-event`. */
export type SshEventDto = { sessionId: string } & SshEvent;

export type AskpassKind = 'password' | 'passphrase' | 'confirm' | 'other';

/** Payload of `askpass-prompt`. */
export interface AskpassPromptDto {
  requestId: string;
  sessionId: string;
  kind: AskpassKind;
  prompt: string;
}

/** Payload of `askpass-closed`. */
export interface AskpassClosedDto {
  requestId: string;
}

/** An external terminal for Settings (`installed_terminals`). */
export interface TerminalChoiceDto {
  id: string;
  name: string;
}

/** A browser for the web connection picker (`installed_browsers`). */
export interface BrowserDto {
  id: string;
  name: string;
}

/** What a launch did (`connect_web`, `connect_winbox`, `connect_rdp_external`), for the toast. */
export type LaunchOutcomeDto =
  { kind: 'launched' } | { kind: 'loginCopied' } | { kind: 'passwordCopied'; reason: string };

/** Payload of `launch-notice`: a launch fell back after the command returned. `message` is a catalog
 * key. */
export interface LaunchNoticeDto {
  connectionName: string;
  message: string;
}

/** Where WinBox is (`winbox_status`). */
export interface WinboxStatusDto {
  detectedPath: string | null;
  /** Whether the configured path exists; `null` when none is set. */
  configuredExists: boolean | null;
  usesWine: boolean;
}

// File browser (docs: app/src-tauri/src/commands/files.rs)

/** What a listing entry is. */
export type EntryKind = 'file' | 'directory' | 'symlink' | 'other';

/** One entry of a remote or local listing. `modified` is Unix seconds. */
export interface FileEntryDto {
  name: string;
  kind: EntryKind;
  size: number;
  modified: number | null;
  permissions: string;
}

/** A folder listing; `parent` is `null` at the root. */
export interface ListingDto {
  path: string;
  parent: string | null;
  entries: FileEntryDto[];
}

/** How the server is used: tar streams, listings only, or SFTP single files (RouterOS). */
export type RemoteMode = 'tar' | 'shellOnly' | 'sftp';

export type BrowserState =
  | { state: 'connecting' }
  | { state: 'ready'; mode: RemoteMode }
  | { state: 'failed'; code: string; error: string };

/** `files_info` and the `files-status` event. */
export type FilesStatusDto = {
  browserId: string;
  connectionId: string;
  connectionName: string;
  startPath: string;
  transferring: boolean;
} & BrowserState;

/** What `ssh_key_setup` answers. */
export interface KeySetupDto {
  /** The host the key was installed on. */
  host: string;
  /** The key file, `~`-relative when it is in the home folder. */
  keyPath: string;
  /** A new key pair was generated and saved on the connection or its credential. */
  generated: boolean;
  /** A login with only the key worked afterwards. */
  verified: boolean;
}

/** What `files_open` answers. */
export interface FilesOpenDto {
  browserId: string;
  connectionName: string;
  created: boolean;
}

export type TransferDirection = 'download' | 'upload';
export type ConflictChoice = 'replace' | 'skip';

/** An unknown (or changed) host key: what the host key dialog shows. */
export interface HostKeyQuestion {
  requestId: string;
  host: string;
  port: number;
  keyType: string;
  fingerprint: string;
  /** Set for a changed key: the stored entry that accepting replaces. */
  changed?: StoredKey | null;
}

/** Payload of `files-host-key-prompt`. */
export interface FilesHostKeyDto extends HostKeyQuestion {
  browserId: string;
}

/** Payload of `files-progress`. */
export interface FilesProgressDto {
  browserId: string;
  transferId: string;
  direction: TransferDirection;
  index: number;
  count: number;
  name: string;
  bytesDone: number;
  bytesTotal: number | null;
}

/** Payload of `files-conflict`. */
export interface FilesConflictDto {
  browserId: string;
  requestId: string;
  name: string;
  isFolder: boolean;
  direction: TransferDirection;
}

export type ItemStatus = 'done' | 'skipped' | 'cancelled' | 'failed';

export interface ItemReportDto {
  name: string;
  status: ItemStatus;
  error: string | null;
}

/** Payload of `files-done`. */
export interface FilesDoneDto {
  browserId: string;
  transferId: string;
  direction: TransferDirection;
  items: ItemReportDto[];
  cancelled: boolean;
}

// Embedded RDP

/** What a session reports (`rdp-event`, without its `sessionId`). */
export type RdpEvent =
  | { kind: 'connected'; width: number; height: number }
  | { kind: 'resized'; width: number; height: number }
  | { kind: 'reconnecting'; attempt: number }
  /** `clean`: the tab may close; an empty `message` means the server gave no reason. */
  | { kind: 'disconnected'; clean: boolean; code: number; message: string }
  /** The remote copied text; it is already on the local clipboard. */
  | { kind: 'remoteClipboard'; chars: number }
  | { kind: 'typed'; finished: boolean }
  /** Files from the remote clipboard are downloading (bytes; `total` grows while sizes are asked). */
  | { kind: 'fileProgress'; done: number; total: number }
  /** The remote clipboard's files are downloaded and already on the local clipboard. */
  | { kind: 'filesReady'; count: number }
  /** A download, the server's file list or the local files offered to it failed (English message). */
  | { kind: 'fileTransferFailed'; message: string }
  | { kind: 'fileTransferCancelled' }
  /** The remote pointer's shape (RGBA PNG, base64; the hotspot is inside it). */
  | { kind: 'pointerShape'; png: string; hotspotX: number; hotspotY: number }
  | { kind: 'pointerHidden' };

/** Payload of `rdp-event`. */
export type RdpEventDto = { sessionId: string } & RdpEvent;

/** One canvas input event (`rdp_input`); `x`/`y` are canvas CSS pixels, Rust maps them to the desktop. */
export type RdpInput =
  | { type: 'key'; code: string; key: string; down: boolean }
  | { type: 'mouseMove'; x: number; y: number }
  | { type: 'mouseButton'; button: number; down: boolean; x: number; y: number }
  | { type: 'wheel'; deltaX: number; deltaY: number; deltaMode: number; x: number; y: number };

/** Reply of `rdp_type_clipboard`. */
export type TypeClipboardDto =
  | { status: 'typing'; chars: number }
  | { status: 'needsConfirmation'; chars: number; seconds: number }
  | { status: 'empty' }
  | { status: 'busy' };

/** Payload of `rdp-cert-prompt`. */
export interface RdpCertPromptDto {
  requestId: string;
  sessionId: string;
  connectionName: string;
  host: string;
  port: number;
  /** SHA-256, `AB:CD:…`. */
  fingerprint: string;
  /** A different certificate was accepted before. */
  changed: boolean;
  /** The certificate is not for this host name. */
  nameMismatch: boolean;
  commonName: string;
  subject: string;
  issuer: string;
}

/** Payload of `rdp-credentials-prompt`. */
export interface RdpCredentialsPromptDto {
  requestId: string;
  sessionId: string;
  connectionName: string;
  username: string;
  domain: string;
  /** Offer "Save to this connection" (no linked credential). */
  canSave: boolean;
  /** The server rejected the previous credentials. */
  rejected: boolean;
}

/** The answer to an NLA prompt (`rdp_credentials_answer`). */
export interface RdpCredentialsAnswer {
  username: string;
  domain: string;
  password: string;
  save: boolean;
}

/** Payload of `rdp-prompt-closed`. */
export interface RdpPromptClosedDto {
  requestId: string;
}

export type CertificateChoice = 'acceptPermanently' | 'acceptOnce' | 'reject';

/** Reply of `rdp_capture_keyboard`. */
export interface RdpCaptureDto {
  /** `hook` (Windows), `grab` (X11: the canvas gets no key events) or `none`. */
  mode: 'hook' | 'grab' | 'none';
  active: boolean;
}

/** Reply of `rdp_engine_info`. */
export interface RdpEngineInfoDto {
  name: string;
  version: string;
}

/** Result of `update_check` (the Tauri updater; see `app/src-tauri/src/updates.rs`). */
export type UpdateCheckDto =
  | { kind: 'notConfigured'; currentVersion: string }
  | { kind: 'skipped' }
  | { kind: 'upToDate'; currentVersion: string }
  | {
      kind: 'available';
      currentVersion: string;
      version: string;
      notes: string | null;
      date: string | null;
    };

/** Payload of `update-progress`. */
export interface UpdateProgressDto {
  downloaded: number;
  total: number | null;
}

// Connect with VPN (macOS)

/** One MacVPN connection RemoteDesktop may use (`vpn_profiles`). */
export interface VpnProfileDto {
  id: string;
  name: string;
  kind: string;
  state: string;
}

/** A connect's VPN gate (event `vpn-progress`). */
export interface VpnProgressDto {
  requestId: string;
  phase: 'testing' | 'connecting' | 'waitingForMfa' | 'waitingForCredentials' | 'done' | 'failed';
  profileName: string;
  entryName: string;
  /** The address that did not answer the ping. */
  unreachable: string | null;
}

/** A VPN RemoteDesktop holds up. */
export interface HeldVpnDto {
  profileId: string;
  profileName: string;
  /** External launches holding it ("Release" drops these). */
  externalEntries: { entryId: string; entryName: string }[];
  /** Every entry holding it (sidebar badge). */
  entryIds: string[];
  /** Open tabs, panes, files windows and forwards holding it. */
  openHolders: number;
}

/** `vpn_status`: pairing and the held VPNs. */
export interface VpnStatusDto {
  supported: boolean;
  paired: boolean;
  held: HeldVpnDto[];
}
