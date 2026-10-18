// The detail views' pure logic (Swift `DetailView.swift`): the connection subtitle, the field grids with
// "—" for empty values, the port forward lines, and the "Used by" / link sheet checkbox sets.

import { t } from '../i18n/index.svelte';
import { terminalName, typeName, usesSSH } from '../sidebar/model';
import type {
  ConnectionDto,
  CredentialDto,
  LibraryDto,
  MikrotikMode,
  Platform,
  PortForwardDto,
  RdpClient,
} from '../types';

/** What an empty value shows. */
export const EMPTY = '—';
/** A stored secret. */
export const MASKED = '••••••••';

/** The text of a grid value: "—" when empty. */
export function displayValue(value: string | null | undefined): string {
  return value ? value : EMPTY;
}

/** Swift `MikrotikMode.displayName`. */
export function mikrotikModeName(mode: MikrotikMode): string {
  return mode === 'winbox' ? 'WinBox' : 'SSH';
}

/** The system RDP client: "Windows App" on macOS, the OS client elsewhere. */
export function systemRdpClientName(platform: Platform): string {
  return platform === 'macos' ? 'Windows App' : t('System RDP client');
}

/** Swift `RDPClient.displayName`. */
export function rdpClientName(client: RdpClient, platform: Platform): string {
  return client === 'embedded' ? t('Embedded (FreeRDP)') : systemRdpClientName(platform);
}

/** The header subtitle per type (Swift `ConnectionDetailView.subtitle`). */
export function connectionSubtitle(
  connection: ConnectionDto,
  platform: Platform,
  browserName: (id: string) => string = (id) => id,
): string {
  switch (connection.type) {
    case 'rdp':
      return (
        'RDP · ' +
        (connection.rdpClient === 'embedded' ? t('embedded (FreeRDP)') : systemRdpClientName(platform)) +
        (connection.rdpAdminSession ? ' · /admin' : '')
      );
    case 'ssh':
      return (
        'SSH · ' + (connection.sshTerminal === 'embedded' ? t('embedded terminal') : terminalName(platform))
      );
    case 'mikrotik':
      return t('MikroTik · default %@', mikrotikModeName(connection.mikrotikMode));
    case 'web':
      return `${typeName('web')} · ${
        connection.browserBundleId ? browserName(connection.browserBundleId) : t('default browser')
      }`;
  }
}

export interface DetailField {
  /** Catalog key of the label. */
  label: string;
  /** The shown text ("—" when empty). */
  value: string;
  /** The text the copy button copies; absent when the field has no copy button. */
  copy?: string;
}

function field(label: string, value: string | null | undefined, copyable = false): DetailField {
  const shown = displayValue(value);
  return copyable && value ? { label, value: shown, copy: value } : { label, value: shown };
}

/**
 * The connection grid without the credential link (the view puts it after Admin session): Host and Port
 * (Web: the normalised URL), Admin session, then the resolved User, Domain, Password, SSH key, Key
 * passphrase and the folder path.
 */
export function connectionFields(connection: ConnectionDto): { before: DetailField[]; after: DetailField[] } {
  const resolved = connection.resolved;
  const before: DetailField[] = [];
  if (connection.type === 'web') {
    before.push(field('URL', connection.webUrl ?? connection.host, true));
  } else {
    before.push(field('Host', connection.host, true));
    before.push(field('Port', String(connection.effectivePort)));
  }
  if (connection.type === 'rdp') {
    before.push(field('Admin session', connection.rdpAdminSession ? t('Yes (/admin)') : t('No')));
  }
  const after: DetailField[] = [field('User', resolved.username, true)];
  if (connection.type === 'rdp') after.push(field('Domain', resolved.domain));
  after.push(field('Password', resolved.hasPassword ? MASKED : ''));
  if (usesSSH(connection.type)) {
    after.push(field('SSH key', resolved.sshKeyPath));
    if (resolved.hasPassphrase) after.push(field('Key passphrase', MASKED));
  }
  after.push(field('Folder', connection.folderPath));
  return { before, after };
}

/** The credential grid (Swift `CredentialDetailView`). */
export function credentialFields(credential: CredentialDto): DetailField[] {
  const fields = [
    field('User', credential.username),
    field('Domain', credential.domain),
    field('Password', credential.hasPassword ? MASKED : ''),
    field('SSH key', credential.sshKeyPath),
  ];
  if (credential.hasPassphrase) fields.push(field('Key passphrase', MASKED));
  if (credential.hasUnlockedKey) fields.push(field('Unlocked key', t('stored')));
  return fields;
}

/** Whether "Copy password" shows: the resolved login (the credential's when linked) has a password. */
export function canCopyPassword(connection: ConnectionDto): boolean {
  return connection.resolved.hasPassword;
}

/** Swift `PortForward.destination`: `host:port`, IPv6 in brackets. */
export function forwardDestination(forward: PortForwardDto): string {
  const host = forward.remoteHost.trim();
  return host.includes(':') ? `[${host}]:${forward.remotePort}` : `${host}:${forward.remotePort}`;
}

/** Swift `PortForward.displayName`: the name, or the destination when it has none. */
export function forwardName(forward: PortForwardDto): string {
  return forward.name.trim() || forwardDestination(forward);
}

/** The connection's forwards by local port. */
export function forwardsOf(library: LibraryDto, connectionId: string): PortForwardDto[] {
  return library.portForwards.filter((f) => f.connectionId === connectionId);
}

/** The connections linked to a credential, in library order (Swift `connections(using:)`). */
export function usersOf(library: LibraryDto, credentialId: string): ConnectionDto[] {
  return library.connections.filter((c) => c.credentialId === credentialId);
}

/** The connections directly in a folder (Swift `FolderDetailView` count). */
export function directConnectionCount(library: LibraryDto, folderId: string): number {
  return library.connections.filter((c) => c.folderId === folderId).length;
}

// Checkbox sets ("Used by", the link sheet)

/** "Select all" is on when every shown row is picked (and some row is shown). */
export function allPicked(picked: ReadonlySet<string>, shown: readonly string[]): boolean {
  return shown.length > 0 && shown.every((id) => picked.has(id));
}

/** "Select all" acts on the shown rows only; picks of hidden rows stay. */
export function setAllPicked(
  picked: ReadonlySet<string>,
  shown: readonly string[],
  on: boolean,
): Set<string> {
  const next = new Set(picked);
  for (const id of shown) {
    if (on) next.add(id);
    else next.delete(id);
  }
  return next;
}

/** One row's checkbox. */
export function setPicked(picked: ReadonlySet<string>, id: string, on: boolean): Set<string> {
  return setAllPicked(picked, [id], on);
}

/** What "Unlink n" acts on: the picks that still use the credential. */
export function pickedAmong(picked: ReadonlySet<string>, ids: readonly string[]): string[] {
  return ids.filter((id) => picked.has(id));
}

/** No row picked. */
export const NO_PICKS: ReadonlySet<string> = Object.freeze(new Set<string>());
