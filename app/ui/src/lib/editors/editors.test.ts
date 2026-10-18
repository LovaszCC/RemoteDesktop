import { flushSync } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { sidebarLibrary } from '../testing/fixtures';
import { buttonByText, cleanupView, click, renderView, settle, text, type } from '../testing/views';
import type { LibraryDto } from '../types';
import ConnectionEditor from './ConnectionEditor.svelte';
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

describe('ConnectionEditor', () => {
  it('creates a connection, sends untouched secrets as null and selects it', async () => {
    let closed = false;
    const { bridge, store } = renderView(
      ConnectionEditor,
      { request: { mode: 'new', type: 'rdp', folderId: 'f2' }, onclose: () => (closed = true) },
      { replies: { save_connection: 'new-id' } },
    );
    expect(buttonByText('Create').disabled).toBe(true);
    click(buttonByText('SSH'));
    type(input('Name'), 'web01');
    type(input('Host / IP'), 'web01.local');
    expect(input('Port').placeholder).toBe('22');
    type(input('SSH key'), '~/.ssh/id');
    expect(text()).toContain('Key passphrase');
    click(buttonByText('Create'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('save_connection', {
      id: null,
      draft: expect.objectContaining({
        type: 'ssh',
        folderId: 'f2',
        name: 'web01',
        host: 'web01.local',
        sshKeyPath: '~/.ssh/id',
        domain: '',
      }),
      password: null,
      passphrase: null,
    });
    expect(closed).toBe(true);
    expect(store.selection).toEqual(['c:new-id']);
  });

  it('shows Rust validation errors inline and stays open', async () => {
    let closed = false;
    renderView(
      ConnectionEditor,
      { request: { mode: 'edit', id: 'c1' }, onclose: () => (closed = true) },
      {
        replies: {
          save_connection: () => {
            throw {
              code: 'invalidPort',
              message: 'The port must be a number from 1 to 65535.',
              detail: null,
            };
          },
        },
      },
    );
    expect(text()).not.toContain('Type');
    type(input('Port'), '99999');
    click(buttonByText('Save'));
    await settle();
    expect(closed).toBe(false);
    expect(document.querySelector('[role="alert"]')?.textContent).toBe(
      'The port must be a number from 1 to 65535.',
    );
    expect(input('Port').getAttribute('aria-invalid')).toBe('true');
  });

  it('clears a stored password explicitly', async () => {
    const library: LibraryDto = sidebarLibrary();
    library.connections[0] = { ...library.connections[0]!, hasPassword: true };
    const { bridge } = renderView(
      ConnectionEditor,
      { request: { mode: 'edit', id: 'c1' }, onclose: () => {} },
      { library, replies: { save_connection: 'c1' } },
    );
    expect(input('Password').placeholder).toBe('unchanged');
    click(buttonByText('Clear'));
    expect(text()).toContain('Removed on save');
    click(buttonByText('Save'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith(
      'save_connection',
      expect.objectContaining({ id: 'c1', password: '', passphrase: null }),
    );
  });

  it('shows the linked credential and the no-username note', () => {
    const library = sidebarLibrary();
    library.credentials[1] = { ...library.credentials[1]!, username: '', displayUser: '' };
    renderView(ConnectionEditor, { request: { mode: 'edit', id: 'c2' }, onclose: () => {} }, { library });
    choose('Credential', 'k1');
    expect(text()).toContain('admin');
    expect(text()).not.toContain('Browse…');
    choose('Credential', 'k2');
    expect(text()).toContain('The credential has no username – this connection uses its own.');
    expect(input('Domain')).toBeTruthy();
  });

  it('labels the system RDP client per platform', () => {
    renderView(
      ConnectionEditor,
      { request: { mode: 'new', type: 'rdp', folderId: null }, onclose: () => {} },
      { platform: 'windows' },
    );
    expect(text()).toContain('System RDP client');
    expect(text()).toContain('Admin session (/admin)');
  });
});

describe('ConnectionEditor browser picker', () => {
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
    const { bridge } = renderView(
      ConnectionEditor,
      { request: { mode: 'edit', id: 'c5' }, onclose: () => {} },
      { library, replies: { installed_browsers: installed } },
    );
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
      ConnectionEditor,
      { request: { mode: 'new', type: 'web', folderId: null }, onclose: () => {} },
      { replies: { installed_browsers: installed, save_connection: 'new-id' } },
    );
    await settle();
    type(input('Name'), 'Portal');
    type(input('URL'), 'portal.example.test');
    choose('Browser', 'org.mozilla.firefox');
    click(buttonByText('Create'));
    await settle();
    const call = bridge.invoke.mock.calls.find(([command]) => command === 'save_connection');
    expect((call?.[1] as { draft: { browserBundleId: string } }).draft.browserBundleId).toBe(
      'org.mozilla.firefox',
    );
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
      ConnectionEditor,
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
    click(buttonByText('Generate…'));
    await settle();
    expect(alerts.current).toBe("Couldn't create the SSH key: disk full");
    expect(input('SSH key').value).toBe('');
    expect(buttonByText('Generate…').disabled).toBe(false);
  });
});
