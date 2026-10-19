import { describe, expect, it } from 'vitest';
import { connection, multiEntryLibrary, sidebarLibrary } from '../testing/fixtures';
import {
  browserOptions,
  buildConnectionSave,
  buildCredentialSave,
  canSave,
  clearSecret,
  connectionFormFrom,
  connectionTabLabel,
  credentialLabel,
  credentialOptions,
  buildEntrySave,
  entryFormFrom,
  errorField,
  folderOptions,
  fromPicker,
  invalidPortKey,
  keepSecret,
  keepsStored,
  newConnectionForm,
  newCredentialForm,
  newEntryForm,
  secretPlaceholder,
  secretState,
  secretUpdate,
  splitPastedUrl,
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
    ...newConnectionForm(type),
    port: ' 2222 ',
    username: 'u',
    domain: 'CORP',
    sshKeyPath: '~/.ssh/id',
    rdpAdminSession: true,
    browserBundleId: 'org.example.browser',
    webScheme: ' smb ',
    webPath: '/share',
    label: 'Main',
    password: { text: 'pw', stored: false, cleared: false },
    passphrase: { text: 'pp', stored: false, cleared: false },
  };
}

describe('buildConnectionSave', () => {
  it('keeps the RDP fields and drops the SSH and web ones', () => {
    const { id, draft, password, passphrase, isDefault } = buildConnectionSave(filled('rdp'), true);
    expect(id).toBeNull();
    expect(isDefault).toBe(true);
    expect(draft).toMatchObject({
      type: 'rdp',
      port: '2222',
      domain: 'CORP',
      rdpAdminSession: true,
      label: 'Main',
      showAsButton: true,
    });
    expect(draft.sshKeyPath).toBe('');
    expect(draft.browserBundleId).toBeNull();
    expect(draft.webPath).toBe('');
    expect(password).toBe('pw');
    expect(passphrase).toBeNull();
  });

  it('keeps the key and passphrase for SSH types only', () => {
    for (const type of ['ssh', 'mikrotik'] as const) {
      const { draft, passphrase } = buildConnectionSave(filled(type), false);
      expect(draft).toMatchObject({ sshKeyPath: '~/.ssh/id', domain: '', rdpAdminSession: false });
      expect(draft.browserBundleId).toBeNull();
      expect(passphrase).toBe('pp');
    }
  });

  it('keeps the port, scheme, path and browser for web', () => {
    const { draft, passphrase } = buildConnectionSave(filled('web'), false);
    expect(draft).toMatchObject({
      port: '2222',
      webScheme: 'smb',
      webPath: '/share',
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
    const save = buildConnectionSave(form, false);
    expect(save.id).toBe('c1');
    expect(save.password).toBeNull();
    expect(save.passphrase).toBeNull();
    expect(form.password.stored).toBe(true);
  });
});

describe('entry form', () => {
  it('build_entry_save_orders_and_marks_default', () => {
    const library = multiEntryLibrary();
    const entry = library.entries.find((e) => e.id === 'e1')!;
    const connections = library.connections.filter((c) => c.entryId === 'e1');
    const form = entryFormFrom({ ...entry, defaultConnectionId: 'h2' }, connections);
    expect(form.defaultKey).toBe('h2');
    const added = newConnectionForm('web');
    form.connections = [form.connections[1]!, added, form.connections[0]!];
    form.name = 'Home';
    const save = buildEntrySave(form);
    expect(save.entry).toEqual({
      name: 'Home',
      host: 'home.example.test',
      folderId: null,
      notes: '',
      color: null,
      isFavorite: false,
    });
    expect(save.connections.map((c) => [c.id, c.draft.type, c.isDefault])).toEqual([
      ['h2', 'rdp', true],
      [null, 'web', false],
      ['h1', 'ssh', false],
    ]);
    form.defaultKey = added.key;
    expect(buildEntrySave(form).connections.map((c) => c.isDefault)).toEqual([false, true, false]);
  });

  it('starts a new entry with one connection of the type, its default', () => {
    const form = newEntryForm('ssh', 'f1');
    expect(form).toMatchObject({ name: '', host: '', folderId: 'f1' });
    expect(form.connections.map((c) => c.type)).toEqual(['ssh']);
    expect(form.defaultKey).toBe(form.connections[0]!.key);
    expect(newConnectionForm('ssh').key).not.toBe(form.connections[0]!.key);
  });

  it('labels a connection tab like the sidebar', () => {
    const form = newConnectionForm('ssh');
    expect(connectionTabLabel(form)).toBe('SSH');
    expect(connectionTabLabel({ ...form, username: 'root' })).toBe('SSH · root');
    const credential = sidebarLibrary().credentials[0]!;
    expect(connectionTabLabel({ ...form, username: 'root' }, credential)).toBe('SSH · admin');
    expect(connectionTabLabel({ ...form, label: ' Console ' })).toBe('Console');
  });

  it('finds the connection with an invalid port', () => {
    const form = newEntryForm('ssh', null);
    const rdp = { ...newConnectionForm('rdp'), port: '99999' };
    form.connections.push(rdp);
    expect(invalidPortKey(form)).toBe(rdp.key);
    rdp.port = '3390';
    form.connections[0]!.port = 'x';
    expect(invalidPortKey(form)).toBe(form.connections[0]!.key);
    form.connections[0]!.port = '';
    expect(invalidPortKey(form)).toBeNull();
  });
});

describe('splitPastedUrl', () => {
  it('split_pasted_url_cases', () => {
    const cases: [string, ReturnType<typeof splitPastedUrl>][] = [
      [
        'https://192.168.1.10:8443/admin',
        { scheme: 'https', host: '192.168.1.10', port: 8443, path: '/admin' },
      ],
      [
        'smb://homeserver.local/share',
        { scheme: 'smb', host: 'homeserver.local', port: null, path: '/share' },
      ],
      ['http://user@host/x?y=1#z', { scheme: 'http', host: 'host', port: null, path: '/x?y=1#z' }],
      ['https://[::1]:8443', { scheme: 'https', host: '[::1]', port: 8443, path: '' }],
      ['https://user@[2001:db8::1]/x', { scheme: 'https', host: '[2001:db8::1]', port: null, path: '/x' }],
      ['example.com', { scheme: 'https', host: 'example.com', port: null, path: '' }],
      [
        '  router.example.com/admin ',
        { scheme: 'https', host: 'router.example.com', port: null, path: '/admin' },
      ],
      ['https://example.com:443', { scheme: 'https', host: 'example.com', port: 443, path: '' }],
    ];
    for (const [text, expected] of cases) expect(splitPastedUrl(text), text).toEqual(expected);
    for (const bad of [
      '',
      'not a url',
      'https://',
      'https://host:port',
      'https://host:',
      'https://host:99999',
      'https://h:0',
      '1http://example.com',
      'https:///path',
      'exa\tmple.com',
      'https://[::1',
      'https://[]',
    ]) {
      expect(splitPastedUrl(bad), bad).toBeNull();
    }
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
