// The editors' pure logic (Swift `ConnectionDraft`, `CredentialDraft`, `FolderEditorView`): the editable
// form state, the secret field rule, the entry and connection drafts sent to Rust with the fields each
// type doesn't use left out, pasted URL splitting, the picker options and where a validation error from
// Rust shows.

import { t } from '../i18n/index.svelte';
import { typeName, usesSSH } from '../sidebar/model';
import { browserPickerOptions } from '../stores/browsers.svelte';
import type {
  BrowserDto,
  ConnectionDto,
  ConnectionSave,
  ConnectionType,
  CredentialDto,
  CredentialInput,
  EntryDraft,
  EntryDto,
  EntryVpnDto,
  LabelColor,
  LibraryDto,
  MikrotikMode,
  RdpClient,
  SshTerminal,
} from '../types';

// Secrets

/**
 * A password or passphrase field. The stored secret never reaches the UI, so the field starts empty;
 * `stored` says whether one exists and `cleared` whether the user asked to remove it.
 */
export interface SecretState {
  text: string;
  stored: boolean;
  cleared: boolean;
}

export function secretState(stored: boolean): SecretState {
  return { text: '', stored, cleared: false };
}

/** The value for the save command: `null` keeps the stored secret, `""` clears it, text replaces it. */
export function secretUpdate(secret: SecretState): string | null {
  if (secret.text !== '') return secret.text;
  return secret.cleared ? '' : null;
}

/** The "Clear" action: drops the stored secret on save. */
export function clearSecret(secret: SecretState): SecretState {
  return { ...secret, text: '', cleared: true };
}

/** Undoes "Clear": the stored secret stays. */
export function keepSecret(secret: SecretState): SecretState {
  return { ...secret, text: '', cleared: false };
}

/** The placeholder: "unchanged" while a stored secret is kept, nothing otherwise. */
export function secretPlaceholder(secret: SecretState): string {
  return secret.stored && !secret.cleared ? t('unchanged') : '';
}

/** Whether a stored secret will be kept on save (the field shows "Clear"). */
export function keepsStored(secret: SecretState): boolean {
  return secret.stored && !secret.cleared && secret.text === '';
}

// Entry editor

/** One connection tab of the entry editor: its own fields; the shared ones are the entry's. */
export interface ConnectionForm {
  /** Stable key of the tab: the connection id, or a fresh `new-<n>` for an added connection. */
  key: string;
  /** `null` for a connection added in this editor. */
  id: string | null;
  type: ConnectionType;
  credentialId: string | null;
  port: string;
  username: string;
  domain: string;
  sshKeyPath: string;
  rdpAdminSession: boolean;
  rdpClient: RdpClient;
  browserBundleId: string | null;
  /** The browser saved before editing, listed even when it isn't installed. */
  savedBrowserBundleId: string | null;
  mikrotikMode: MikrotikMode;
  sshTerminal: SshTerminal;
  webScheme: string;
  webPath: string;
  label: string;
  showAsButton: boolean;
  password: SecretState;
  passphrase: SecretState;
}

/** The entry editor: the General tab's shared fields and the connections in order. */
export interface EntryForm {
  name: string;
  host: string;
  folderId: string | null;
  notes: string;
  color: LabelColor | null;
  isFavorite: boolean;
  connections: ConnectionForm[];
  /** The key of the default connection. */
  defaultKey: string | null;
  /** The VPN setting; an empty `connectionId` means no VPN. */
  vpn: EntryVpnDto;
}

const NO_VPN: EntryVpnDto = { connectionId: '', testMode: 'host', testAddress: '' };

let newKeys = 0;

/** A blank connection of `type` (Swift `ConnectionDraft(type:)`), shown as a button on Details. */
export function newConnectionForm(type: ConnectionType): ConnectionForm {
  newKeys += 1;
  return {
    key: `new-${newKeys}`,
    id: null,
    type,
    credentialId: null,
    port: '',
    username: '',
    domain: '',
    sshKeyPath: '',
    rdpAdminSession: false,
    rdpClient: 'embedded',
    browserBundleId: null,
    savedBrowserBundleId: null,
    mikrotikMode: 'winbox',
    sshTerminal: 'embedded',
    webScheme: 'https',
    webPath: '',
    label: '',
    // Off for new connections: an entry's only connection gets its Connect button anyway.
    showAsButton: false,
    password: secretState(false),
    passphrase: secretState(false),
  };
}

/** The form for an existing connection; its own secrets are only flagged. */
export function connectionFormFrom(connection: ConnectionDto): ConnectionForm {
  return {
    key: connection.id,
    id: connection.id,
    type: connection.type,
    credentialId: connection.credentialId,
    port: connection.port === null ? '' : String(connection.port),
    username: connection.username,
    domain: connection.domain,
    sshKeyPath: connection.sshKeyPath ?? '',
    rdpAdminSession: connection.rdpAdminSession,
    rdpClient: connection.rdpClient,
    browserBundleId: connection.browserBundleId,
    savedBrowserBundleId: connection.browserBundleId,
    mikrotikMode: connection.mikrotikMode,
    sshTerminal: connection.sshTerminal,
    webScheme: connection.webScheme,
    webPath: connection.webPath,
    label: connection.label,
    showAsButton: connection.showAsButton,
    password: secretState(connection.hasPassword),
    passphrase: secretState(connection.hasPassphrase),
  };
}

/** "+ New SSH…": an entry with one connection of `type`, its default. */
export function newEntryForm(type: ConnectionType, folderId: string | null): EntryForm {
  const connection = newConnectionForm(type);
  return {
    name: '',
    host: '',
    folderId,
    notes: '',
    color: null,
    isFavorite: false,
    connections: [connection],
    defaultKey: connection.key,
    vpn: { ...NO_VPN },
  };
}

/** The form for an existing entry and its connections (in order). */
export function entryFormFrom(entry: EntryDto, connections: readonly ConnectionDto[]): EntryForm {
  return {
    name: entry.name,
    host: entry.host,
    folderId: entry.folderId,
    notes: entry.notes,
    color: entry.color,
    isFavorite: entry.isFavorite,
    connections: connections.map(connectionFormFrom),
    defaultKey: entry.defaultConnectionId ?? connections[0]?.id ?? null,
    vpn: entry.vpn ? { ...entry.vpn } : { ...NO_VPN },
  };
}

/** A connection tab's title, like Rust's `displayLabel`: the label, else `<TYPE> · <user>`, else `<TYPE>`;
 * the user is the linked credential's when it has one. */
export function connectionTabLabel(form: ConnectionForm, credential?: CredentialDto): string {
  const label = form.label.trim();
  if (label) return label;
  const user = credential && credential.username !== '' ? credential.username : form.username;
  return user ? `${typeName(form.type)} · ${user}` : typeName(form.type);
}

/** Swift `ConnectionType.defaultPort`, the port field's placeholder. */
export const DEFAULT_PORTS: Record<ConnectionType, number> = { rdp: 3389, ssh: 22, web: 443, mikrotik: 22 };

/**
 * One connection for `save_entry` (Rust validates, trims and drops the same fields again): the domain
 * and admin flag only for RDP, the key path and passphrase only for SSH types, the browser, scheme and
 * path only for web. Untouched secrets are `null` (kept).
 */
export function buildConnectionSave(form: ConnectionForm, isDefault: boolean): ConnectionSave {
  const ssh = usesSSH(form.type);
  const rdp = form.type === 'rdp';
  const web = form.type === 'web';
  return {
    id: form.id,
    draft: {
      type: form.type,
      credentialId: form.credentialId,
      port: form.port.trim(),
      username: form.username,
      domain: rdp ? form.domain : '',
      sshKeyPath: ssh ? form.sshKeyPath : '',
      rdpAdminSession: rdp && form.rdpAdminSession,
      rdpClient: form.rdpClient,
      browserBundleId: web ? form.browserBundleId : null,
      mikrotikMode: form.mikrotikMode,
      sshTerminal: form.sshTerminal,
      webScheme: web ? form.webScheme.trim() : 'https',
      webPath: web ? form.webPath.trim() : '',
      label: form.label,
      showAsButton: form.showAsButton,
    },
    password: secretUpdate(form.password),
    passphrase: ssh ? secretUpdate(form.passphrase) : null,
    isDefault,
  };
}

/** What `save_entry` receives: the shared fields and every connection in the tabs' order, the default
 * one flagged. */
export function buildEntrySave(form: EntryForm): { entry: EntryDraft; connections: ConnectionSave[] } {
  return {
    entry: {
      name: form.name,
      host: form.host,
      folderId: form.folderId,
      notes: form.notes,
      color: form.color,
      isFavorite: form.isFavorite,
      vpn: form.vpn.connectionId === '' ? null : { ...form.vpn },
    },
    connections: form.connections.map((c) => buildConnectionSave(c, c.key === form.defaultKey)),
  };
}

/** Whether a typed port is empty or a number from 1 to 65535 (Rust's rule). */
function validPort(text: string): boolean {
  const port = text.trim();
  return port === '' || (/^\d+$/.test(port) && Number(port) >= 1 && Number(port) <= 65535);
}

/** The key of the first connection whose port Rust rejects (`invalidPort` names no connection). */
export function invalidPortKey(form: EntryForm): string | null {
  return form.connections.find((c) => !validPort(c.port))?.key ?? null;
}

/** The parts of a pasted web URL. */
export interface WebParts {
  scheme: string;
  host: string;
  port: number | null;
  /** The path with its `?query#fragment`; empty for none. */
  path: string;
}

const SCHEME = /^[A-Za-z][A-Za-z0-9+.-]*$/;

/**
 * Splits a pasted URL like Rust's `split_web_url`: any `scheme://`, none means `https`; a `user@` is
 * dropped; an IPv6 host stays bracketed; the path keeps its `?query#fragment`; the port is kept as typed,
 * even the scheme's default. `null` for an empty value, one with whitespace, a bad scheme, a missing
 * host, a port that isn't a number from 1 to 65535, or what `URL` can't parse.
 */
export function splitPastedUrl(text: string): WebParts | null {
  const trimmed = text.trim();
  // eslint-disable-next-line no-control-regex
  if (trimmed === '' || /[\s\u0000-\u001f\u007f]/.test(trimmed)) return null;
  const marker = trimmed.indexOf('://');
  const scheme = marker < 0 ? 'https' : trimmed.slice(0, marker);
  const rest = marker < 0 ? trimmed : trimmed.slice(marker + 3);
  if (!SCHEME.test(scheme)) return null;
  const end = rest.search(/[/?#]/);
  const authority = end < 0 ? rest : rest.slice(0, end);
  const path = end < 0 ? '' : rest.slice(end);
  const hostPort = authority.slice(authority.lastIndexOf('@') + 1);
  let host: string;
  let portText: string | null;
  if (hostPort.startsWith('[')) {
    const close = hostPort.indexOf(']');
    if (close < 0) return null;
    host = hostPort.slice(0, close + 1);
    const after = hostPort.slice(close + 1);
    if (after !== '' && !after.startsWith(':')) return null;
    portText = after === '' ? null : after.slice(1);
  } else {
    const colon = hostPort.indexOf(':');
    host = colon < 0 ? hostPort : hostPort.slice(0, colon);
    portText = colon < 0 ? null : hostPort.slice(colon + 1);
  }
  if (host === '' || host === '[]') return null;
  let port: number | null = null;
  if (portText !== null) {
    if (!/^\d+$/.test(portText)) return null;
    port = Number(portText);
    if (port < 1 || port > 65535) return null;
  }
  try {
    new URL(`${scheme}://${host}${port === null ? '' : `:${port}`}${path}`);
  } catch {
    return null;
  }
  return { scheme, host, port, path };
}

/** Whether `text` is more than a host: it has a scheme, a path, a query, a fragment or a port. */
function hasUrlParts(text: string): boolean {
  const parts = splitPastedUrl(text);
  return parts !== null && (/:\/\/|[/?#]/.test(text.trim()) || parts.port !== null);
}

/**
 * A full URL in the General host of an entry with a web connection (pasted, or typed and saved): its host
 * becomes the entry's, its scheme, port and path the first web connection's, overwriting them like a paste
 * into the web fields. `false` (nothing changed) without a web connection or for a plain host.
 */
export function splitEntryHostUrl(form: EntryForm, text: string): boolean {
  const web = form.connections.find((c) => c.type === 'web');
  if (!web || !hasUrlParts(text)) return false;
  const parts = splitPastedUrl(text)!;
  form.host = parts.host;
  web.webScheme = parts.scheme;
  web.port = parts.port === null ? '' : String(parts.port);
  web.webPath = parts.path;
  return true;
}

/** The URL a web connection opens, for the preview: `scheme://host[:port][/path]`. */
export function webUrlPreview(scheme: string, host: string, port: string, path: string): string {
  const trimmedPath = path.trim();
  const joined = trimmedPath === '' || /^[/?#]/.test(trimmedPath) ? trimmedPath : `/${trimmedPath}`;
  return `${scheme.trim()}://${host.trim()}${port.trim() ? `:${port.trim()}` : ''}${joined}`;
}

/** Save is enabled once the name isn't blank; the rest is validated by Rust. */
export function canSave(name: string): boolean {
  return name.trim() !== '';
}

export type DraftField = 'name' | 'host' | 'port' | 'vpnTestAddress';

/** The field a Rust validation code belongs to, or `null` for errors shown at the bottom. */
export function errorField(code: string): DraftField | null {
  switch (code) {
    case 'emptyName':
      return 'name';
    case 'invalidUrl':
    case 'emptyHost':
      return 'host';
    case 'invalidPort':
      return 'port';
    case 'emptyVpnTestAddress':
      return 'vpnTestAddress';
    default:
      return null;
  }
}

// Credential editor

export interface CredentialForm {
  name: string;
  username: string;
  domain: string;
  sshKeyPath: string;
  notes: string;
  password: SecretState;
  passphrase: SecretState;
}

export function newCredentialForm(): CredentialForm {
  return {
    name: '',
    username: '',
    domain: '',
    sshKeyPath: '',
    notes: '',
    password: secretState(false),
    passphrase: secretState(false),
  };
}

export function credentialFormFrom(credential: CredentialDto): CredentialForm {
  return {
    name: credential.name,
    username: credential.username,
    domain: credential.domain,
    sshKeyPath: credential.sshKeyPath ?? '',
    notes: credential.notes,
    password: secretState(credential.hasPassword),
    passphrase: secretState(credential.hasPassphrase),
  };
}

export function buildCredentialSave(form: CredentialForm): {
  credential: CredentialInput;
  password: string | null;
  passphrase: string | null;
} {
  return {
    credential: {
      name: form.name,
      username: form.username,
      domain: form.domain,
      sshKeyPath: form.sshKeyPath,
      notes: form.notes,
    },
    password: secretUpdate(form.password),
    passphrase: secretUpdate(form.passphrase),
  };
}

// Pickers

export interface PickerOption {
  value: string;
  label: string;
}

/** The picker value standing for "none" (the root, own login, the default browser). */
export const NONE = '';

/** The folder picker: "(Root)", then every folder indented by 3 spaces per level. */
export function folderOptions(library: LibraryDto): PickerOption[] {
  const names = new Map(library.folders.map((f) => [f.id, f.name]));
  return [
    { value: NONE, label: t('(Root)') },
    ...library.flattenedFolders.map((f) => ({
      value: f.id,
      label: ' '.repeat(3 * f.depth) + (names.get(f.id) ?? f.id),
    })),
  ];
}

/** "name (DOMAIN\user)", or the name alone when the credential has no user. */
export function credentialLabel(credential: CredentialDto): string {
  return credential.displayUser ? `${credential.name} (${credential.displayUser})` : credential.name;
}

/** The credential picker: "Own login", then each credential. */
export function credentialOptions(library: LibraryDto, labelled = true): PickerOption[] {
  return [
    { value: NONE, label: t('Own login') },
    ...library.credentials.map((c) => ({ value: c.id, label: labelled ? credentialLabel(c) : c.name })),
  ];
}

/** The browser picker: "Default", the installed browsers, and the saved one when it isn't installed. */
export function browserOptions(saved: string | null, installed: readonly BrowserDto[] = []): PickerOption[] {
  return browserPickerOptions(saved, installed);
}

/** `""` ⇄ `null` for the pickers. */
export const fromPicker = (value: string): string | null => (value === NONE ? null : value);
export const toPicker = (value: string | null | undefined): string => value ?? NONE;
