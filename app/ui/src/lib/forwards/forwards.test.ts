import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { closeContextMenu, contextMenu } from '../components/menu.svelte';
import ConnectionDetail from '../detail/ConnectionDetail.svelte';
import { fakeBridge } from '../testing/bridge';
import { sidebarLibrary } from '../testing/fixtures';
import { buttonByText, cleanupView, click, renderView, settle, text, type } from '../testing/views';
import type { ForwardStatusDto, HostKeyPromptDto, LibraryDto, PortForwardDto } from '../types';
import { createForwardStore, isRunning } from './forwards.svelte';
import HostKeyDialog from './HostKeyDialog.svelte';
import { createHostKeyStore } from './hostKeys.svelte';
import { draftOf, failureText } from './model';

const FORWARD: PortForwardDto = {
  id: 'p1',
  connectionId: 'c1',
  name: 'RDP',
  localPort: 13389,
  remoteHost: '192.0.2.20',
  remotePort: 3389,
  createdAt: '',
};

const status = (state: ForwardStatusDto['state'], extra: object = {}): ForwardStatusDto =>
  ({ id: 'p1', connectionId: 'c1', state, ...extra }) as ForwardStatusDto;

function libraryWithForward(): LibraryDto {
  const library = sidebarLibrary();
  library.portForwards = [FORWARD];
  return library;
}

afterEach(() => {
  cleanupView();
  closeContextMenu();
});

describe('forward store', () => {
  it('loads the statuses, follows the events and drops stopped forwards', async () => {
    const bridge = fakeBridge({ port_forward_statuses: [status('active')] });
    const store = createForwardStore(bridge);
    await store.start();
    expect(store.status('p1')?.state).toBe('active');
    expect(store.hasRunning('c1')).toBe(true);
    expect(store.hasRunning('c2')).toBe(false);

    bridge.emit('port-forward-status', status('failed', { code: 'timeout', error: 'late' }));
    expect(store.status('p1')?.state).toBe('failed');
    expect(store.hasRunning('c1')).toBe(false);
    bridge.emit('port-forward-status', status('starting'));
    expect(store.hasRunning('c1')).toBe(true);
    bridge.emit('port-forward-status', status('stopped'));
    expect(store.status('p1')).toBeNull();

    await store.start();
    expect(bridge.listenerCount('port-forward-status')).toBe(1);
    store.stop();
    expect(bridge.listenerCount('port-forward-status')).toBe(0);
  });

  it('saves, optionally starts, stops and deletes through the commands', async () => {
    const bridge = fakeBridge({
      port_forward_save: 'p9',
      port_forward_start: null,
      port_forward_stop: null,
      port_forward_delete: null,
    });
    const store = createForwardStore(bridge);
    const draft = draftOf('c1');
    expect(await store.save(draft, false)).toBe('p9');
    expect(bridge.invoke).toHaveBeenCalledWith('port_forward_save', { forward: draft });
    expect(bridge.invoke).not.toHaveBeenCalledWith('port_forward_start', expect.anything());
    await store.save(draft, true);
    expect(bridge.invoke).toHaveBeenCalledWith('port_forward_start', { id: 'p9' });
    await store.stopForward('p9');
    await store.remove('p9');
    expect(bridge.invoke).toHaveBeenCalledWith('port_forward_stop', { id: 'p9' });
    expect(bridge.invoke).toHaveBeenCalledWith('port_forward_delete', { id: 'p9' });
  });

  it('knows which states run', () => {
    expect(isRunning(status('starting'))).toBe(true);
    expect(isRunning(status('active'))).toBe(true);
    expect(isRunning(status('failed', { code: 'x', error: 'y' }))).toBe(false);
    expect(isRunning(null)).toBe(false);
  });
});

describe('forward model', () => {
  it('translates failure codes and falls back to the English error', () => {
    expect(failureText(FORWARD, status('failed', { code: 'portInUse', error: '' }))).toBe(
      'Local port 13389 is already in use.',
    );
    expect(failureText(FORWARD, status('failed', { code: 'timeout', error: '' }))).toBe(
      "The tunnel didn't open within 30 seconds.",
    );
    expect(failureText(FORWARD, status('failed', { code: 'weird', error: 'Raw text' }))).toBe('Raw text');
    expect(failureText(FORWARD, status('active'))).toBe('');
  });

  it('builds editor drafts', () => {
    expect(draftOf('c1')).toEqual({
      id: null,
      connectionId: 'c1',
      name: '',
      localPort: '',
      remoteHost: '',
      remotePort: '',
    });
    expect(draftOf('c1', FORWARD)).toMatchObject({ id: 'p1', localPort: '13389', remotePort: '3389' });
  });
});

describe('port forward section', () => {
  const row = () => document.querySelector<HTMLElement>('[data-forward="p1"]')!;

  function render(options: Parameters<typeof renderView>[2] = {}) {
    const library = libraryWithForward();
    return renderView(
      ConnectionDetail,
      { connection: library.connections[0]! },
      {
        library,
        ...options,
        replies: {
          port_forward_start: null,
          port_forward_stop: null,
          port_forward_delete: null,
          ...options.replies,
        },
      },
    );
  }

  it('lists forwards with their state and starts or stops them', async () => {
    const { bridge, forwards, toast } = render();
    expect(text()).toContain('RDP');
    expect(text()).toContain('localhost:13389 → 192.0.2.20:3389');
    expect(row().querySelector('[aria-label="Stopped"]')).toBeTruthy();

    click(buttonByText('Start'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('port_forward_start', { id: 'p1' });

    forwards.apply(status('starting'));
    flushSync();
    expect(row().querySelector('[aria-label="Starting…"]')).toBeTruthy();
    forwards.apply(status('active'));
    flushSync();
    expect(row().querySelector('[aria-label="Active"]')).toBeTruthy();

    click(row().querySelector('[aria-label="Copy address"]')!);
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('copy_text', { text: 'localhost:13389' });
    expect(toast.current?.message).toBe('Copied');

    click(buttonByText('Stop'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('port_forward_stop', { id: 'p1' });
  });

  it('shows a failure in red with the English error as tooltip', () => {
    const { forwards } = render();
    forwards.apply(status('failed', { code: 'authFailed', error: 'Permission denied (password).' }));
    flushSync();
    const icon = row().querySelector<HTMLElement>('[aria-label="Failed"]')!;
    expect(icon.title).toBe('Permission denied (password).');
    expect(row().querySelector('.error')?.textContent).toBe('Authentication failed');
    expect(buttonByText('Start')).toBeTruthy();
  });

  it('reports a start error', async () => {
    const { alerts } = render({
      replies: { port_forward_start: () => Promise.reject({ code: 'locked', message: 'x', detail: null }) },
    });
    click(buttonByText('Start'));
    await settle();
    expect(alerts.current).toBe('RemoteDesktop is locked.');
  });

  it('creates a forward, showing validation errors inline', async () => {
    let attempts = 0;
    const { bridge } = render({
      replies: {
        port_forward_save: () =>
          ++attempts === 1
            ? Promise.reject({
                code: 'missingRemoteHost',
                message: 'Enter the remote host.',
                detail: null,
              })
            : 'p2',
      },
    });
    click(buttonByText('New forward'));
    expect(text()).toContain('New port forward');
    const inputs = [...document.querySelectorAll<HTMLInputElement>('.modal input, [role="dialog"] input')];
    expect(inputs).toHaveLength(4);
    type(inputs[1]!, '8080');
    type(inputs[3]!, '80');
    click(buttonByText('Save & Start'));
    await settle();
    expect(document.querySelector('[role="alert"]')?.textContent).toBe('Enter the remote host.');
    type(inputs[2]!, 'intranet');
    click(buttonByText('Save & Start'));
    await settle();
    expect(bridge.invoke).toHaveBeenLastCalledWith('port_forward_start', { id: 'p2' });
    expect(bridge.invoke).toHaveBeenCalledWith('port_forward_save', {
      forward: {
        id: null,
        connectionId: 'c1',
        name: '',
        localPort: '8080',
        remoteHost: 'intranet',
        remotePort: '80',
      },
    });
    expect(text()).not.toContain('New port forward');
  });

  it('edits through the button and the context menu, and deletes', async () => {
    const { bridge } = render({ replies: { port_forward_save: 'p1' } });
    click(row().querySelector('[aria-label="Edit…"]')!);
    expect(text()).toContain('Edit port forward');
    const inputs = [...document.querySelectorAll<HTMLInputElement>('[role="dialog"] input')];
    expect(inputs.map((i) => i.value)).toEqual(['RDP', '13389', '192.0.2.20', '3389']);
    click(buttonByText('Save'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('port_forward_save', {
      forward: expect.objectContaining({ id: 'p1', localPort: '13389' }),
    });
    expect(bridge.invoke).not.toHaveBeenCalledWith('port_forward_start', expect.anything());

    row().dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, clientX: 5, clientY: 5 }));
    const items = contextMenu.open!.items as { label: string; action?: () => void }[];
    expect(items.map((i) => i.label)).toEqual(['Edit…', 'Delete']);
    items[1]!.action!();
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('port_forward_delete', { id: 'p1' });
  });

  it('disables the editing controls when read-only', () => {
    render({ readOnly: true });
    expect(buttonByText('New forward').disabled).toBe(true);
    expect(row().querySelector<HTMLButtonElement>('[aria-label="Delete"]')!.disabled).toBe(true);
    expect(buttonByText('Start').disabled).toBe(false);
  });

  it('is only shown for SSH connections', () => {
    const library = libraryWithForward();
    const mikrotik = library.connections.find((c) => c.type === 'mikrotik')!;
    renderView(ConnectionDetail, { connection: mikrotik }, { library });
    expect(text()).not.toContain('Port forwards');
  });
});

describe('host key prompt', () => {
  const prompt = (requestId: string, port = 2221): HostKeyPromptDto => ({
    requestId,
    forwardId: 'p1',
    host: '192.0.2.5',
    port,
    keyType: 'ssh-ed25519',
    fingerprint: 'SHA256:UrfsIL8MHH+8HuZ73p6ZKMoNYLt5jyAz6OjfkLXRibA',
  });

  let mounted: ReturnType<typeof mount> | undefined;
  afterEach(() => {
    if (mounted) unmount(mounted);
    mounted = undefined;
  });

  function setup() {
    const bridge = fakeBridge({ host_key_answer: null });
    const store = createHostKeyStore(bridge);
    mounted = mount(HostKeyDialog, { target: document.body, props: { store } });
    flushSync();
    return { bridge, store };
  }

  it('queues prompts and drops closed ones', async () => {
    const { bridge, store } = setup();
    await store.start();
    bridge.emit('host-key-prompt', prompt('R1'));
    bridge.emit('host-key-prompt', prompt('R2', 22));
    bridge.emit('host-key-prompt', prompt('R1'));
    expect(store.queued).toBe(2);
    bridge.emit('host-key-closed', { requestId: 'R1' });
    expect(store.current?.requestId).toBe('R2');
    store.stop();
    expect(bridge.listenerCount('host-key-prompt')).toBe(0);
  });

  it('shows the host, key type and fingerprint; Accept and Reject answer', async () => {
    const { bridge, store } = setup();
    store.add(prompt('R1'));
    store.add(prompt('R2', 22));
    flushSync();
    expect(text()).toContain('Unknown host key');
    expect(text()).toContain('[192.0.2.5]:2221');
    expect(text()).toContain('ssh-ed25519');
    const fingerprint = document.querySelector('[data-testid="host-key-fingerprint"]')!;
    expect(fingerprint.textContent).toBe('SHA256:UrfsIL8MHH+8HuZ73p6ZKMoNYLt5jyAz6OjfkLXRibA');
    expect(fingerprint.className).toContain('mono');

    click(buttonByText('Accept and Start'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('host_key_answer', { requestId: 'R1', accept: true });
    expect(text()).toContain('192.0.2.5');
    expect(text()).not.toContain('[192.0.2.5]:2221');

    document
      .querySelector('[role="dialog"]')!
      .dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('host_key_answer', { requestId: 'R2', accept: false });
    expect(store.current).toBeNull();
    await store.answer(true);
  });

  it('warns about a changed key; Replace key accepts, Cancel rejects', async () => {
    const { bridge, store } = setup();
    const changed = { file: '/home/u/.ssh/known_hosts', line: 7 };
    store.add({ ...prompt('R1'), changed });
    store.add({ ...prompt('R2'), changed });
    flushSync();
    expect(text()).toContain('Host key changed');
    expect(text()).not.toContain('Unknown host key');
    expect(text()).toContain('someone is intercepting the connection');
    expect(text()).toContain('[192.0.2.5]:2221');
    expect(document.querySelector('[data-testid="host-key-stored"]')!.textContent!.trim()).toBe(
      '/home/u/.ssh/known_hosts:7',
    );
    expect(document.querySelector('[data-testid="host-key-fingerprint"]')!.textContent).toBe(
      'SHA256:UrfsIL8MHH+8HuZ73p6ZKMoNYLt5jyAz6OjfkLXRibA',
    );
    expect(buttonByText('Replace key').className).toContain('destructive');
    expect(() => buttonByText('Accept and Start')).toThrow();

    click(buttonByText('Replace key'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('host_key_answer', { requestId: 'R1', accept: true });
    click(buttonByText('Cancel'));
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith('host_key_answer', { requestId: 'R2', accept: false });
    expect(store.current).toBeNull();
  });
});
