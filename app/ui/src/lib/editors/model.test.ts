import { describe, expect, it } from 'vitest';
import { connection, sidebarLibrary } from '../testing/fixtures';
import {
  browserOptions,
  buildConnectionSave,
  buildCredentialSave,
  canSave,
  clearSecret,
  connectionFormFrom,
  credentialLabel,
  credentialOptions,
  errorField,
  folderOptions,
  fromPicker,
  keepSecret,
  keepsStored,
  newConnectionForm,
  newCredentialForm,
  secretPlaceholder,
  secretState,
  secretUpdate,
  toPicker,
  type ConnectionForm,
} from './model';

describe('secret fields', () => {
  it('keeps the stored secret while untouched', () => {
    expect(secretUpdate(secretState(true))).toBeNull();
    expect(secretUpdate(secretState(false))).toBeNull();
  });

  it('sends "" when the user clears it', () => {
    const cleared = clearSecret(secretState(true));
    expect(secretUpdate(cleared)).toBe('');
    expect(keepsStored(cleared)).toBe(false);
    expect(secretUpdate(keepSecret(cleared))).toBeNull();
  });

  it('sends the typed value, also after clearing', () => {
    expect(secretUpdate({ ...secretState(true), text: 'pw' })).toBe('pw');
    expect(secretUpdate({ ...clearSecret(secretState(true)), text: 'new' })).toBe('new');
  });

  it('treats a field typed into and emptied again as untouched', () => {
    expect(secretUpdate({ ...secretState(true), text: '' })).toBeNull();
  });

  it('shows "unchanged" only while a stored secret is kept', () => {
    expect(secretPlaceholder(secretState(true))).toBe('unchanged');
    expect(secretPlaceholder(secretState(false))).toBe('');
    expect(secretPlaceholder(clearSecret(secretState(true)))).toBe('');
    expect(keepsStored(secretState(true))).toBe(true);
    expect(keepsStored({ ...secretState(true), text: 'x' })).toBe(false);
  });
});

function filled(type: ConnectionForm['type']): ConnectionForm {
  return {
    ...newConnectionForm(type, 'f1'),
    name: 'n',
    host: 'h',
    port: ' 2222 ',
    username: 'u',
    domain: 'CORP',
    sshKeyPath: '~/.ssh/id',
    rdpAdminSession: true,
    browserBundleId: 'org.example.browser',
    password: { text: 'pw', stored: false, cleared: false },
    passphrase: { text: 'pp', stored: false, cleared: false },
  };
}

describe('buildConnectionSave', () => {
  it('keeps the RDP fields and drops the SSH and web ones', () => {
    const { draft, password, passphrase } = buildConnectionSave(filled('rdp'));
    expect(draft).toMatchObject({ type: 'rdp', port: '2222', domain: 'CORP', rdpAdminSession: true });
    expect(draft.sshKeyPath).toBe('');
    expect(draft.browserBundleId).toBeNull();
    expect(password).toBe('pw');
    expect(passphrase).toBeNull();
  });

  it('keeps the key and passphrase for SSH types only', () => {
    for (const type of ['ssh', 'mikrotik'] as const) {
      const { draft, passphrase } = buildConnectionSave(filled(type));
      expect(draft).toMatchObject({ sshKeyPath: '~/.ssh/id', domain: '', rdpAdminSession: false });
      expect(draft.browserBundleId).toBeNull();
      expect(passphrase).toBe('pp');
    }
  });

  it('drops the port for web and keeps the browser', () => {
    const { draft, passphrase } = buildConnectionSave(filled('web'));
    expect(draft).toMatchObject({
      port: '',
      browserBundleId: 'org.example.browser',
      domain: '',
      sshKeyPath: '',
    });
    expect(passphrase).toBeNull();
  });

  it('sends null secrets for an untouched existing connection', () => {
    const form = connectionFormFrom({
      ...connection('c1'),
      hasPassword: true,
      hasPassphrase: true,
      port: 2200,
    });
    expect(form.port).toBe('2200');
    const save = buildConnectionSave(form);
    expect(save.password).toBeNull();
    expect(save.passphrase).toBeNull();
    expect(form.password.stored).toBe(true);
  });
});

describe('credential editor', () => {
  it('builds the input with the secret rule', () => {
    const form = { ...newCredentialForm(), name: 'Admin', password: clearSecret(secretState(true)) };
    expect(buildCredentialSave(form)).toEqual({
      credential: { name: 'Admin', username: '', domain: '', sshKeyPath: '', notes: '' },
      password: '',
      passphrase: null,
    });
  });
});

describe('validation and pickers', () => {
  it('needs a name to save', () => {
    expect(canSave('  ')).toBe(false);
    expect(canSave(' a ')).toBe(true);
  });

  it('maps Rust validation codes to fields', () => {
    expect(errorField('emptyName')).toBe('name');
    expect(errorField('invalidUrl')).toBe('host');
    expect(errorField('emptyHost')).toBe('host');
    expect(errorField('invalidPort')).toBe('port');
    expect(errorField('database')).toBeNull();
  });

  it('indents the folder picker under (Root)', () => {
    expect(folderOptions(sidebarLibrary()).map((o) => [o.value, o.label])).toEqual([
      ['', '(Root)'],
      ['f3', 'Lab'],
      ['f1', 'Office'],
      ['f2', '   Servers'],
    ]);
  });

  it('labels credentials with their user', () => {
    const library = sidebarLibrary();
    expect(credentialLabel(library.credentials[0]!)).toBe('Domain admin (admin)');
    expect(credentialLabel({ ...library.credentials[0]!, displayUser: '' })).toBe('Domain admin');
    expect(credentialOptions(library).map((o) => o.label)).toEqual([
      'Own login',
      'Domain admin (admin)',
      'Spare (admin)',
    ]);
  });

  it('offers the default browser plus the saved one', () => {
    expect(browserOptions(null)).toEqual([{ value: '', label: 'Default' }]);
    expect(browserOptions('x.y')).toHaveLength(2);
    expect(browserOptions(null, [{ id: 'x.y', name: 'XY' }])).toEqual([
      { value: '', label: 'Default' },
      { value: 'x.y', label: 'XY' },
    ]);
    expect(fromPicker('')).toBeNull();
    expect(toPicker(null)).toBe('');
  });
});
