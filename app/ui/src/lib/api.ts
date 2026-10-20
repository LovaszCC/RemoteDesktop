// Typed wrappers for every Tauri command and the event helpers.
// Every wrapper rejects with a `CommandError`, whatever the bridge threw.

import { invoke as tauriInvoke } from '@tauri-apps/api/core';
import { listen as tauriListen } from '@tauri-apps/api/event';
import type {
  AppStatusDto,
  BrowserDto,
  CertificateChoice,
  AskpassClosedDto,
  AskpassPromptDto,
  ClearAction,
  ConflictChoice,
  ConnectionSave,
  CredentialInput,
  DeleteCountsDto,
  EntryDraft,
  FilesConflictDto,
  FilesDoneDto,
  FilesHostKeyDto,
  FilesOpenDto,
  FilesProgressDto,
  FilesStatusDto,
  FolderInput,
  ForwardStatusDto,
  HostKeyClosedDto,
  HostKeyPromptDto,
  ImportKind,
  ImportPreviewDto,
  ImportResultDto,
  ExportResultDto,
  TransferDecisionDto,
  TransferPreviewDto,
  TransferResultDto,
  CsvPreviewDto,
  KeySetupDto,
  KeyShortcut,
  LaunchNoticeDto,
  LaunchOutcomeDto,
  LibraryDto,
  ListingDto,
  LockChoice,
  MergeResultDto,
  MoveResultDto,
  PickMode,
  PortForwardDraft,
  RdpCaptureDto,
  RdpCertPromptDto,
  RdpCredentialsAnswer,
  RdpCredentialsPromptDto,
  RdpEngineInfoDto,
  RdpEventDto,
  RdpInput,
  RdpPromptClosedDto,
  RecordPhase,
  SearchResultDto,
  SettingsDto,
  SettingsPatch,
  ShortcutCommand,
  ShortcutDto,
  SshEventDto,
  TerminalChoiceDto,
  TypeClipboardDto,
  UpdateCheckDto,
  UpdateProgressDto,
  WinboxStatusDto,
  TransferDirection,
} from './types';

/** What every command rejects with. `message` is English and doubles as the translation key. */
export interface CommandError {
  code: string;
  message: string;
  detail: string | null;
}

export function isCommandError(value: unknown): value is CommandError {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as CommandError).code === 'string' &&
    typeof (value as CommandError).message === 'string'
  );
}

/** Normalizes anything thrown by the bridge into a `CommandError` (`internal` for the unexpected). */
export function toCommandError(error: unknown): CommandError {
  if (isCommandError(error))
    return { code: error.code, message: error.message, detail: error.detail ?? null };
  const message = error instanceof Error ? error.message : String(error);
  return { code: 'internal', message: 'The command failed.', detail: message };
}

export type Invoke = <T>(command: string, args?: Record<string, unknown>) => Promise<T>;
export type Unlisten = () => void;
export type Listen = <T>(event: string, handler: (payload: T) => void) => Promise<Unlisten>;

/** The real bridge. */
export const tauriBridge: { invoke: Invoke; listen: Listen } = {
  invoke: (command, args) => tauriInvoke(command, args),
  listen: (event, handler) => tauriListen(event, (e) => handler(e.payload as never)),
};

export const EVENTS = {
  appStatusChanged: 'app-status-changed',
  libraryChanged: 'library-changed',
  settingsChanged: 'settings-changed',
  shortcutsChanged: 'shortcuts-changed',
  menuCommand: 'menu-command',
  sshEvent: 'ssh-event',
  askpassPrompt: 'askpass-prompt',
  askpassClosed: 'askpass-closed',
  portForwardStatus: 'port-forward-status',
  hostKeyPrompt: 'host-key-prompt',
  hostKeyClosed: 'host-key-closed',
  filesStatus: 'files-status',
  filesHostKeyPrompt: 'files-host-key-prompt',
  filesProgress: 'files-progress',
  filesConflict: 'files-conflict',
  filesDone: 'files-done',
  launchNotice: 'launch-notice',
  rdpEvent: 'rdp-event',
  rdpCertPrompt: 'rdp-cert-prompt',
  rdpCredentialsPrompt: 'rdp-credentials-prompt',
  rdpPromptClosed: 'rdp-prompt-closed',
  updateProgress: 'update-progress',
} as const;

/** Payload of `menu-command`: the clicked native menu item's command id. */
export interface MenuCommandPayload {
  command: string;
}

export function onAppStatusChanged(listen: Listen, handler: (status: AppStatusDto) => void) {
  return listen<AppStatusDto>(EVENTS.appStatusChanged, handler);
}
export function onLibraryChanged(listen: Listen, handler: (library: LibraryDto) => void) {
  return listen<LibraryDto>(EVENTS.libraryChanged, handler);
}
export function onSettingsChanged(listen: Listen, handler: (settings: SettingsDto) => void) {
  return listen<SettingsDto>(EVENTS.settingsChanged, handler);
}
export function onMenuCommand(listen: Listen, handler: (payload: MenuCommandPayload) => void) {
  return listen<MenuCommandPayload>(EVENTS.menuCommand, handler);
}
export function onShortcutsChanged(listen: Listen, handler: (rows: ShortcutDto[]) => void) {
  return listen<ShortcutDto[]>(EVENTS.shortcutsChanged, handler);
}

export function onSshEvent(listen: Listen, handler: (event: SshEventDto) => void) {
  return listen<SshEventDto>(EVENTS.sshEvent, handler);
}
export function onAskpassPrompt(listen: Listen, handler: (prompt: AskpassPromptDto) => void) {
  return listen<AskpassPromptDto>(EVENTS.askpassPrompt, handler);
}
export function onAskpassClosed(listen: Listen, handler: (closed: AskpassClosedDto) => void) {
  return listen<AskpassClosedDto>(EVENTS.askpassClosed, handler);
}

export function onPortForwardStatus(listen: Listen, handler: (status: ForwardStatusDto) => void) {
  return listen<ForwardStatusDto>(EVENTS.portForwardStatus, handler);
}
export function onHostKeyPrompt(listen: Listen, handler: (prompt: HostKeyPromptDto) => void) {
  return listen<HostKeyPromptDto>(EVENTS.hostKeyPrompt, handler);
}
export function onHostKeyClosed(listen: Listen, handler: (closed: HostKeyClosedDto) => void) {
  return listen<HostKeyClosedDto>(EVENTS.hostKeyClosed, handler);
}

export function onFilesStatus(listen: Listen, handler: (status: FilesStatusDto) => void) {
  return listen<FilesStatusDto>(EVENTS.filesStatus, handler);
}
export function onFilesHostKeyPrompt(listen: Listen, handler: (prompt: FilesHostKeyDto) => void) {
  return listen<FilesHostKeyDto>(EVENTS.filesHostKeyPrompt, handler);
}
export function onFilesProgress(listen: Listen, handler: (progress: FilesProgressDto) => void) {
  return listen<FilesProgressDto>(EVENTS.filesProgress, handler);
}
export function onFilesConflict(listen: Listen, handler: (conflict: FilesConflictDto) => void) {
  return listen<FilesConflictDto>(EVENTS.filesConflict, handler);
}
export function onFilesDone(listen: Listen, handler: (done: FilesDoneDto) => void) {
  return listen<FilesDoneDto>(EVENTS.filesDone, handler);
}

export function onLaunchNotice(listen: Listen, handler: (notice: LaunchNoticeDto) => void) {
  return listen<LaunchNoticeDto>(EVENTS.launchNotice, handler);
}

export function onRdpEvent(listen: Listen, handler: (event: RdpEventDto) => void) {
  return listen<RdpEventDto>(EVENTS.rdpEvent, handler);
}
export function onRdpCertPrompt(listen: Listen, handler: (prompt: RdpCertPromptDto) => void) {
  return listen<RdpCertPromptDto>(EVENTS.rdpCertPrompt, handler);
}
export function onRdpCredentialsPrompt(listen: Listen, handler: (prompt: RdpCredentialsPromptDto) => void) {
  return listen<RdpCredentialsPromptDto>(EVENTS.rdpCredentialsPrompt, handler);
}
export function onRdpPromptClosed(listen: Listen, handler: (closed: RdpPromptClosedDto) => void) {
  return listen<RdpPromptClosedDto>(EVENTS.rdpPromptClosed, handler);
}

export function onUpdateProgress(listen: Listen, handler: (progress: UpdateProgressDto) => void) {
  return listen<UpdateProgressDto>(EVENTS.updateProgress, handler);
}

/** Secrets for the editors: `null`/absent keeps the stored value, `""` clears it, anything else replaces. */
export type SecretUpdate = string | null;

/** Every command, typed. `invoke` is injectable for tests. */
export function createApi(invoke: Invoke = tauriBridge.invoke) {
  async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    try {
      return await invoke<T>(command, args);
    } catch (error) {
      throw toCommandError(error);
    }
  }

  return {
    // Session
    appStatus: () => call<AppStatusDto>('app_status'),
    dismissHandover: () => call<AppStatusDto>('dismiss_handover'),
    createVault: (password: string, remember: boolean) =>
      call<AppStatusDto>('create_vault', { password, remember }),
    unlock: (password: string, remember: boolean) => call<AppStatusDto>('unlock', { password, remember }),
    lock: () => call<AppStatusDto>('lock'),
    changePassword: (current: string, next: string) => call<void>('change_password', { current, new: next }),
    runningCount: () => call<number>('running_count'),
    appQuit: () => call<void>('app_quit'),

    // Database
    openDatabase: (path: string) => call<AppStatusDto>('open_database', { path }),
    createDatabase: (path: string) => call<AppStatusDto>('create_database', { path }),
    reopenDatabase: () => call<AppStatusDto>('reopen_database'),
    resolveDbLock: (choice: LockChoice) => call<AppStatusDto>('resolve_db_lock', { choice }),
    revealDatabase: () => call<void>('reveal_database'),
    pickDatabaseFile: (mode: PickMode) => call<string | null>('pick_database_file', { mode }),
    /** The SSH key file panel (starts in ~/.ssh); a `~`-relative path, or `null` when cancelled. */
    pickSshKey: () => call<string | null>('pick_ssh_key'),

    // Import
    /** The open panel for an export (`.rdm`/`.xml` or `.cdb`); the path, or `null` when cancelled. */
    pickImportFile: (kind: ImportKind) => call<string | null>('pick_import_file', { kind }),
    importPreview: (kind: ImportKind, path: string) =>
      call<ImportPreviewDto>('import_preview', { kind, path }),
    /** Imports the rows whose ids are in `selected`; emits `library-changed`. */
    importCommit: (kind: ImportKind, path: string, selected: string[]) =>
      call<ImportResultDto>('import_commit', { kind, path, selected }),

    // Export / import of the app's own format
    /** The save panel for an export; the path, or `null` when cancelled. */
    pickExportPath: () => call<string | null>('pick_export_path'),
    /** The open panel for an export; the path, or `null` when cancelled. */
    pickTransferFile: () => call<string | null>('pick_transfer_file'),
    /** Writes the picked records; with a password the file is encrypted and carries the passwords. */
    exportLibrary: (path: string, entryIds: string[], credentialIds: string[], password: string | null) =>
      call<ExportResultDto>('export_library', { path, entryIds, credentialIds, password }),
    transferPreview: (path: string, password: string | null) =>
      call<TransferPreviewDto>('transfer_preview', { path, password }),
    /** Imports with a decision per row (rows without one are skipped); emits `library-changed`. */
    transferCommit: (path: string, password: string | null, decisions: TransferDecisionDto[]) =>
      call<TransferResultDto>('transfer_commit', { path, password, decisions }),

    // CSV import
    /** Asks where to save the sample CSV and writes it; `false` when cancelled. */
    saveCsvSample: () => call<boolean>('save_csv_sample'),
    /** The open panel for a CSV file; the path, or `null` when cancelled. */
    pickCsvFile: () => call<string | null>('pick_csv_file'),
    csvPreview: (path: string) => call<CsvPreviewDto>('csv_preview', { path }),
    /** Imports with a decision per row (rows without one are skipped); emits `library-changed`. */
    csvCommit: (path: string, decisions: TransferDecisionDto[]) =>
      call<TransferResultDto>('csv_commit', { path, decisions }),

    // Library queries
    library: () => call<LibraryDto>('library'),
    search: (query: string) => call<SearchResultDto>('search', { query }),
    linkCandidates: (credentialId: string, query: string) =>
      call<string[]>('link_candidates', { credentialId, query }),
    matching: (connectionIds: string[], query: string) =>
      call<string[]>('matching', { connectionIds, query }),

    // Library saves
    /** Saves an entry with exactly `connections` in their order; returns the entry id. */
    saveEntry: (id: string | null, entry: EntryDraft, connections: ConnectionSave[]) =>
      call<string>('save_entry', { id, entry, connections }),
    saveCredential: (
      id: string | null,
      credential: CredentialInput,
      password: SecretUpdate = null,
      passphrase: SecretUpdate = null,
    ) => call<string>('save_credential', { id, credential, password, passphrase }),
    saveFolder: (folder: FolderInput) => call<string>('save_folder', { folder }),

    // Library changes
    deleteItems: (tags: string[]) => call<DeleteCountsDto>('delete_items', { tags }),
    moveItems: (tags: string[], targetFolderId: string | null) =>
      call<MoveResultDto>('move_items', { tags, targetFolderId }),
    /** Stars or unstars the entry behind `tag` (`e:`, `fav:` or `c:`). */
    setFavorite: (tag: string, isFavorite: boolean) => call<void>('set_favorite', { tag, isFavorite }),
    /** Makes a connection its entry's default. */
    setDefaultConnection: (entryId: string, connectionId: string) =>
      call<void>('set_default_connection', { entryId, connectionId }),
    setCredential: (credentialId: string | null, target: { tags?: string[]; connectionIds?: string[] }) =>
      call<number>('set_credential', {
        credentialId,
        tags: target.tags ?? null,
        connectionIds: target.connectionIds ?? null,
      }),
    /** Duplicates an entry with all its connections; returns the new entry id. */
    duplicateEntry: (id: string, copySuffix: string) => call<string>('duplicate_entry', { id, copySuffix }),
    /** Moves the `sources` entries' connections into `target` and deletes the emptied entries. */
    mergeEntries: (target: string, sources: string[]) =>
      call<MergeResultDto>('merge_entries', { target, sources }),

    // Saved splits
    saveSavedSplit: (id: string | null, name: string, connectionIds: string[]) =>
      call<string>('save_saved_split', { id, name, connectionIds }),
    renameSavedSplit: (id: string, name: string) => call<void>('rename_saved_split', { id, name }),
    deleteSavedSplit: (id: string) => call<void>('delete_saved_split', { id }),
    reorderSavedSplits: (ids: string[]) => call<void>('reorder_saved_splits', { ids }),

    // Clipboard
    copyPassword: (connectionId: string) => call<void>('copy_password', { connectionId }),
    copyText: (text: string) => call<void>('copy_text', { text }),

    // Settings and shortcuts
    settingsGet: () => call<SettingsDto>('settings_get'),
    settingsSet: (patch: SettingsPatch) => call<SettingsDto>('settings_set', { patch }),
    shortcutsGet: () => call<ShortcutDto[]>('shortcuts_get'),
    shortcutSet: (command: ShortcutCommand, shortcut: KeyShortcut | null) =>
      call<ShortcutDto[]>('shortcut_set', { command, shortcut }),
    shortcutReset: (command: ShortcutCommand) => call<ShortcutDto[]>('shortcut_reset', { command }),
    shortcutsResetAll: () => call<ShortcutDto[]>('shortcuts_reset_all'),

    // Embedded SSH
    /** Starts a session; its raw output arrives on `onData` (a Tauri `Channel`). */
    openSshSession: (connectionId: string, cols: number, rows: number, onData: unknown) =>
      call<{ sessionId: string }>('open_ssh_session', { connectionId, cols, rows, onData }),
    /** Starts a local terminal (this machine's shell); the `ssh*` calls below apply to it too. */
    openLocalTerminal: (cols: number, rows: number, onData: unknown) =>
      call<{ sessionId: string }>('open_local_terminal', { cols, rows, onData }),
    /** Typed text (`onData`) or raw bytes (`onBinary`). */
    sshInput: (sessionId: string, data: string | number[]) => call<void>('ssh_input', { sessionId, data }),
    /** Input typed into a broadcasting split pane: to the source, then to the targets (paste markers
     * dropped for targets not in bracketed-paste mode). */
    sshBroadcastInput: (sourceSessionId: string, targetSessionIds: string[], data: string | number[]) =>
      call<void>('ssh_broadcast_input', { sourceSessionId, targetSessionIds, data }),
    sshResize: (sessionId: string, cols: number, rows: number) =>
      call<void>('ssh_resize', { sessionId, cols, rows }),
    sshClose: (sessionId: string) => call<void>('ssh_close', { sessionId }),
    /** "Replace key" after an exit with `hostKeyChanged`; reconnect afterwards. */
    sshReplaceHostKey: (sessionId: string) => call<void>('ssh_replace_host_key', { sessionId }),
    sshReconnect: (sessionId: string, cols: number, rows: number) =>
      call<void>('ssh_reconnect', { sessionId, cols, rows }),
    /** Arms, cancels or finishes "copy next output"; the new phase. */
    sshRecordNextOutput: (sessionId: string) => call<RecordPhase>('ssh_record_next_output', { sessionId }),
    sshRecordCancel: (sessionId: string) => call<void>('ssh_record_cancel', { sessionId }),
    sshClearDecision: (sessionId: string) => call<ClearAction>('ssh_clear_decision', { sessionId }),
    /** Answers an `askpass-prompt`; `null` refuses. */
    askpassAnswer: (requestId: string, answer: string | null) =>
      call<void>('askpass_answer', { requestId, answer }),
    /** Opens the connection in the external terminal; the id of the terminal used. */
    openExternalTerminal: (connectionId: string) => call<string>('open_external_terminal', { connectionId }),
    installedTerminals: () => call<TerminalChoiceDto[]>('installed_terminals'),
    /** The clipboard text for a terminal paste, or `null`. */
    terminalPasteText: () => call<string | null>('terminal_paste_text'),
    /** Opens an http(s) link clicked in a terminal. */
    terminalOpenUrl: (url: string) => call<void>('terminal_open_url', { url }),

    // External launchers
    /** Opens a web connection in its browser, then copies password and username. */
    connectWeb: (connectionId: string) => call<LaunchOutcomeDto>('connect_web', { connectionId }),
    installedBrowsers: () => call<BrowserDto[]>('installed_browsers'),
    /** Starts WinBox for a MikroTik connection (the password goes on WinBox's command line). */
    connectWinbox: (connectionId: string) => call<LaunchOutcomeDto>('connect_winbox', { connectionId }),
    winboxStatus: () => call<WinboxStatusDto>('winbox_status'),
    /** An open panel for the WinBox app or executable; `null` when cancelled. */
    pickWinbox: () => call<string | null>('pick_winbox'),
    /** Opens an RDP connection in the system RDP client (Windows App, mstsc, Remmina or xfreerdp). */
    connectRdpExternal: (connectionId: string) =>
      call<LaunchOutcomeDto>('connect_rdp_external', { connectionId }),
    /** The macOS Accessibility permission for the RDP autofill; `null` on the other OSes. */
    accessibilityStatus: () => call<boolean | null>('accessibility_status'),
    /** Asks for the permission and opens System Settings › Accessibility (macOS). */
    openAccessibilitySettings: () => call<void>('open_accessibility_settings'),

    // Port forwards (background SSH)
    /** Saves the editor's draft (a running forward restarts); the id. Emits `library-changed`. */
    portForwardSave: (forward: PortForwardDraft) => call<string>('port_forward_save', { forward }),
    portForwardDelete: (id: string) => call<void>('port_forward_delete', { id }),
    /** Starts the tunnel; progress arrives as `port-forward-status`. */
    portForwardStart: (id: string) => call<void>('port_forward_start', { id }),
    portForwardStop: (id: string) => call<void>('port_forward_stop', { id }),
    portForwardStatuses: () => call<ForwardStatusDto[]>('port_forward_statuses'),
    /** Answers a `host-key-prompt`. */
    hostKeyAnswer: (requestId: string, accept: boolean) =>
      call<void>('host_key_answer', { requestId, accept }),

    // SSH keys
    /** Generates an ed25519 key pair in ~/.ssh named after `name`; the `~`-relative path (nothing saved). */
    sshKeyGenerate: (name: string) => call<string>('ssh_key_generate', { name }),
    /** Installs the connection's key on the server (generating and saving one when it has none). An
     * unknown host key arrives as `host-key-prompt`; emits `library-changed`. */
    sshKeySetup: (connectionId: string) => call<KeySetupDto>('ssh_key_setup', { connectionId }),

    // File browser (one window per connection)
    /** Opens (or focuses) the connection's Files window; `sessionId` starts in that shell's directory. */
    filesOpen: (connectionId: string, options: { startPath?: string; sessionId?: string } = {}) =>
      call<FilesOpenDto>('files_open', {
        connectionId,
        startPath: options.startPath ?? null,
        sessionId: options.sessionId ?? null,
      }),
    filesInfo: (browserId: string) => call<FilesStatusDto>('files_info', { browserId }),
    filesReconnect: (browserId: string) => call<void>('files_reconnect', { browserId }),
    filesListRemote: (browserId: string, path: string, showHidden: boolean) =>
      call<ListingDto>('files_list_remote', { browserId, path, showHidden }),
    /** `null`: the default folder (Downloads). */
    filesListLocal: (path: string | null, showHidden: boolean) =>
      call<ListingDto>('files_list_local', { path, showHidden }),
    /** Starts a transfer (one at a time); the transfer id. Progress arrives as events. */
    filesTransfer: (
      browserId: string,
      direction: TransferDirection,
      sources: string[],
      destination: string,
      gzip: boolean,
    ) => call<string>('files_transfer', { browserId, direction, sources, destination, gzip }),
    filesCancel: (browserId: string) => call<void>('files_cancel', { browserId }),
    filesConflictAnswer: (requestId: string, choice: ConflictChoice, applyToAll: boolean) =>
      call<void>('files_conflict_answer', { requestId, choice, applyToAll }),
    /** Whether "Files (SCP)…" applies in the main window (enables the menu item). */
    menuFilesTarget: (enabled: boolean) => call<void>('menu_files_target', { enabled }),

    // Embedded RDP
    /** Opens a session for a view of `width`×`height` CSS pixels; binary frames arrive on `onFrame`. */
    openRdpSession: (connectionId: string, width: number, height: number, onFrame: unknown) =>
      call<{ sessionId: string }>('open_rdp_session', { connectionId, width, height, onFrame }),
    /** A frame was painted (at most 2 are in flight). */
    rdpAckFrame: (sessionId: string) => call<void>('rdp_ack_frame', { sessionId }),
    rdpInput: (sessionId: string, events: RdpInput[]) => call<void>('rdp_input', { sessionId, events }),
    rdpReleaseAllKeys: (sessionId: string) => call<void>('rdp_release_all_keys', { sessionId }),
    /** The view's new size; Rust debounces the desktop resize (300 ms). */
    rdpResize: (sessionId: string, width: number, height: number) =>
      call<void>('rdp_resize', { sessionId, width, height }),
    rdpClose: (sessionId: string) => call<void>('rdp_close', { sessionId }),
    rdpReconnect: (sessionId: string, width: number, height: number) =>
      call<void>('rdp_reconnect', { sessionId, width, height }),
    /** Types the local clipboard text into the session; above 2000 characters only with `confirmed`. */
    /** Cancels the download of remote clipboard files (`fileTransferCancelled` follows). */
    rdpCancelFileTransfer: (sessionId: string) => call<void>('rdp_cancel_file_transfer', { sessionId }),
    rdpTypeClipboard: (sessionId: string, confirmed: boolean) =>
      call<TypeClipboardDto>('rdp_type_clipboard', { sessionId, confirmed }),
    rdpCertAnswer: (requestId: string, choice: CertificateChoice) =>
      call<void>('rdp_cert_answer', { requestId, choice }),
    /** Answers an NLA prompt; `null` cancels (the session ends cleanly). */
    rdpCredentialsAnswer: (requestId: string, answer: RdpCredentialsAnswer | null) =>
      call<void>('rdp_credentials_answer', { requestId, answer }),
    /** Starts (canvas focus) or stops (blur) the system keyboard capture. */
    rdpCaptureKeyboard: (sessionId: string, active: boolean) =>
      call<RdpCaptureDto>('rdp_capture_keyboard', { sessionId, active }),
    rdpEngineInfo: () => call<RdpEngineInfoDto>('rdp_engine_info'),

    // Updates (Tauri updater, driven from Rust).
    /** `automatic`: the launch check, which only runs when enabled and due (`skipped` otherwise). */
    updateCheck: (automatic: boolean) => call<UpdateCheckDto>('update_check', { automatic }),
    /** Downloads, verifies and installs the found update (`update-progress` events). */
    updateInstall: () => call<void>('update_install'),
    /** Restarts into the new version (the quit confirmation asks while sessions run). */
    updateRelaunch: () => call<void>('update_relaunch'),
  };
}

export type Api = ReturnType<typeof createApi>;

/** The app-wide instance on the real bridge. */
export const api = createApi();
