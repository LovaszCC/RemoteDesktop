import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApi } from '../api';
import { setLocale } from '../i18n/index.svelte';
import { createAlertStore } from '../stores/alert.svelte';
import { createToastStore } from '../stores/toast.svelte';
import { fakeBridge } from '../testing/bridge';
import { buttonByText, check, click, settle, text } from '../testing/views';
import type { ImportKind, ImportPreviewDto, ImportRowDto } from '../types';
import { createImportStore, resultMessage, skippedLabel } from './importer.svelte';
import ImportSheet from './ImportSheet.svelte';
import { allIncluded, initialIncluded, withIncluded } from './model';

function row(id: string, overrides: Partial<ImportRowDto> = {}): ImportRowDto {
  return {
    id,
    type: 'mikrotik',
    name: `router ${id}`,
    host: '192.0.2.1',
    port: null,
    user: 'admin',
    groupPath: ['Branch East'],
    hasPassword: true,
    duplicate: false,
    ...overrides,
  };
}

function preview(overrides: Partial<ImportPreviewDto> = {}): ImportPreviewDto {
  return {
    kind: 'winbox',
    fileName: 'book.cdb',
    rows: [row('c0'), row('c1', { duplicate: true, hasPassword: false }), row('c2', { port: 8728 })],
    skipped: [],
    withoutPassword: 0,
    emptyGroups: 0,
    ...overrides,
  };
}

function setup(replies: Parameters<typeof fakeBridge>[0] = {}) {
  const bridge = fakeBridge({
    pick_import_file: '/tmp/book.cdb',
    import_preview: preview(),
    import_commit: { connections: 2, credentials: 0, foldersCreated: 1, skipped: 0, withoutPassword: 0 },
    ...replies,
  });
  const toast = createToastStore();
  const alerts = createAlertStore();
  const store = createImportStore({ api: createApi(bridge.invoke), toast, alerts });
  return { bridge, toast, alerts, store };
}

let mounted: ReturnType<typeof mount> | undefined;

afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
  setLocale('en');
});

describe('import model', () => {
  it('checks new rows, toggles and selects all or none', () => {
    const rows = preview().rows;
    const start = initialIncluded(rows);
    expect([...start]).toEqual(['c0', 'c2']);
    expect([...withIncluded(start, 'c1', true)]).toEqual(['c0', 'c2', 'c1']);
    expect([...withIncluded(start, 'c0', false)]).toEqual(['c2']);
    expect([...start]).toEqual(['c0', 'c2']);
    expect(allIncluded(rows, true).size).toBe(3);
    expect(allIncluded(rows, false).size).toBe(0);
  });

  it('labels skipped entries and results', () => {
    expect(skippedLabel({ name: 'portal', reason: 'unsupported', kind: 'WebBrowser' })).toBe(
      'portal (WebBrowser)',
    );
    expect(skippedLabel({ name: '', reason: 'unsupported', kind: 'VNC' })).toBe('VNC');
    expect(skippedLabel({ name: 'box', reason: 'noHost' })).toBe('box (no host)');
    const result = { connections: 3, credentials: 1, foldersCreated: 2, skipped: 0, withoutPassword: 0 };
    expect(resultMessage('winbox', result)).toBe('Imported from WinBox — connections: 3, new folders: 2');
    expect(resultMessage('rdm', result)).toBe('Imported — connections: 3, credentials: 1, new folders: 2.');
    setLocale('hu');
    expect(skippedLabel({ name: 'box', reason: 'noHost' })).toBe('box (nincs host)');
  });
});

describe('import store', () => {
  it('previews the picked file with duplicates unchecked, then imports the checked rows', async () => {
    const { bridge, toast, store } = setup();
    await store.start('winbox');
    expect(bridge.invoke).toHaveBeenCalledWith('pick_import_file', { kind: 'winbox' });
    expect(bridge.invoke).toHaveBeenCalledWith('import_preview', { kind: 'winbox', path: '/tmp/book.cdb' });
    expect(store.includedCount).toBe(2);
    expect(store.isIncluded('c1')).toBe(false);

    store.setIncluded('c2', false);
    store.setIncluded('c1', true);
    await store.commit();
    expect(bridge.invoke).toHaveBeenLastCalledWith('import_commit', {
      kind: 'winbox',
      path: '/tmp/book.cdb',
      selected: ['c0', 'c1'],
    });
    expect(store.draft).toBeNull();
    expect(toast.current?.message).toBe('Imported from WinBox — connections: 2, new folders: 1');
  });

  it('does nothing when the panel is cancelled', async () => {
    const { bridge, store } = setup({ pick_import_file: null });
    await store.start('rdm');
    expect(store.draft).toBeNull();
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
  });

  it('shows errors and keeps the sheet open when the commit fails', async () => {
    const unreadable = {
      code: 'importUnreadable',
      message: "This isn't a WinBox address book (.cdb) or it is damaged.",
      detail: null,
    };
    const failing = setup({
      import_preview: () => {
        throw unreadable;
      },
    });
    await failing.store.start('winbox');
    expect(failing.store.draft).toBeNull();
    expect(failing.alerts.current).toBe("This isn't a WinBox address book (.cdb) or it is damaged.");

    const { store, alerts } = setup({
      import_commit: () => {
        throw { code: 'readOnly', message: 'x', detail: null };
      },
    });
    await store.start('winbox');
    await store.commit();
    expect(store.draft).not.toBeNull();
    expect(alerts.current).toBe('The database is open read-only because another computer is using it.');
  });

  it('refuses to commit nothing and ignores a second start while busy', async () => {
    const { bridge, store } = setup();
    await store.commit();
    expect(bridge.invoke).not.toHaveBeenCalled();
    const first = store.start('winbox');
    void store.start('rdm');
    await first;
    expect(bridge.invoke.mock.calls.filter(([c]) => c === 'pick_import_file')).toHaveLength(1);
    store.setAll(false);
    await store.commit();
    expect(bridge.invoke).not.toHaveBeenCalledWith('import_commit', expect.anything());
    store.close();
    store.setIncluded('c0', true);
    store.setAll(true);
    expect(store.includedCount).toBe(0);
  });
});

describe('ImportSheet', () => {
  async function render(replies: Parameters<typeof fakeBridge>[0] = {}, kind: ImportKind = 'winbox') {
    const env = setup(replies);
    mounted = mount(ImportSheet, { target: document.body, props: { importer: env.store } });
    await env.store.start(kind);
    flushSync();
    return env;
  }

  const boxes = () => [...document.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')];

  it('lists the rows with duplicates unchecked and badged', async () => {
    await render();
    expect(text()).toContain('Import from WinBox');
    expect(text()).toContain('book.cdb');
    expect(text()).toContain('Entries: 3, already in the library: 1');
    expect(boxes().map((b) => b.checked)).toEqual([true, false, true]);
    expect(document.querySelectorAll('.capsule')).toHaveLength(1);
    expect(text()).toContain('192.0.2.1:8728');
    expect(text()).toContain('Branch East');
    expect(buttonByText('Import 2').disabled).toBe(false);
  });

  it('selects none and all, then imports', async () => {
    const { bridge, toast } = await render();
    click(buttonByText('Select none'));
    expect(buttonByText('Import 0').disabled).toBe(true);
    click(buttonByText('Select all'));
    expect(boxes().every((b) => b.checked)).toBe(true);
    check(boxes()[0]!, false);
    click(buttonByText('Import 2'));
    await settle();
    expect(bridge.invoke).toHaveBeenLastCalledWith('import_commit', {
      kind: 'winbox',
      path: '/tmp/book.cdb',
      selected: ['c1', 'c2'],
    });
    expect(document.querySelector('[role="dialog"]')).toBeNull();
    expect(toast.current).not.toBeNull();
  });

  it('shows the RDM title, credentials, notes and skipped entries; Cancel closes', async () => {
    const { store } = await render(
      {
        import_preview: preview({
          kind: 'rdm',
          rows: [row('c0', { type: 'rdp' }), row('k0', { type: 'credential', host: '', groupPath: [] })],
          skipped: [
            { name: 'portal', reason: 'unsupported', kind: 'WebBrowser' },
            { name: 'box', reason: 'noHost' },
          ],
          withoutPassword: 2,
        }),
      },
      'rdm',
    );
    expect(text()).toContain('Import from RDM');
    expect(text()).toContain('2 entries had an encrypted password');
    expect(text()).toContain('Skipped (2): portal (WebBrowser), box (no host)');
    expect(document.querySelector('[data-id="k0"] .credential-icon')).not.toBeNull();
    click(buttonByText('Cancel'));
    expect(store.draft).toBeNull();
    expect(document.querySelector('[role="dialog"]')).toBeNull();
  });

  it('closes on Escape', async () => {
    const { store } = await render();
    const dialog = document.querySelector('[role="dialog"]')!;
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    flushSync();
    expect(store.draft).toBeNull();
    vi.restoreAllMocks();
  });
});
