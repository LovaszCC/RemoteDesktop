import { flushSync } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { multiEntryLibrary, sidebarLibrary } from '../testing/fixtures';
import { buttonByText, cleanupView, click, renderView, settle, text, type } from '../testing/views';
import type { ConnectionSave, LibraryDto } from '../types';
import EntryEditor from './EntryEditor.svelte';
import CredentialEditor from './CredentialEditor.svelte';
import FolderEditor from './FolderEditor.svelte';

afterEach(cleanupView);

const input = (label: string) =>
  document.querySelector<HTMLInputElement>(`input[aria-label="${label}"], #${CSS.escape(fieldId(label))}`)!;

/** The id of the input a form label points at. */
function fieldId(label: string): string {
  const element = [...document.querySelectorAll('label')].find((l) => l.textContent?.trim() === label);
  return element?.getAttribute('for') ?? `missing-${label}`;
}

function choose(label: string, value: string) {
  const select = document.querySelector<HTMLSelectElement>(`select[aria-label="${label}"]`)!;
  select.value = value;
  select.dispatchEvent(new Event('change', { bubbles: true }));
  flushSync();
}

/** A tab of the entry editor by its text. */
function tab(label: string): HTMLButtonElement {
  const found = [...document.querySelectorAll<HTMLButtonElement>('[role="tab"]')].find(
    (b) => b.textContent?.trim() === label || b.getAttribute('aria-label') === label,
  );
  if (!found) throw new Error(`no tab "${label}"`);
  return found;
}
const tabLabels = () =>
  [...document.querySelectorAll('[role="tab"]')].map(
    (b) => b.getAttribute('aria-label') || b.textContent?.trim(),
  );
const selectedTab = () => document.querySelector('[role="tab"][aria-selected="true"]')?.textContent?.trim();

function paste(element: HTMLInputElement, value: string) {
  const event = new Event('paste', { bubbles: true, cancelable: true });
  Object.defineProperty(event, 'clipboardData', { value: { getData: () => value } });
  element.dispatchEvent(event);
  flushSync();
}

function key(element: Element, name: string) {
  element.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true }));
  flushSync();
}

const edit = (id: string) => ({ request: { mode: 'edit' as const, id }, onclose: () => {} });

describe('EntryEditor', () => {
  it('editor_has_general_and_one_tab_per_connection', () => {
    renderView(EntryEditor, edit('e1'), { library: multiEntryLibrary() });
    expect(tabLabels()).toEqual(['General', 'SSH · root', 'Console', 'Add connection']);
    expect(selectedTab()).toBe('General');
    expect(input('Name').value).toBe('Home Server');
    expect(input('Host / IP').value).toBe('home.example.test');
    expect(tab('SSH · root').querySelector('[aria-label="Default connection"]')).toBeTruthy();
    expect(tab('Console').querySelector('[aria-label="Default connection"]')).toBeNull();
    click(tab('Console'));
    expect(selectedTab()).toBe('Console');
    expect(input('Label').value).toBe('Console');
    expect(text()).toContain('RDP client');
  });

  it('opens on the requested connection tab', () => {
    renderView(
      EntryEditor,
      { request: { mode: 'edit', id: 'e1', connectionId: 'h2' }, onclose: () => {} },
      { library: multiEntryLibrary() },
    );
    expect(selectedTab()).toBe('Console');
    const panel = document.querySelector('[role="tabpanel"]')!;
    expect(document.getElementById(panel.getAttribute('aria-labelledby')!)?.textContent?.trim()).toBe(
      'Console',
    );
  });

  it('moves between tabs with the arrow keys', () => {
    renderView(EntryEditor, edit('e1'), { library: multiEntryLibrary() });
    key(tab('General'), 'ArrowRight');
    expect(selectedTab()).toBe('SSH · root');
    expect(document.activeElement).toBe(tab('SSH · root'));
    key(tab('SSH · root'), 'End');
    expect(document.activeElement?.getAttribute('aria-label')).toBe('Add connection');
    key(tab('Add connection'), 'ArrowRight');
    expect(selectedTab()).toBe('General');
    key(tab('General'), 'ArrowLeft');
    expect(document.activeElement?.getAttribute('aria-label')).toBe('Add connection');
    key(tab('Add connection'), 'Home');
    expect(selectedTab()).toBe('General');
  });

  it('creates a new SSH entry: General first, untouched secrets null, then selects it', async () => {
    let closed = false;
    const { bridge, store } = renderView(
      EntryEditor,
      { request: { mode: 'new', type: 'ssh', folderId: 'f2' }, onclose: () => (closed = true) },
      { replies: { save_entry: 'new-id' } },
    );
    expect(text()).toContain('New entry');
    expect(tabLabels()).toEqual(['General', 'SSH', 'Add connection']);
    expect(selectedTab()).toBe('General');
    expect(buttonByText('Create').disabled).toBe(true);
    type(input('Name'), 'web01');
    type(input('Host / IP'), 'web01.local');
    click(tab('SSH'));
    expect(input('Port').placeholder).toBe('22');
    type(input('SSH key'), '~/.ssh/id');
    expect(text()).toContain('Key passphrase');
    click(buttonByText('Create'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('save_entry', {
      id: null,
      entry: {
        name: 'web01',
        host: 'web01.local',
        folderId: 'f2',
        notes: '',
        color: null,
        isFavorite: false,
      },
      connections: [
        {
          id: null,
          draft: expect.objectContaining({ type: 'ssh', sshKeyPath: '~/.ssh/id', domain: '', port: '' }),
          password: null,
          passphrase: null,
          isDefault: true,
        },
      ],
    });
    expect(closed).toBe(true);
    expect(store.selection).toEqual(['e:new-id']);
  });

  it('lets a new connection change its type', () => {
    renderView(EntryEditor, { request: { mode: 'new', type: 'ssh', folderId: null }, onclose: () => {} });
    click(tab('SSH'));
    click(buttonByText('RDP'));
    expect(tabLabels()).toEqual(['General', 'RDP', 'Add connection']);
    expect(input('Port').placeholder).toBe('3389');
  });

  it('plus_tab_adds_a_connection_of_the_chosen_type', async () => {
    const { bridge } = renderView(EntryEditor, edit('e1'), {
      library: multiEntryLibrary(),
      replies: { save_entry: 'e1' },
    });
    click(tab('Add connection'));
    expect(text()).toContain('Type');
    click(buttonByText('Web'));
    expect(tabLabels()).toEqual(['General', 'SSH · root', 'Console', 'Web', 'Add connection']);
    expect(selectedTab()).toBe('Web');
    expect(input('Scheme').value).toBe('https');
    click(buttonByText('Save'));
    await settle();
    const call = bridge.invoke.mock.calls.find(([command]) => command === 'save_entry');
    const sent = (call?.[1] as { connections: ConnectionSave[] }).connections;
    expect(sent.map((c) => [c.id, c.draft.type, c.isDefault])).toEqual([
      ['h1', 'ssh', true],
      ['h2', 'rdp', false],
      [null, 'web', false],
    ]);
  });

  it('removes a connection and moves the default', async () => {
    const { bridge } = renderView(EntryEditor, edit('e1'), {
      library: multiEntryLibrary(),
      replies: { save_entry: 'e1' },
    });
    click(tab('SSH · root'));
    click(buttonByText('Remove this connection'));
    expect(tabLabels()).toEqual(['General', 'Console', 'Add connection']);
    expect(tab('Console').querySelector('[aria-label="Default connection"]')).toBeTruthy();
    click(buttonByText('Save'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith(
      'save_entry',
      expect.objectContaining({
        connections: [expect.objectContaining({ id: 'h2', isDefault: true })],
      }),
    );
  });

  it('paste_full_url_fills_scheme_port_path_and_empty_host', () => {
    renderView(EntryEditor, { request: { mode: 'new', type: 'web', folderId: null }, onclose: () => {} });
    click(tab('Web'));
    paste(input('Scheme'), 'http://192.168.1.10:8443/admin?x=1');
    expect(input('Scheme').value).toBe('http');
    expect(input('Port').value).toBe('8443');
    expect(input('Path').value).toBe('/admin?x=1');
    expect(text()).toContain('http://192.168.1.10:8443/admin?x=1');
    click(tab('General'));
    expect(input('Host / IP').value).toBe('192.168.1.10');
  });

  it('paste_with_different_host_offers_use_host', () => {
    renderView(EntryEditor, edit('c5'));
    click(tab('Web'));
    paste(input('Path'), 'smb://other.example/share');
    expect(input('Scheme').value).toBe('smb');
    expect(input('Path').value).toBe('/share');
    expect(text()).toContain('The URL is on other.example, this entry on c5.example.test.');
    click(buttonByText('Use other.example'));
    expect(text()).not.toContain('Use other.example');
    click(tab('General'));
    expect(input('Host / IP').value).toBe('other.example');
  });

  it('splits a full URL pasted into the General host of a web entry', () => {
    renderView(EntryEditor, edit('c5'));
    expect(text()).toContain('Edit entry');
    paste(input('Host / IP'), 'http://nas.local:8080/admin?x=1');
    expect(input('Host / IP').value).toBe('nas.local');
    click(tab('Web'));
    expect([input('Scheme').value, input('Port').value, input('Path').value]).toEqual([
      'http',
      '8080',
      '/admin?x=1',
    ]);
    // A plain host is pasted as text.
    click(tab('General'));
    paste(input('Host / IP'), 'other.local');
    expect(input('Host / IP').value).toBe('nas.local');
  });

  it('splits a typed full URL in the General host on Save', async () => {
    const { bridge } = renderView(EntryEditor, edit('c5'), { replies: { save_entry: 'c5' } });
    type(input('Host / IP'), 'smb://files.local/share');
    click(buttonByText('Save'));
    await settle();
    const call = bridge.invoke.mock.calls.find(([command]) => command === 'save_entry');
    const sent = call?.[1] as { entry: { host: string }; connections: ConnectionSave[] };
    expect(sent.entry.host).toBe('files.local');
    expect(sent.connections[0]!.draft).toMatchObject({ webScheme: 'smb', port: '', webPath: '/share' });
  });

  it('leaves a URL in the host of an entry without a web connection to Rust', async () => {
    const { bridge } = renderView(EntryEditor, edit('c1'), { replies: { save_entry: 'c1' } });
    type(input('Host / IP'), 'https://web01.local/x');
    click(buttonByText('Save'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith(
      'save_entry',
      expect.objectContaining({ entry: expect.objectContaining({ host: 'https://web01.local/x' }) }),
    );
  });

  it('disables removing the last connection', () => {
    renderView(EntryEditor, edit('c1'));
    click(tab('SSH'));
    expect(buttonByText('Remove this connection').disabled).toBe(true);
  });

  it('closes for an entry that does not exist', async () => {
    let closed = false;
    renderView(EntryEditor, { request: { mode: 'edit', id: 'gone' }, onclose: () => (closed = true) });
    await settle();
    expect(closed).toBe(true);
  });

  it('pastes a plain path as text', () => {
    renderView(EntryEditor, edit('c5'));
    click(tab('Web'));
    paste(input('Path'), '/admin');
    expect(input('Path').value).toBe('');
    expect(input('Scheme').value).toBe('https');
  });

  it('save_sends_save_entry_with_all_connections', async () => {
    let closed = false;
    const { bridge } = renderView(
      EntryEditor,
      { request: { mode: 'edit', id: 'e1' }, onclose: () => (closed = true) },
      { library: multiEntryLibrary(), replies: { save_entry: 'e1' } },
    );
    type(input('Name'), 'Home 2');
    click(tab('Console'));
    click(buttonByText('Set as default'));
    expect(tab('Console').querySelector('[aria-label="Default connection"]')).toBeTruthy();
    expect(tab('SSH · root').querySelector('[aria-label="Default connection"]')).toBeNull();
    click(buttonByText('Save'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('save_entry', {
      id: 'e1',
      entry: {
        name: 'Home 2',
        host: 'home.example.test',
        folderId: null,
        notes: '',
        color: null,
        isFavorite: false,
      },
      connections: [
        {
          id: 'h1',
          draft: expect.objectContaining({ type: 'ssh', username: 'root', label: '' }),
          password: null,
          passphrase: null,
          isDefault: false,
        },
        {
          id: 'h2',
          draft: expect.objectContaining({ type: 'rdp', label: 'Console', showAsButton: true }),
          password: null,
          passphrase: null,
          isDefault: true,
        },
      ],
    });
    expect(closed).toBe(true);
  });

  it('invalid_port_selects_that_connection_tab', async () => {
    let closed = false;
    renderView(
      EntryEditor,
      { request: { mode: 'edit', id: 'e1' }, onclose: () => (closed = true) },
      {
        library: multiEntryLibrary(),
        replies: {
          save_entry: () => {
            throw {
              code: 'invalidPort',
              message: 'The port must be a number from 1 to 65535.',
              detail: null,
            };
          },
        },
      },
    );
    click(tab('Console'));
    type(input('Port'), '99999');
    click(tab('General'));
    click(buttonByText('Save'));
    await settle();
    expect(closed).toBe(false);
    expect(selectedTab()).toBe('Console');
    expect(document.querySelector('[role="alert"]')?.textContent).toBe(
      'The port must be a number from 1 to 65535.',
    );
    expect(input('Port').getAttribute('aria-invalid')).toBe('true');
  });

  it('shows name and host errors on General and other errors in the footer', async () => {
    let code = 'emptyHost';
    renderView(EntryEditor, edit('e1'), {
      library: multiEntryLibrary(),
      replies: {
        save_entry: () => {
          throw { code, message: code, detail: null };
        },
      },
    });
    click(tab('Console'));
    click(buttonByText('Save'));
    await settle();
    expect(selectedTab()).toBe('General');
    expect(input('Host / IP').getAttribute('aria-invalid')).toBe('true');
    code = 'emptyName';
    click(tab('Console'));
    click(buttonByText('Save'));
    await settle();
    expect(selectedTab()).toBe('General');
    expect(input('Name').getAttribute('aria-invalid')).toBe('true');
    code = 'noConnections';
    click(buttonByText('Save'));
    await settle();
    expect(document.querySelector('.failure')?.textContent).toBe('An entry needs at least one connection.');
  });

  it('clears a stored password explicitly', async () => {
    const library: LibraryDto = sidebarLibrary();
    library.connections[0] = { ...library.connections[0]!, hasPassword: true };
    const { bridge } = renderView(EntryEditor, edit('c1'), { library, replies: { save_entry: 'c1' } });
    click(tab('SSH'));
    expect(input('Password').placeholder).toBe('unchanged');
    click(buttonByText('Clear'));
    expect(text()).toContain('Removed on save');
    click(buttonByText('Save'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith(
      'save_entry',
      expect.objectContaining({
        id: 'c1',
        connections: [expect.objectContaining({ id: 'c1', password: '', passphrase: null })],
      }),
    );
  });

  it('shows the linked credential and the no-username note', () => {
    const library = sidebarLibrary();
    library.credentials[1] = { ...library.credentials[1]!, username: '', displayUser: '' };
    renderView(EntryEditor, edit('c2'), { library });
    click(tab('RDP'));
    choose('Credential', 'k1');
    expect(text()).toContain('admin');
    expect(text()).not.toContain('Browse…');
    expect(tabLabels()).toContain('RDP · admin');
    choose('Credential', 'k2');
    expect(text()).toContain('The credential has no username – this connection uses its own.');
    expect(input('Domain')).toBeTruthy();
  });

  it('labels the system RDP client per platform', () => {
    renderView(
      EntryEditor,
      { request: { mode: 'new', type: 'rdp', folderId: null }, onclose: () => {} },
      { platform: 'windows' },
    );
    click(tab('RDP'));
    expect(text()).toContain('System RDP client');
    expect(text()).toContain('Admin session (/admin)');
  });

  it('closes when the edited entry is deleted', () => {
    let closed = false;
    const { store } = renderView(
      EntryEditor,
      { request: { mode: 'edit', id: 'e1' }, onclose: () => (closed = true) },
      { library: multiEntryLibrary() },
    );
    store.replace(sidebarLibrary());
    flushSync();
    expect(closed).toBe(true);
  });
});

describe('EntryEditor browser picker', () => {
  const installed = [
    { id: 'com.brave.Browser', name: 'Brave Browser' },
    { id: 'org.mozilla.firefox', name: 'Firefox' },
  ];
  const options = () =>
    [...document.querySelectorAll<HTMLOptionElement>('select[aria-label="Browser"] option')].map((o) => [
      o.value,
      o.textContent?.trim(),
    ]);

  it('lists the installed browsers by name and keeps a saved browser that is missing', async () => {
    const library = sidebarLibrary();
    library.connections[4] = { ...library.connections[4]!, browserBundleId: 'com.example.gone' };
    const { bridge } = renderView(EntryEditor, edit('c5'), {
      library,
      replies: { installed_browsers: installed },
    });
    click(tab('Web'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('installed_browsers', undefined);
    expect(options()).toEqual([
      ['', 'Default'],
      ['com.brave.Browser', 'Brave Browser'],
      ['org.mozilla.firefox', 'Firefox'],
      ['com.example.gone', 'com.example.gone (not installed)'],
    ]);
  });

  it('saves the chosen browser id', async () => {
    const { bridge } = renderView(
      EntryEditor,
      { request: { mode: 'new', type: 'web', folderId: null }, onclose: () => {} },
      { replies: { installed_browsers: installed, save_entry: 'new-id' } },
    );
    type(input('Name'), 'Portal');
    type(input('Host / IP'), 'portal.example.test');
    click(tab('Web'));
    await settle();
    choose('Browser', 'org.mozilla.firefox');
    click(buttonByText('Create'));
    await settle();
    const call = bridge.invoke.mock.calls.find(([command]) => command === 'save_entry');
    const sent = (call?.[1] as { connections: ConnectionSave[] }).connections;
    expect(sent[0]!.draft.browserBundleId).toBe('org.mozilla.firefox');
  });
});

describe('CredentialEditor', () => {
  it('keeps the sheet open with the key error in red', async () => {
    let closed = false;
    renderView(
      CredentialEditor,
      { request: { mode: 'new' }, onclose: () => (closed = true) },
      {
        replies: {
          save_credential: () => {
            throw { code: 'cannotReadKey', message: "Can't read the SSH key: %@", detail: '~/.ssh/gone' };
          },
        },
      },
    );
    type(input('Name'), 'Ops');
    type(input('SSH key'), '~/.ssh/gone');
    type(input('Key passphrase'), 'secret');
    click(buttonByText('Create'));
    await settle();
    expect(closed).toBe(false);
    expect(document.querySelector('.failure')?.textContent).toContain("Can't read the SSH key: ~/.ssh/gone");
  });

  it('creates a credential and selects it', async () => {
    const { bridge, store } = renderView(
      CredentialEditor,
      { request: { mode: 'new' }, onclose: () => {} },
      { replies: { save_credential: 'k9' } },
    );
    type(input('Name'), 'Ops');
    type(input('Password'), 'pw');
    click(buttonByText('Create'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('save_credential', {
      id: null,
      credential: { name: 'Ops', username: '', domain: '', sshKeyPath: '', notes: '' },
      password: 'pw',
      passphrase: null,
    });
    expect(store.selection).toEqual(['k:k9']);
  });
});

describe('FolderEditor', () => {
  it('creates a folder in the chosen parent with a colour', async () => {
    const { bridge } = renderView(
      FolderEditor,
      { request: { mode: 'new', parentId: 'f1' }, onclose: () => {} },
      { replies: { save_folder: 'f9' } },
    );
    expect(text()).toContain('New folder');
    expect(document.querySelector<HTMLSelectElement>('select')!.value).toBe('f1');
    type(input('Name'), ' Racks ');
    click(document.querySelector('[aria-label="Green"]')!);
    click(buttonByText('Create'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('save_folder', {
      folder: { parentId: 'f1', name: 'Racks', color: 'green' },
    });
  });

  it('renames a folder, and clicking its colour again clears it', async () => {
    const { bridge } = renderView(
      FolderEditor,
      { request: { mode: 'edit', id: 'f1' }, onclose: () => {} },
      { replies: { save_folder: 'f1' } },
    );
    expect(text()).toContain('Rename folder');
    expect(document.querySelector('select')).toBeNull();
    click(document.querySelector('[aria-label="Red"]')!);
    click(buttonByText('Save'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('save_folder', {
      folder: { id: 'f1', name: 'Office', color: null },
    });
  });
});

describe('SshKeyField', () => {
  it('generates a key pair named after the entry and fills the path', async () => {
    const { bridge, toast } = renderView(
      CredentialEditor,
      { request: { mode: 'new' }, onclose: () => {} },
      { replies: { ssh_key_generate: '~/.ssh/rdnext_ops_ed25519' } },
    );
    type(input('Name'), ' Ops ');
    click(buttonByText('Generate…'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_key_generate', { name: 'Ops' });
    expect(input('SSH key').value).toBe('~/.ssh/rdnext_ops_ed25519');
    expect(toast.current?.message).toBe('SSH key created: ~/.ssh/rdnext_ops_ed25519');
  });

  it('shows a generation failure in the alert', async () => {
    const { alerts } = renderView(
      EntryEditor,
      { request: { mode: 'new', type: 'ssh', folderId: null }, onclose: () => {} },
      {
        replies: {
          ssh_key_generate: () => {
            throw {
              code: 'keyGenerateFailed',
              message: "Couldn't create the SSH key: %@",
              detail: 'disk full',
            };
          },
        },
      },
    );
    click(tab('SSH'));
    click(buttonByText('Generate…'));
    await settle();
    expect(alerts.current).toBe("Couldn't create the SSH key: disk full");
    expect(input('SSH key').value).toBe('');
    expect(buttonByText('Generate…').disabled).toBe(false);
  });
});
