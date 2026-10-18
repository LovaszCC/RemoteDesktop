// The editors' pure logic (Swift `ConnectionDraft`, `CredentialDraft`, `FolderEditorView`): the editable
// form state, the secret field rule, the draft sent to Rust with the fields each type doesn't use left
// out, the picker options and where a validation error from Rust shows.

import { t } from '../i18n/index.svelte';
import { usesSSH } from '../sidebar/model';
import { browserPickerOptions } from '../stores/browsers.svelte';
import type {
  BrowserDto,
  ConnectionDraft,
  ConnectionDto,
  ConnectionType,
  CredentialDto,
  CredentialInput,
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

// Connection editor

export interface ConnectionForm {
  type: ConnectionType;
  folderId: string | null;
  credentialId: string | null;
  name: string;
  host: string;
  port: string;
  username: string;
  domain: string;
  sshKeyPath: string;
  notes: string;
  isFavorite: boolean;
  rdpAdminSession: boolean;
  rdpClient: RdpClient;
  browserBundleId: string | null;
  mikrotikMode: MikrotikMode;
  sshTerminal: SshTerminal;
  color: LabelColor | null;
  password: SecretState;
  passphrase: SecretState;
}

/** A blank form for a new connection (Swift `ConnectionDraft(type:folderId:)`). */
export function newConnectionForm(type: ConnectionType, folderId: string | null): ConnectionForm {
  return {
    type,
    folderId,
    credentialId: null,
    name: '',
    host: '',
    port: '',
    username: '',
    domain: '',
    sshKeyPath: '',
    notes: '',
    isFavorite: false,
    rdpAdminSession: false,
    rdpClient: 'embedded',
    browserBundleId: null,
    mikrotikMode: 'winbox',
    sshTerminal: 'embedded',
    color: null,
    password: secretState(false),
    passphrase: secretState(false),
  };
}

/** The form for an existing connection; its own secrets are only flagged. */
export function connectionFormFrom(connection: ConnectionDto): ConnectionForm {
  return {
    type: connection.type,
    folderId: connection.folderId,
    credentialId: connection.credentialId,
    name: connection.name,
    host: connection.host,
    port: connection.port === null ? '' : String(connection.port),
    username: connection.username,
    domain: connection.domain,
    sshKeyPath: connection.sshKeyPath ?? '',
    notes: connection.notes,
    isFavorite: connection.isFavorite,
    rdpAdminSession: connection.rdpAdminSession,
    rdpClient: connection.rdpClient,
    browserBundleId: connection.browserBundleId,
    mikrotikMode: connection.mikrotikMode,
    sshTerminal: connection.sshTerminal,
    color: connection.color,
    password: secretState(connection.hasPassword),
    passphrase: secretState(connection.hasPassphrase),
  };
}

/** Swift `ConnectionType.defaultPort`, the port field's placeholder. */
export const DEFAULT_PORTS: Record<ConnectionType, number> = { rdp: 3389, ssh: 22, web: 443, mikrotik: 22 };

/** What `save_connection` receives. */
export interface ConnectionSave {
  draft: ConnectionDraft;
  password: string | null;
  passphrase: string | null;
}

/**
 * The draft for Rust (which validates, trims and drops the same fields again): the port only for
 * non-web types, the domain and admin flag only for RDP, the key path and passphrase only for SSH
 * types, the browser only for web. Untouched secrets are `null` (kept).
 */
export function buildConnectionSave(form: ConnectionForm): ConnectionSave {
  const ssh = usesSSH(form.type);
  const rdp = form.type === 'rdp';
  const web = form.type === 'web';
  return {
    draft: {
      type: form.type,
      folderId: form.folderId,
      credentialId: form.credentialId,
      name: form.name,
      host: form.host,
      port: web ? '' : form.port.trim(),
      username: form.username,
      domain: rdp ? form.domain : '',
      sshKeyPath: ssh ? form.sshKeyPath : '',
      notes: form.notes,
      isFavorite: form.isFavorite,
      rdpAdminSession: rdp && form.rdpAdminSession,
      rdpClient: form.rdpClient,
      browserBundleId: web ? form.browserBundleId : null,
      mikrotikMode: form.mikrotikMode,
      sshTerminal: form.sshTerminal,
      color: form.color,
    },
    password: secretUpdate(form.password),
    passphrase: ssh ? secretUpdate(form.passphrase) : null,
  };
}

/** Save is enabled once the name isn't blank; the rest is validated by Rust. */
export function canSave(name: string): boolean {
  return name.trim() !== '';
}

export type DraftField = 'name' | 'host' | 'port';

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
