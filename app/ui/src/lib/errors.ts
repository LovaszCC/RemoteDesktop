// Turns a `CommandError` into the text shown to the user, in the current language. The one place that
// knows the error codes.

import { toCommandError, type CommandError } from './api';
import { has, t } from './i18n/index.svelte';
import type { NoDatabaseCode, Phase, ShortcutCommand } from './types';

/** Catalog keys of the shortcut commands' titles (Swift `ShortcutCommand.title`). */
export const COMMAND_TITLES: Record<ShortcutCommand, string> = {
  newRDP: 'New RDP connection',
  newSSH: 'New SSH connection',
  newWeb: 'New web connection',
  newMikrotik: 'New MikroTik connection',
  newFolder: 'New folder',
  newCredential: 'New credential',
  importRDM: 'Import from RDM…',
  importWinbox: 'Import from WinBox…',
  connect: 'Connect',
  newLocalTerminal: 'New Local Terminal',
  typeClipboard: 'Type clipboard into RDP session',
  splitTerminal: 'Split terminal',
  broadcastAll: 'Broadcast input to all panes',
  clearTerminal: 'Clear terminal',
  recordOutput: 'Copy next command output',
  openFiles: 'Files (SCP)…',
  closeTab: 'Close session',
  nextTab: 'Next tab',
  previousTab: 'Previous tab',
  reconnect: 'Reconnect',
  lock: 'Lock',
  toggleSidebar: 'Toggle sidebar',
};

/** The translated title of a shortcut command id; unknown ids come back unchanged. */
export function commandTitle(command: string): string {
  const key = COMMAND_TITLES[command as ShortcutCommand];
  return key ? t(key) : command;
}

const READ_ONLY_NEEDS_UPGRADE =
  "This database needs an upgrade, which can't be done while it is open on another computer. Close it there, or update the app there first, then retry.";

/**
 * Fallback catalog key per code, for messages that aren't catalog keys themselves. Rust's English
 * message wins when it is a key (e.g. `wrongPassword` is "Wrong password" on unlock but "The current
 * master password is wrong." on a password change).
 */
const CODE_KEYS: Record<string, string> = {
  noDatabase: 'No database is open.',
  locked: 'RemoteDesktop is locked.',
  readOnly: 'The database is open read-only because another computer is using it.',
  noLockConflict: 'No lock conflict is pending.',
  wrongPassword: 'Wrong password',
  emptyPassword: 'The password is empty.',
  vaultAlreadySetUp: 'The vault is already set up.',
  vaultNotSetUp: 'The vault is not set up.',
  corruptData: 'A stored secret is damaged.',
  database: 'The database failed.',
  notFound: "The item doesn't exist any more.",
  folderCycle: "A folder can't be moved into itself",
  emptyName: 'The name is required.',
  invalidUrl: 'The URL is not valid.',
  emptyHost: 'The host is required.',
  invalidPort: 'The port must be a number from 1 to 65535.',
  invalidLanguage: 'Unknown language.',
  invalidSetting: 'Unknown setting value.',
  unknownCommand: 'Unknown command.',
  wrongKeyPassphrase: 'Wrong key passphrase.',
  cannotReadKey: "Can't read the SSH key: %@",
  cannotUnlockKey: "Couldn't unlock the SSH key: %@",
  superKey: "The Windows/Super key can't be used in app shortcuts.",
  missingModifier: 'Use at least one of Ctrl or Alt.',
  altGr: 'Ctrl+Alt acts as AltGr on some keyboard layouts.',
  reserved: 'This shortcut is reserved by the system.',
  inUse: 'Already used by: %@',
  noPassword: 'No saved password',
  clipboard: 'The clipboard is not available.',
  revealFailed: "Couldn't show the file.",
  settingsWrite: "Couldn't save the settings.",
  importRead: "Can't read the file.",
  importTooLarge: 'The file is too large to import.',
  importUnreadable: "This isn't a Devolutions RDM export (.rdm) or it is damaged.",
  importEmpty: 'The file has nothing to import.',
  internal: 'The command failed.',
  invalidHost: 'The host is not valid.',
  invalidLocalPort: 'The local port must be between 1 and 65535.',
  missingRemoteHost: 'Enter the remote host.',
  invalidRemotePort: 'The remote port must be between 1 and 65535.',
  duplicateLocalPort: 'Another forward of this connection uses this local port.',
  notSsh: 'Port forwards need an SSH connection.',
  browserNotFound: 'The file browser is closed.',
  notConnected: "The file browser isn't connected.",
  transferRunning: 'Wait for the running transfer to finish.',
  nothingToCopy: 'Nothing is selected.',
  windowFailed: "Couldn't open the window.",
  appNotInstalled: '%@ is not installed.',
  launchFailed: "Couldn't launch: %@",
  wrongType: "This connection can't be opened this way.",
  noConnections: 'An entry needs at least one connection.',
  mergeIntoSelf: "An entry can't be merged into itself.",
  multipleDefaults: 'Only one connection can be the default.',
  readOnlyNeedsUpgrade: READ_ONLY_NEEDS_UPGRADE,
};

/** Codes whose detail is technical context appended in parentheses rather than a placeholder value. */
const DETAIL_SUFFIX = new Set([
  'database',
  'settingsWrite',
  'revealFailed',
  'internal',
  'clipboard',
  'importRead',
  'keyFile',
  'spawnFailed',
  'terminalFailed',
  'openFailed',
  'windowFailed',
  'updateCheckFailed',
  'updateInstallFailed',
]);

/** The user-facing, translated text for anything a command threw. */
export function errorMessage(error: unknown): string {
  const e: CommandError = toCommandError(error);
  const key = has(e.message) ? e.message : (CODE_KEYS[e.code] ?? e.message);
  const detail = e.code === 'inUse' && e.detail ? commandTitle(e.detail) : e.detail;
  const text = key.includes('%@') ? t(key, detail ?? '') : t(key);
  return DETAIL_SUFFIX.has(e.code) && detail ? `${text} (${detail})` : text;
}

const NO_DATABASE_KEYS: Record<NoDatabaseCode, string> = {
  openFailed: "Can't open the database: %@",
  noPath: 'No database is open.',
  lockCancelled: 'The database is in use on another computer.',
  readOnlyNeedsUpgrade: READ_ONLY_NEEDS_UPGRADE,
};

const OPEN_FAILED_PREFIX = "Can't open the database: ";

/** The error screen's text for a `noDatabase` phase. */
export function noDatabaseMessage(phase: Extract<Phase, { kind: 'noDatabase' }>): string {
  if (has(phase.error)) return t(phase.error);
  if (phase.code === 'openFailed') {
    const reason = phase.error.startsWith(OPEN_FAILED_PREFIX)
      ? phase.error.slice(OPEN_FAILED_PREFIX.length)
      : phase.error;
    return t(NO_DATABASE_KEYS.openFailed, reason);
  }
  return t(NO_DATABASE_KEYS[phase.code] ?? phase.error);
}
