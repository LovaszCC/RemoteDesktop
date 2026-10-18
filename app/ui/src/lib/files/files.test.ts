import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { fakeBridge } from '../testing/bridge';
import { buttonByText, click } from '../testing/views';
import type { FilesStatusDto, ListingDto } from '../types';
import { createFilesStore } from './files.svelte';
import FilesView from './FilesView.svelte';
import type { OsDrop } from './model';

const ID = 'files-c1';

function status(overrides: Partial<FilesStatusDto> = {}): FilesStatusDto {
  return {
    browserId: ID,
    connectionId: 'c1',
    connectionName: 'box',
    startPath: '/var/log',
    transferring: false,
    state: 'ready',
    mode: 'tar',
    ...overrides,
  } as FilesStatusDto;
}

function listing(path: string, names: string[], parent: string | null = '/'): ListingDto {
  return {
    path,
    parent,
    entries: names.map((name) => ({
      name,
      kind: name.endsWith('/') ? 'directory' : 'file',
      size: 10,
      modified: 1_760_000_000,
      permissions: '-rw-r--r--',
    })),
  };
}

function setup(info: FilesStatusDto | Error = status()) {
  const bridge = fakeBridge({
    files_info: () => {
      if (info instanceof Error)
        throw { code: 'browserNotFound', message: 'The file browser is closed.', detail: ID };
      return info;
    },
    files_list_remote: (args?: Record<string, unknown>) => listing(String(args?.path), ['app.log', 'sub/']),
    files_list_local: (args?: Record<string, unknown>) =>
      listing((args?.path as string | null) ?? '/Users/me/Downloads', ['local.txt']),
    files_transfer: 'T1',
    files_cancel: undefined,
    files_conflict_answer: undefined,
    files_reconnect: undefined,
    host_key_answer: undefined,
  });
  const store = createFilesStore(ID, bridge);
  return { bridge, store };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

// jsdom has no hit testing; the drag tests stub it.
document.elementFromPoint ??= () => null;

describe('files store', () => {
  it('loads both panes, starting the remote one in the start path', async () => {
    const { bridge, store } = setup();
    await store.start();
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('files_list_remote', {
      browserId: ID,
      path: '/var/log',
      showHidden: false,
    });
    expect(bridge.invoke).toHaveBeenCalledWith('files_list_local', { path: null, showHidden: false });
    expect(store.remote.listing?.path).toBe('/var/log');
    expect(store.local.listing?.path).toBe('/Users/me/Downloads');
    expect(store.rows('remote').map((e) => e.name)).toEqual(['sub/', 'app.log']);

    await store.setShowHidden(true);
    expect(bridge.invoke).toHaveBeenLastCalledWith('files_list_local', {
      path: '/Users/me/Downloads',
      showHidden: true,
    });
    await store.up('remote');
    expect(store.remote.listing?.path).toBe('/');
    await store.open('local', store.local.listing!.entries[0]!);
    expect(store.local.listing?.path).toBe('/Users/me/Downloads');
    store.stop();
  });

  it('connects later, ignores other browsers and retries a failure', async () => {
    const { bridge, store } = setup(status({ state: 'connecting' } as Partial<FilesStatusDto>));
    await store.start();
    expect(store.ready).toBe(false);
    bridge.emit('files-status', status({ browserId: 'files-other' }));
    expect(store.ready).toBe(false);
    bridge.emit(
      'files-status',
      status({ state: 'failed', code: 'authFailed', error: 'x' } as Partial<FilesStatusDto>),
    );
    expect(store.status?.state).toBe('failed');
    await store.reconnect();
    expect(bridge.invoke).toHaveBeenCalledWith('files_reconnect', { browserId: ID });
    bridge.emit('files-status', status());
    await settle();
    expect(store.remote.listing?.path).toBe('/var/log');

    bridge.emit('files-host-key-prompt', {
      browserId: 'files-other',
      requestId: 'r0',
      host: 'h',
      port: 22,
      keyType: 'k',
      fingerprint: 'f',
    });
    bridge.emit('files-host-key-prompt', {
      browserId: ID,
      requestId: 'r1',
      host: 'h',
      port: 22,
      keyType: 'k',
      fingerprint: 'f',
    });
    expect(store.hostKeys.queued).toBe(1);
    await store.hostKeys.answer(true);
    expect(bridge.invoke).toHaveBeenCalledWith('host_key_answer', { requestId: 'r1', accept: true });
  });

  it('runs one transfer with progress, a conflict and the result', async () => {
    const { bridge, store } = setup();
    await store.start();
    await settle();
    await store.copy('remote', ['app.log']);
    expect(bridge.invoke).toHaveBeenCalledWith('files_transfer', {
      browserId: ID,
      direction: 'download',
      sources: ['/var/log/app.log'],
      destination: '/Users/me/Downloads',
      gzip: true,
    });
    expect(store.transferring).toBe(true);
    // One at a time: a second start is ignored while running.
    await store.copy('local', ['local.txt']);
    expect(bridge.invoke.mock.calls.filter(([c]) => c === 'files_transfer')).toHaveLength(1);

    bridge.emit('files-progress', {
      browserId: ID,
      transferId: 'T1',
      direction: 'download',
      index: 0,
      count: 1,
      name: 'app.log',
      bytesDone: 5,
      bytesTotal: 10,
    });
    expect(store.progress?.bytesDone).toBe(5);
    bridge.emit('files-conflict', {
      browserId: ID,
      requestId: 'q1',
      name: 'app.log',
      isFolder: false,
      direction: 'download',
    });
    expect(store.conflict?.name).toBe('app.log');
    await store.answerConflict('replace', true);
    expect(bridge.invoke).toHaveBeenCalledWith('files_conflict_answer', {
      requestId: 'q1',
      choice: 'replace',
      applyToAll: true,
    });
    expect(store.conflict).toBeNull();

    bridge.emit('files-done', {
      browserId: ID,
      transferId: 'T1',
      direction: 'download',
      items: [{ name: 'app.log', status: 'done', error: null }],
      cancelled: false,
    });
    expect(store.transferring).toBe(false);
    expect(store.lastDone?.items).toHaveLength(1);
    await settle();
    expect(bridge.invoke).toHaveBeenLastCalledWith('files_list_local', {
      path: '/Users/me/Downloads',
      showHidden: false,
    });
    store.dismissResult();
    expect(store.lastDone).toBeNull();

    store.gzip = false;
    await store.uploadPaths(['/tmp/a/1', '/tmp/b/2', '/tmp/a/3']);
    expect(bridge.invoke).toHaveBeenLastCalledWith('files_transfer', {
      browserId: ID,
      direction: 'upload',
      sources: ['/tmp/a/1', '/tmp/a/3', '/tmp/b/2'],
      destination: '/var/log',
      gzip: false,
    });
    await store.cancel();
    expect(bridge.invoke).toHaveBeenCalledWith('files_cancel', { browserId: ID });
  });

  it('shows a refused transfer and a closed browser', async () => {
    const { bridge, store } = setup();
    await store.start();
    await settle();
    bridge.replies.files_transfer = () => {
      throw { code: 'transferRunning', message: 'Wait for the running transfer to finish.', detail: null };
    };
    await store.copy('local', ['local.txt']);
    expect(store.actionError).toBe('Wait for the running transfer to finish.');
    expect(store.transferring).toBe(false);

    const closed = setup(new Error('gone'));
    await closed.store.start();
    expect(closed.store.statusError).toBe('The file browser is closed.');
  });
});

let mounted: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
});

describe('FilesView', () => {
  it('shows connecting, failure with Retry, then both panes', async () => {
    const { bridge, store } = setup(status({ state: 'connecting' } as Partial<FilesStatusDto>));
    mounted = mount(FilesView, { target: document.body, props: { store } });
    await store.start();
    flushSync();
    expect(document.body.textContent).toContain('Connecting…');
    bridge.emit(
      'files-status',
      status({
        state: 'failed',
        code: 'authFailed',
        error: 'Permission denied (password).',
      } as Partial<FilesStatusDto>),
    );
    flushSync();
    expect(document.body.textContent).toContain('Authentication failed');
    expect(document.body.textContent).toContain('Permission denied (password).');
    click(buttonByText('Retry'));
    expect(bridge.invoke).toHaveBeenCalledWith('files_reconnect', { browserId: ID });

    bridge.emit('files-status', status({ mode: 'sftp' } as Partial<FilesStatusDto>));
    await settle();
    flushSync();
    expect(document.body.textContent).toContain('Only single files can be copied');
    const rows = document.querySelectorAll('[data-pane="remote"] .body .row');
    expect([...rows].map((r) => r.getAttribute('data-name'))).toEqual(['sub/', 'app.log']);
  });

  it('selects rows, copies with the arrow buttons and answers conflicts', async () => {
    const { bridge, store } = setup();
    mounted = mount(FilesView, { target: document.body, props: { store } });
    await store.start();
    await settle();
    flushSync();
    const download = document.querySelector<HTMLButtonElement>(
      '[aria-label="Download the selected items into the local folder"]',
    )!;
    expect(download.disabled).toBe(true);
    click(document.querySelector('[data-pane="remote"] [data-name="app.log"]')!);
    flushSync();
    expect(store.remote.selection).toEqual(['app.log']);
    expect(download.disabled).toBe(false);
    click(download);
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith(
      'files_transfer',
      expect.objectContaining({ direction: 'download' }),
    );

    bridge.emit('files-progress', {
      browserId: ID,
      transferId: 'T1',
      direction: 'download',
      index: 0,
      count: 2,
      name: 'app.log',
      bytesDone: 5,
      bytesTotal: 10,
    });
    flushSync();
    expect(document.body.textContent).toContain('Downloading “app.log” (1 of 2)');
    expect(document.querySelector('[role="progressbar"]')?.getAttribute('aria-valuenow')).toBe('50');

    bridge.emit('files-conflict', {
      browserId: ID,
      requestId: 'q1',
      name: 'app.log',
      isFolder: true,
      direction: 'download',
    });
    flushSync();
    expect(document.body.textContent).toContain('“app.log” already exists');
    expect(document.body.textContent).toContain('Replacing a folder merges it');
    click(buttonByText('Replace'));
    expect(bridge.invoke).toHaveBeenCalledWith('files_conflict_answer', {
      requestId: 'q1',
      choice: 'replace',
      applyToAll: false,
    });
    click(buttonByText('Cancel'));
    expect(bridge.invoke).toHaveBeenCalledWith('files_cancel', { browserId: ID });

    bridge.emit('files-done', {
      browserId: ID,
      transferId: 'T1',
      direction: 'download',
      items: [{ name: 'app.log', status: 'failed', error: 'tar: app.log: Permission denied' }],
      cancelled: false,
    });
    flushSync();
    expect(document.body.textContent).toContain('1 failed');
    expect(document.body.textContent).toContain('app.log: tar: app.log: Permission denied');
  });

  it('uploads files dropped from the OS onto the remote pane', async () => {
    const { bridge, store } = setup();
    let deliver: ((event: OsDrop) => void) | undefined;
    const subscribeDrop = vi.fn(async (handler: (event: OsDrop) => void) => {
      deliver = handler;
      return () => {};
    });
    mounted = mount(FilesView, { target: document.body, props: { store, subscribeDrop } });
    await store.start();
    await settle();
    flushSync();
    const remotePane = document.querySelector('[data-pane="remote"]')!;
    const localPane = document.querySelector('[data-pane="local"]')!;
    const at = vi.spyOn(document, 'elementFromPoint');
    at.mockReturnValue(remotePane);
    deliver!({ type: 'over', x: 10, y: 10 });
    flushSync();
    expect(remotePane.classList.contains('highlight')).toBe(true);
    deliver!({ type: 'drop', x: 10, y: 10, paths: ['/Users/me/a.txt'] });
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('files_transfer', {
      browserId: ID,
      direction: 'upload',
      sources: ['/Users/me/a.txt'],
      destination: '/var/log',
      gzip: true,
    });
    at.mockReturnValue(localPane);
    deliver!({ type: 'leave' });
    flushSync();
    expect(remotePane.classList.contains('highlight')).toBe(false);
    at.mockRestore();
  });

  it('drags rows to the other pane with the pointer', async () => {
    const { bridge, store } = setup();
    mounted = mount(FilesView, { target: document.body, props: { store } });
    await store.start();
    await settle();
    flushSync();
    const row = document.querySelector('[data-pane="local"] [data-name="local.txt"]')!;
    const remotePane = document.querySelector('[data-pane="remote"]')!;
    const at = vi.spyOn(document, 'elementFromPoint').mockReturnValue(remotePane);
    const pointer = (type: string, x: number) =>
      new PointerEvent(type, { bubbles: true, button: 0, clientX: x, clientY: 5 } as PointerEventInit);
    row.dispatchEvent(pointer('pointerdown', 0));
    window.dispatchEvent(pointer('pointermove', 40));
    flushSync();
    expect(document.querySelector('.ghost')?.textContent).toContain('local.txt');
    window.dispatchEvent(pointer('pointerup', 40));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('files_transfer', {
      browserId: ID,
      direction: 'upload',
      sources: ['/Users/me/Downloads/local.txt'],
      destination: '/var/log',
      gzip: true,
    });
    at.mockRestore();
  });
});
