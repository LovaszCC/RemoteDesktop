import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { createApi } from '../api';
import { setLocale } from '../i18n/index.svelte';
import { createAlertStore } from '../stores/alert.svelte';
import { createToastStore } from '../stores/toast.svelte';
import { fakeBridge } from '../testing/bridge';
import { buttonByText, check, click, settle, text } from '../testing/views';
import type { CsvPreviewDto, TransferRowDto } from '../types';
import CsvImportSheet from './CsvImportSheet.svelte';
import { createCsvImportStore, csvSkippedLabel } from './csvImporter.svelte';

function row(id: string, overrides: Partial<TransferRowDto> = {}): TransferRowDto {
  return {
    id,
    type: 'entry',
    name: `server ${id}`,
    host: `${id}.example.test`,
    user: '',
    folderPath: ['Clients', 'Acme'],
    connectionTypes: ['rdp', 'ssh'],
    credentialIds: [],
    hasPassword: false,
    conflict: false,
    ...overrides,
  };
}

function preview(overrides: Partial<CsvPreviewDto> = {}): CsvPreviewDto {
  return {
    fileName: 'servers.csv',
    rows: [row('e0'), row('e1', { conflict: true })],
    skipped: [{ line: 4, name: 'box', reason: 'unknownType', detail: 'vnc' }],
    ...overrides,
  };
}

function setup(replies: Parameters<typeof fakeBridge>[0] = {}) {
  const bridge = fakeBridge({
    save_csv_sample: true,
    pick_csv_file: '/tmp/servers.csv',
    csv_preview: preview(),
    csv_commit: {
      entriesCreated: 1,
      entriesUpdated: 1,
      credentialsCreated: 0,
      credentialsUpdated: 0,
      foldersCreated: 2,
    },
    ...replies,
  });
  const toast = createToastStore();
  const alerts = createAlertStore();
  const store = createCsvImportStore({ api: createApi(bridge.invoke), toast, alerts });
  return { bridge, toast, alerts, store };
}

let mounted: ReturnType<typeof mount> | undefined;

afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
  setLocale('en');
});

function render(replies: Parameters<typeof fakeBridge>[0] = {}) {
  const env = setup(replies);
  mounted = mount(CsvImportSheet, { target: document.body, props: { importer: env.store } });
  env.store.start();
  flushSync();
  return env;
}

describe('CSV import', () => {
  it('labels skipped rows', () => {
    expect(csvSkippedLabel({ line: 4, name: 'box', reason: 'unknownType', detail: 'vnc' })).toBe(
      'Line 4: box (unknown type: vnc)',
    );
    expect(csvSkippedLabel({ line: 2, name: '', reason: 'noName' })).toBe('Line 2: no name');
    expect(csvSkippedLabel({ line: 3, name: 'a', reason: 'noHost' })).toBe('Line 3: a (no host)');
    expect(csvSkippedLabel({ line: 5, name: 'w', reason: 'badUrl', detail: 'x y' })).toBe(
      'Line 5: w (invalid URL: x y)',
    );
    setLocale('hu');
    expect(csvSkippedLabel({ line: 2, name: '', reason: 'noName' })).toBe('2. sor: nincs név');
  });

  it('explains the columns and saves the sample', async () => {
    const { bridge, toast } = render();
    expect(text()).toContain('Import from CSV');
    for (const column of ['folder', 'name', 'host', 'type']) {
      expect([...document.querySelectorAll('dt')].map((d) => d.textContent)).toContain(column);
    }
    click(buttonByText('Download sample…'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('save_csv_sample', undefined);
    expect(toast.current?.message).toBe('Sample saved');
  });

  it('a cancelled sample panel says nothing; Cancel closes', async () => {
    const { toast, store } = render({ save_csv_sample: false });
    await store.saveSample();
    expect(toast.current).toBeNull();
    click(buttonByText('Cancel'));
    expect(store.open).toBe(false);
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });

  it('previews the chosen file, asks about existing entries and imports', async () => {
    const { bridge, toast, store } = render();
    click(buttonByText('Choose file…'));
    await settle();
    expect(bridge.invoke).toHaveBeenLastCalledWith('csv_preview', { path: '/tmp/servers.csv' });
    expect(text()).toContain('servers.csv');
    expect(text()).toContain('Entries: 2, already in the library: 1');
    expect(text()).toContain('Skipped (1): Line 4: box (unknown type: vnc)');
    expect(text()).toContain('Clients / Acme');
    // No user / password columns for a CSV.
    expect(text()).not.toContain('Password');
    expect(buttonByText('Import 2').disabled).toBe(true);

    const select = document.querySelector<HTMLSelectElement>('[data-id="e1"] select')!;
    select.value = 'overwrite';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    flushSync();
    click(buttonByText('Import 2'));
    await settle();
    expect(bridge.invoke).toHaveBeenLastCalledWith('csv_commit', {
      path: '/tmp/servers.csv',
      decisions: [
        { id: 'e0', action: 'new' },
        { id: 'e1', action: 'overwrite' },
      ],
    });
    expect(store.open).toBe(false);
    expect(toast.current?.message).toBe(
      'Imported — new entries: 1, replaced: 1, new credentials: 0, replaced: 0',
    );
  });

  it('unchecking, select none / all and choosing another file', async () => {
    const { bridge, store } = render();
    await store.choose();
    flushSync();
    check(document.querySelector<HTMLInputElement>('[data-id="e1"] input')!, false);
    expect(buttonByText('Import 1').disabled).toBe(false);
    click(buttonByText('Select none'));
    expect(buttonByText('Import 0').disabled).toBe(true);
    click(buttonByText('Select all'));
    expect(store.unanswered).toBe(1);
    bridge.replies.csv_preview = preview({ fileName: 'other.csv', rows: [row('e0')], skipped: [] });
    click(buttonByText('Choose another file…'));
    await settle();
    expect(text()).toContain('other.csv');
    expect(text()).not.toContain('Skipped');
  });

  it('shows errors, keeps the state when the panel is cancelled, refuses incomplete commits', async () => {
    const failing = setup({
      csv_preview: () => {
        throw { code: 'csvMissingColumns', message: 'x', detail: 'folder, type' };
      },
    });
    failing.store.start();
    await failing.store.choose();
    expect(failing.alerts.current).toBe('The CSV file is missing columns: folder, type');
    expect(failing.store.draft).toBeNull();

    const { bridge, store } = setup({ pick_csv_file: null });
    store.start();
    await store.choose();
    expect(store.draft).toBeNull();
    await store.commit();
    expect(bridge.invoke).not.toHaveBeenCalledWith('csv_commit', expect.anything());

    const unanswered = setup();
    unanswered.store.start();
    await unanswered.store.choose();
    await unanswered.store.commit();
    expect(unanswered.bridge.invoke).not.toHaveBeenCalledWith('csv_commit', expect.anything());
    unanswered.store.close();
    unanswered.store.setIncluded(['e0'], true);
    unanswered.store.setChoice('e1', 'new');
    expect(unanswered.store.includedCount).toBe(0);
    expect(unanswered.store.unanswered).toBe(0);
  });
});
