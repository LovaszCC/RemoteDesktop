import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { createApi } from '../api';
import { setLocale } from '../i18n/index.svelte';
import { createAlertStore } from '../stores/alert.svelte';
import { createToastStore } from '../stores/toast.svelte';
import { fakeBridge } from '../testing/bridge';
import { connection, entry, libraryOf } from '../testing/fixtures';
import { buttonByText, check, click, settle, text, type } from '../testing/views';
import type { CredentialDto, LibraryDto, TransferPreviewDto, TransferRowDto } from '../types';
import ExportSheet from './ExportSheet.svelte';
import { createExportStore, exportMessage } from './exporter.svelte';
import {
  decisions,
  exportGroups,
  exportProblem,
  groupState,
  initialImportIncluded,
  initialSelection,
  missingCredentials,
  unanswered,
  withAll,
} from './model';
import TransferImportSheet from './TransferImportSheet.svelte';
import { createTransferImportStore, transferMessage } from './transferImporter.svelte';

function credential(id: string, name: string): CredentialDto {
  return {
    id,
    name,
    username: 'admin',
    domain: '',
    displayUser: 'admin',
    hasPassword: true,
    sshKeyPath: null,
    hasPassphrase: false,
    hasUnlockedKey: false,
    notes: '',
    usageCount: 1,
    createdAt: '2026-01-01 00:00:00.000',
    updatedAt: '2026-01-01 00:00:00.000',
  };
}

/** `Acme` (with `Acme/Sub`) holding dc01 (linked to k1) and app01, sub01 in the subfolder, web at the root. */
function library(): LibraryDto {
  const base = libraryOf([]);
  const inFolder = (
    id: string,
    folderId: string,
    folderPath: string,
    credentialId: string | null = null,
  ) => ({
    ...connection(id),
    name: id,
    folderId,
    folderPath,
    credentialId,
  });
  const connections = [
    inFolder('dc01', 'F1', 'Acme', 'k1'),
    inFolder('app01', 'F1', 'Acme'),
    inFolder('sub01', 'F2', 'Acme / Sub'),
    { ...connection('web'), type: 'web' as const },
  ];
  return {
    ...base,
    folders: [
      { id: 'F1', parentId: null, name: 'Acme', sortOrder: 0, color: null, path: 'Acme' },
      { id: 'F2', parentId: 'F1', name: 'Sub', sortOrder: 0, color: null, path: 'Acme / Sub' },
    ],
    connections,
    entries: connections.map((c) => entry(c.id, [c])),
    credentials: [credential('k1', 'Domain admin'), credential('k2', 'Spare')],
  };
}

function row(id: string, overrides: Partial<TransferRowDto> = {}): TransferRowDto {
  return {
    id,
    type: id.startsWith('k') ? 'credential' : 'entry',
    name: `item ${id}`,
    host: id.startsWith('k') ? '' : `${id}.example.test`,
    user: 'admin',
    folderPath: [],
    connectionTypes: id.startsWith('k') ? [] : ['rdp'],
    credentialIds: [],
    hasPassword: true,
    conflict: false,
    ...overrides,
  };
}

function preview(overrides: Partial<TransferPreviewDto> = {}): TransferPreviewDto {
  return {
    fileName: 'share.rdexport',
    encrypted: true,
    exportedAt: '2026-10-20 10:00:00.000',
    rows: [row('e0'), row('e1', { conflict: true }), row('k0', { conflict: true })],
    ...overrides,
  };
}

let mounted: ReturnType<typeof mount> | undefined;

afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
  setLocale('en');
});

describe('transfer model', () => {
  it('groups entries by folder path, the root first', () => {
    const groups = exportGroups(library());
    expect(groups.map((g) => g.path)).toEqual(['', 'Acme', 'Acme / Sub']);
    expect(groups[1]!.entries.map((e) => e.id)).toEqual(['app01', 'dc01']);
  });

  it('toggles sets and reports group states', () => {
    const set = withAll(new Set(['a']), ['b', 'c'], true);
    expect([...set]).toEqual(['a', 'b', 'c']);
    expect([...withAll(set, ['a', 'c'], false)]).toEqual(['b']);
    expect(groupState(set, ['a', 'b'])).toBe('all');
    expect(groupState(set, ['a', 'x'])).toBe('some');
    expect(groupState(set, ['x'])).toBe('none');
  });

  it('starts with the records behind the sidebar selection', () => {
    const lib = library();
    const folder = initialSelection(lib, ['f:F1', 'k:k2', 'x:unknown']);
    expect([...folder.entries].sort()).toEqual(['app01', 'dc01', 'sub01']);
    expect([...folder.credentials]).toEqual(['k2']);
    const rows = initialSelection(lib, ['e:web', 'fav:dc01', 'c:sub01', 's:split']);
    expect([...rows.entries]).toEqual(['web', 'dc01', 'sub01']);
  });

  it('finds credentials left behind and export problems', () => {
    const lib = library();
    expect(missingCredentials(lib, new Set(['dc01', 'web']), new Set())).toEqual(['k1']);
    expect(missingCredentials(lib, new Set(['dc01']), new Set(['k1']))).toEqual([]);
    expect(exportProblem(0, 0, '', '')).toBe('nothing');
    expect(exportProblem(1, 0, 'a', 'b')).toBe('mismatch');
    expect(exportProblem(0, 1, 'a', 'a')).toBeNull();
  });

  it('turns rows, checks and choices into decisions', () => {
    const rows = preview().rows;
    const included = initialImportIncluded(rows);
    expect(included.size).toBe(3);
    expect(unanswered(rows, included, {})).toBe(2);
    expect(unanswered(rows, included, { e1: 'overwrite', k0: 'new' })).toBe(0);
    expect(unanswered(rows, withAll(included, ['e1', 'k0'], false), {})).toBe(0);
    expect(decisions(rows, withAll(included, ['k0'], false), { e1: 'overwrite' })).toEqual([
      { id: 'e0', action: 'new' },
      { id: 'e1', action: 'overwrite' },
      { id: 'k0', action: 'skip' },
    ]);
    expect(decisions(rows, included, {})[1]).toEqual({ id: 'e1', action: 'skip' });
  });

  it('words the toasts', () => {
    expect(exportMessage({ entries: 3, credentials: 1, encrypted: true })).toBe(
      'Exported with password — entries: 3, credentials: 1',
    );
    expect(exportMessage({ entries: 2, credentials: 0, encrypted: false })).toBe(
      'Exported without passwords — entries: 2, credentials: 0',
    );
    expect(
      transferMessage({
        entriesCreated: 2,
        entriesUpdated: 1,
        credentialsCreated: 0,
        credentialsUpdated: 1,
        foldersCreated: 0,
      }),
    ).toBe('Imported — new entries: 2, replaced: 1, new credentials: 0, replaced: 1');
    setLocale('hu');
    expect(exportMessage({ entries: 1, credentials: 1, encrypted: true })).toBe(
      'Exportálva jelszóval — bejegyzések: 1, credentialök: 1',
    );
  });
});

describe('export', () => {
  function setup(replies: Parameters<typeof fakeBridge>[0] = {}) {
    const bridge = fakeBridge({
      pick_export_path: '/tmp/share.rdexport',
      export_library: { entries: 1, credentials: 1, encrypted: true },
      ...replies,
    });
    const toast = createToastStore();
    const alerts = createAlertStore();
    const store = createExportStore({ api: createApi(bridge.invoke), toast, alerts });
    return { bridge, toast, alerts, store };
  }

  function render(selection: string[] = ['e:dc01'], replies: Parameters<typeof fakeBridge>[0] = {}) {
    const env = setup(replies);
    mounted = mount(ExportSheet, {
      target: document.body,
      props: { exporter: env.store, library: library() },
    });
    env.store.open(library(), selection);
    flushSync();
    return env;
  }

  const entryBox = (id: string) => document.querySelector<HTMLInputElement>(`.entry[data-id="${id}"] input`)!;
  const folderBox = (path: string) =>
    document.querySelector<HTMLInputElement>(`[data-folder="${path}"] input`)!;
  const passwords = () => [...document.querySelectorAll<HTMLInputElement>('input[type="password"]')];

  it('starts with the selection, flags a credential left behind and exports with a password', async () => {
    const { bridge, toast, store } = render();
    expect(text()).toContain('Export');
    expect(entryBox('dc01').checked).toBe(true);
    expect(folderBox('Acme').indeterminate).toBe(true);
    expect(text()).toContain('without credential');
    expect(text()).toContain('Without a password the file is readable');

    check(document.querySelector<HTMLInputElement>('[data-id="k1"] input')!, true);
    expect(text()).not.toContain('without credential');
    check(folderBox('Acme'), true);
    expect(entryBox('app01').checked).toBe(true);

    type(passwords()[0]!, 'pw');
    expect(text()).toContain("The passwords don't match.");
    expect(buttonByText('Export…').disabled).toBe(true);
    type(passwords()[1]!, 'pw');
    expect(text()).toContain('The file is encrypted and contains the passwords.');
    click(buttonByText('Export…'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('pick_export_path', undefined);
    expect(bridge.invoke).toHaveBeenLastCalledWith('export_library', {
      path: '/tmp/share.rdexport',
      entryIds: ['dc01', 'app01'],
      credentialIds: ['k1'],
      password: 'pw',
    });
    expect(store.draft).toBeNull();
    expect(toast.current?.message).toBe('Exported with password — entries: 1, credentials: 1');
  });

  it('exports without a password as null, selects all or none, and keeps the sheet when cancelled', async () => {
    const { bridge, store } = render([], { pick_export_path: null });
    expect(buttonByText('Export…').disabled).toBe(true);
    click(buttonByText('Select all'));
    expect(store.draft?.entries.size).toBe(4);
    expect(store.draft?.credentials.size).toBe(2);
    click(buttonByText('Select none'));
    expect(store.draft?.entries.size).toBe(0);
    check(entryBox('web'), true);
    click(buttonByText('Export…'));
    await settle();
    expect(bridge.invoke).not.toHaveBeenCalledWith('export_library', expect.anything());
    expect(store.draft).not.toBeNull();
    click(buttonByText('Cancel'));
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });

  it('sends no password when it is empty and shows errors', async () => {
    const { bridge, alerts, store } = render(['e:web'], {
      export_library: () => {
        throw { code: 'exportWrite', message: "Can't write the file.", detail: 'denied' };
      },
    });
    await store.commit();
    expect(bridge.invoke).toHaveBeenLastCalledWith('export_library', {
      path: '/tmp/share.rdexport',
      entryIds: ['web'],
      credentialIds: [],
      password: null,
    });
    expect(alerts.current).toBe("Can't write the file. (denied)");
    expect(store.draft).not.toBeNull();
  });
});

describe('transfer import', () => {
  function setup(replies: Parameters<typeof fakeBridge>[0] = {}) {
    const bridge = fakeBridge({
      pick_transfer_file: '/tmp/share.rdexport',
      transfer_preview: preview(),
      transfer_commit: {
        entriesCreated: 1,
        entriesUpdated: 1,
        credentialsCreated: 0,
        credentialsUpdated: 0,
        foldersCreated: 0,
      },
      ...replies,
    });
    const toast = createToastStore();
    const alerts = createAlertStore();
    const store = createTransferImportStore({ api: createApi(bridge.invoke), toast, alerts });
    return { bridge, toast, alerts, store };
  }

  async function render(replies: Parameters<typeof fakeBridge>[0] = {}) {
    const env = setup(replies);
    mounted = mount(TransferImportSheet, { target: document.body, props: { importer: env.store } });
    await env.store.start();
    flushSync();
    return env;
  }

  const choice = (id: string) => document.querySelector<HTMLSelectElement>(`[data-id="${id}"] select`)!;
  function choose(id: string, value: string) {
    const select = choice(id);
    select.value = value;
    select.dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();
  }

  it('asks for the password of an encrypted file, then previews it', async () => {
    let tries = 0;
    const { bridge, store } = await render({
      transfer_preview: (args?: Record<string, unknown>) => {
        tries += 1;
        if (args?.password === 'right') return preview();
        throw {
          code: args?.password ? 'transferWrongPassword' : 'transferNeedsPassword',
          message: 'x',
          detail: null,
        };
      },
    });
    expect(store.passwordStep?.fileName).toBe('share.rdexport');
    expect(text()).toContain('share.rdexport is encrypted');
    const field = document.querySelector<HTMLInputElement>('input[type="password"]')!;
    type(field, 'wrong');
    click(buttonByText('Open'));
    await settle();
    expect(text()).toContain('Wrong password');
    type(field, 'right');
    click(buttonByText('Open'));
    await settle();
    expect(tries).toBe(3);
    expect(bridge.invoke).toHaveBeenLastCalledWith('transfer_preview', {
      path: '/tmp/share.rdexport',
      password: 'right',
    });
    expect(store.passwordStep).toBeNull();
    expect(text()).toContain('Entries: 3, already in the library: 2');
  });

  it('needs a choice for every checked conflict before importing', async () => {
    const { bridge, toast, store } = await render();
    expect(text()).toContain('Some items already exist');
    expect(buttonByText('Import 3').disabled).toBe(true);
    choose('e1', 'overwrite');
    expect(buttonByText('Import 3').disabled).toBe(true);
    check(document.querySelector<HTMLInputElement>('[data-id="k0"] input')!, false);
    expect(choice('k0').disabled).toBe(true);
    expect(buttonByText('Import 2').disabled).toBe(false);
    click(buttonByText('Import 2'));
    await settle();
    expect(bridge.invoke).toHaveBeenLastCalledWith('transfer_commit', {
      path: '/tmp/share.rdexport',
      password: null,
      decisions: [
        { id: 'e0', action: 'new' },
        { id: 'e1', action: 'overwrite' },
        { id: 'k0', action: 'skip' },
      ],
    });
    expect(store.draft).toBeNull();
    expect(toast.current?.message).toBe(
      'Imported — new entries: 1, replaced: 1, new credentials: 0, replaced: 0',
    );
  });

  it('notes a plain export, keeps both, and selects none or all', async () => {
    const { bridge } = await render({ transfer_preview: preview({ encrypted: false }) });
    expect(text()).toContain('This export has no passwords');
    click(buttonByText('Select none'));
    expect(buttonByText('Import 0').disabled).toBe(true);
    click(buttonByText('Select all'));
    choose('e1', 'new');
    choose('k0', 'new');
    click(buttonByText('Import 3'));
    await settle();
    expect(bridge.invoke).toHaveBeenLastCalledWith(
      'transfer_commit',
      expect.objectContaining({
        decisions: [
          { id: 'e0', action: 'new' },
          { id: 'e1', action: 'new' },
          { id: 'k0', action: 'new' },
        ],
      }),
    );
  });

  it('shows other errors, ignores a cancelled panel and closes', async () => {
    const cancelled = setup({ pick_transfer_file: null });
    await cancelled.store.start();
    expect(cancelled.store.draft).toBeNull();
    expect(cancelled.store.passwordStep).toBeNull();

    const broken = setup({
      transfer_preview: () => {
        throw { code: 'transferUnreadable', message: 'x', detail: null };
      },
    });
    await broken.store.start();
    expect(broken.alerts.current).toBe("This isn't a RemoteDesktop export or it is damaged.");
    expect(broken.store.passwordStep).toBeNull();

    const failing = setup({
      transfer_commit: () => {
        throw { code: 'readOnly', message: 'x', detail: null };
      },
      transfer_preview: preview({ rows: [row('e0')] }),
    });
    await failing.store.start();
    await failing.store.commit();
    expect(failing.store.draft).not.toBeNull();
    expect(failing.alerts.current).toBe(
      'The database is open read-only because another computer is using it.',
    );
    await failing.store.unlock('ignored');
    failing.store.close();
    expect(failing.store.draft).toBeNull();
    failing.store.setIncluded(['e0'], true);
    failing.store.setChoice('e0', 'new');
    expect(failing.store.includedCount).toBe(0);
    expect(failing.store.unanswered).toBe(0);
  });
});
