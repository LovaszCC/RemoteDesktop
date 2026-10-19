import { flushSync, mount, unmount } from 'svelte';
import { describe, expect, it, vi } from 'vitest';
import { createApi } from '../api';
import HostKeyDialog from '../forwards/HostKeyDialog.svelte';
import { createAlertStore } from '../stores/alert.svelte';
import { createToastStore } from '../stores/toast.svelte';
import { TabsStore } from '../tabs.svelte';
import { fakeBridge } from '../testing/bridge';
import { buttonByText, click, settle, text } from '../testing/views';
import type { ClearAction, SshEvent, SshEventDto } from '../types';
import { createSessionsStore, toBytes, type OutputChannel } from './sessions.svelte';
import type { TerminalHandle, TerminalOptions } from './xterm';

/** A terminal without xterm: records what it was asked to do. */
function fakeTerminal() {
  const made: {
    options: TerminalOptions;
    handle: TerminalHandle & Record<string, ReturnType<typeof vi.fn>>;
  }[] = [];
  const factory = (options: TerminalOptions) => {
    const handle = {
      write: vi.fn(),
      clearLocal: vi.fn(),
      attach: vi.fn(),
      detach: vi.fn(),
      fit: vi.fn(),
      size: vi.fn(() => ({ cols: 100, rows: 30 })),
      focus: vi.fn(),
      dispose: vi.fn(),
    };
    made.push({ options, handle });
    return handle;
  };
  return { factory, made };
}

const connection = { id: 'c1', name: 'web01', type: 'ssh' as const, color: 'red' as const };

function setup(replies: Parameters<typeof fakeBridge>[0] = {}) {
  let n = 0;
  const bridge = fakeBridge({
    open_ssh_session: () => ({ sessionId: `S${++n}` }),
    ssh_input: null,
    ssh_resize: null,
    ssh_close: null,
    ssh_reconnect: null,
    ssh_record_next_output: 'armed',
    ssh_record_cancel: null,
    copy_text: null,
    terminal_paste_text: 'pasted',
    terminal_open_url: null,
    ...replies,
  });
  const tabs = new TabsStore();
  const toast = createToastStore();
  const alerts = createAlertStore();
  const terminal = fakeTerminal();
  const channels: OutputChannel[] = [];
  const sessions = createSessionsStore({
    api: createApi(bridge.invoke),
    listen: bridge.listen,
    tabs,
    toast,
    alerts,
    createTerminal: terminal.factory,
    createChannel: () => {
      const channel = { onmessage: () => {} } as OutputChannel;
      channels.push(channel);
      return channel;
    },
    platform: () => 'linux',
    shortcuts: () => [],
  });
  return { bridge, tabs, toast, alerts, terminal, channels, sessions };
}

const event = (e: SshEvent, sessionId = 'S1') => ({ sessionId, ...e }) as SshEventDto;

describe('sessions store', () => {
  it('opens a session in a new tab with the connection name and colour', async () => {
    const { sessions, tabs, bridge, channels, terminal } = setup();
    const id = await sessions.open(connection);
    expect(id).toBe('S1');
    expect(bridge.invoke).toHaveBeenCalledWith('open_ssh_session', {
      connectionId: 'c1',
      cols: 100,
      rows: 30,
      onData: channels[0],
    });
    expect(tabs.active).toBe('S1');
    expect(tabs.session('S1')).toMatchObject({ title: 'web01', type: 'ssh', color: 'red' });
    expect(sessions.active?.connectionId).toBe('c1');
    expect(sessions.runningCount).toBe(1);

    // Output bytes go straight to the terminal.
    const bytes = new Uint8Array([104, 105]);
    channels[0]!.onmessage(bytes.buffer);
    expect(terminal.made[0]!.handle.write).toHaveBeenCalledWith(bytes);
  });

  it('opens a second session of a connection next to the first, numbered', async () => {
    const { sessions, tabs } = setup();
    await sessions.open(connection);
    await sessions.open({ ...connection, id: 'c2', name: 'db01' });
    await sessions.open(connection);
    expect(tabs.ids).toEqual(['S1', 'S3', 'S2']);
    expect(tabs.sessions.map((s) => tabs.displayTitle(s))).toEqual(['web01', 'web01 (2)', 'db01']);
    sessions.handleEvent(event({ kind: 'title', title: 'root@web01: ~' }, 'S3'));
    expect(tabs.displayTitle(tabs.session('S3')!)).toBe('root@web01: ~ (2)');
  });

  it('opens numbered local terminals that reconnect and close like SSH sessions', async () => {
    let n = 0;
    const { sessions, tabs, bridge, channels } = setup({
      open_local_terminal: () => ({ sessionId: `L${++n}` }),
    });
    expect(await sessions.openLocal()).toBe('L1');
    expect(bridge.invoke).toHaveBeenCalledWith('open_local_terminal', {
      cols: 100,
      rows: 30,
      onData: channels[0],
    });
    expect(bridge.invoke).not.toHaveBeenCalledWith('open_ssh_session', expect.anything());
    expect(tabs.session('L1')).toMatchObject({ title: 'Local Terminal', type: 'ssh', color: null });
    expect(sessions.state('L1')?.connectionId).toBe('local-terminal');

    await sessions.openLocal();
    await sessions.open(connection);
    expect(tabs.session('L2')?.title).toBe('Local Terminal 2');
    tabs.closeSession('L1');
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_close', { sessionId: 'L1' });
    await sessions.openLocal();
    // The lowest free name.
    expect(tabs.session('L3')?.title).toBe('Local Terminal');

    sessions.handleEvent(event({ kind: 'exit', code: 130, authFailed: false }, 'L2'));
    await sessions.reconnect('L2');
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_reconnect', { sessionId: 'L2', cols: 100, rows: 30 });
    expect(sessions.state('L2')?.status.kind).toBe('running');
  });

  it('disposes the terminal when opening fails', async () => {
    const { sessions, tabs, terminal } = setup({
      open_ssh_session: () => {
        throw { code: 'sshNotFound', message: 'The ssh program is not installed.', detail: null };
      },
    });
    await expect(sessions.open(connection)).rejects.toMatchObject({ code: 'sshNotFound' });
    expect(terminal.made[0]!.handle.dispose).toHaveBeenCalled();
    expect(tabs.hasSessions).toBe(false);
  });

  it('sends input and settled sizes while running', async () => {
    const { sessions, bridge, terminal } = setup();
    await sessions.open(connection);
    const { options } = terminal.made[0]!;
    options.onInput('ls\r');
    options.onInput([27, 91, 77]);
    options.onResize({ cols: 120, rows: 40 });
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_input', { sessionId: 'S1', data: 'ls\r' });
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_input', { sessionId: 'S1', data: [27, 91, 77] });
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_resize', { sessionId: 'S1', cols: 120, rows: 40 });

    sessions.handleEvent(event({ kind: 'exit', code: 1, authFailed: false }));
    bridge.invoke.mockClear();
    options.onInput('x');
    expect(bridge.invoke).not.toHaveBeenCalled();
  });

  it('updates the tab from events and keeps events that arrive before the open answers', async () => {
    const { sessions, tabs } = setup();
    sessions.handleEvent(event({ kind: 'root', root: true }));
    await sessions.open(connection);
    expect(tabs.session('S1')?.root).toBe(true);
    sessions.handleEvent(event({ kind: 'title', title: 'root@web01: ~' }));
    expect(tabs.displayTitle(tabs.session('S1')!)).toBe('root@web01: ~');
    sessions.handleEvent(event({ kind: 'title', title: 'x' }, 'unknown'));
    expect(tabs.sessions).toHaveLength(1);
  });

  it('closes the tab on exit 0 and ends the session when the tab closes', async () => {
    const { sessions, tabs, bridge, terminal } = setup();
    await sessions.open(connection);
    await sessions.open(connection);
    sessions.handleEvent(event({ kind: 'exit', code: 0, authFailed: false }));
    expect(tabs.session('S1')).toBeUndefined();
    expect(sessions.has('S1')).toBe(false);
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_close', { sessionId: 'S1' });
    expect(terminal.made[0]!.handle.dispose).toHaveBeenCalled();

    tabs.closeActive();
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_close', { sessionId: 'S2' });
    expect(sessions.has('S2')).toBe(false);
  });

  it('shows the exit bar on other exits and reconnects in the same tab', async () => {
    const { sessions, bridge } = setup();
    await sessions.open(connection);
    sessions.handleEvent(event({ kind: 'root', root: true }));
    sessions.handleEvent(event({ kind: 'exit', code: 255, authFailed: true }));
    expect(sessions.state('S1')?.status).toMatchObject({ kind: 'exited', code: 255, authFailed: true });
    expect(sessions.state('S1')?.root).toBe(false);
    expect(sessions.runningCount).toBe(0);

    await sessions.reconnect('S1');
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_reconnect', { sessionId: 'S1', cols: 100, rows: 30 });
    expect(sessions.state('S1')?.status.kind).toBe('running');
    // A running session isn't reconnected again.
    await sessions.reconnect('S1');
    expect(bridge.invoke.mock.calls.filter(([c]) => c === 'ssh_reconnect')).toHaveLength(1);
  });

  describe('a changed host key refused by ssh', () => {
    const hostKeyChanged = {
      host: '[192.0.2.5]:2221',
      keyType: 'ED25519',
      fingerprint: 'SHA256:new',
      file: '/home/u/.ssh/known_hosts',
      line: 2,
    };
    const refused = (sessionId = 'S1') =>
      event({ kind: 'exit', code: 255, authFailed: false, hostKeyChanged }, sessionId);

    it('is offered for replacing; Replace key replaces it and reconnects', async () => {
      const { sessions, bridge } = setup({ ssh_replace_host_key: null });
      await sessions.open(connection);
      expect(sessions.hostKeyPrompts.current).toBeNull();
      sessions.handleEvent(event({ kind: 'exit', code: 255, authFailed: true }));
      expect(sessions.hostKeyPrompts.current).toBeNull();
      await sessions.reconnect('S1');
      sessions.handleEvent(refused());
      expect(sessions.hostKeyPrompts.current).toEqual({
        requestId: 'S1',
        host: '[192.0.2.5]:2221',
        port: 22,
        keyType: 'ED25519',
        fingerprint: 'SHA256:new',
        changed: { file: '/home/u/.ssh/known_hosts', line: 2 },
      });

      await sessions.hostKeyPrompts.answer(true);
      const calls = bridge.invoke.mock.calls.map(([c]) => c);
      expect(bridge.invoke).toHaveBeenCalledWith('ssh_replace_host_key', { sessionId: 'S1' });
      expect(calls.lastIndexOf('ssh_reconnect')).toBeGreaterThan(calls.indexOf('ssh_replace_host_key'));
      expect(sessions.state('S1')?.status.kind).toBe('running');
      expect(sessions.hostKeyPrompts.current).toBeNull();

      // Refused again after the reconnect: asked again.
      sessions.handleEvent(refused());
      expect(sessions.hostKeyPrompts.current?.requestId).toBe('S1');
    });

    it('Cancel keeps the exit bar without replacing; one prompt per session', async () => {
      const { sessions, bridge } = setup({ ssh_replace_host_key: null });
      await sessions.open(connection);
      await sessions.open(connection);
      sessions.handleEvent(refused('S1'));
      sessions.handleEvent(refused('S2'));
      expect(sessions.hostKeyPrompts.current?.requestId).toBe('S1');
      await sessions.hostKeyPrompts.answer(false);
      expect(sessions.hostKeyPrompts.current?.requestId).toBe('S2');
      expect(sessions.state('S1')?.status).toMatchObject({ kind: 'exited', hostKeyChanged });
      sessions.close('S2');
      expect(sessions.hostKeyPrompts.current).toBeNull();
      await sessions.hostKeyPrompts.answer(true);
      expect(bridge.invoke).not.toHaveBeenCalledWith('ssh_replace_host_key', expect.anything());
    });

    it('shows in the host key dialog as a changed key', async () => {
      const { sessions, bridge } = setup({ ssh_replace_host_key: null });
      await sessions.open(connection);
      const mounted = mount(HostKeyDialog, {
        target: document.body,
        props: { store: sessions.hostKeyPrompts },
      });
      try {
        sessions.handleEvent(refused());
        flushSync();
        expect(text()).toContain('Host key changed');
        expect(text()).toContain('[192.0.2.5]:2221');
        expect(document.querySelector('[data-testid="host-key-stored"]')!.textContent!.trim()).toBe(
          '/home/u/.ssh/known_hosts:2',
        );
        click(buttonByText('Replace key'));
        await settle();
        expect(bridge.invoke).toHaveBeenCalledWith('ssh_replace_host_key', { sessionId: 'S1' });
        expect(document.querySelector('[role="dialog"]')).toBeNull();
      } finally {
        unmount(mounted);
      }
    });

    it('a failed replacement is shown and nothing reconnects', async () => {
      const { sessions, bridge, alerts } = setup({
        ssh_replace_host_key: () => {
          throw { code: 'hostKeyReplace', message: "Couldn't replace the host key.", detail: 'denied' };
        },
      });
      await sessions.open(connection);
      sessions.handleEvent(refused());
      await sessions.hostKeyPrompts.answer(true);
      expect(alerts.current).toContain("Couldn't replace the host key.");
      expect(bridge.invoke).not.toHaveBeenCalledWith('ssh_reconnect', expect.anything());
      expect(sessions.state('S1')?.status.kind).toBe('exited');
    });
  });

  it('keeps a failed reconnect in the exit bar', async () => {
    const { sessions } = setup({
      ssh_reconnect: () => {
        throw { code: 'sshNotFound', message: 'The ssh program is not installed.', detail: null };
      },
    });
    await sessions.open(connection);
    sessions.handleEvent(event({ kind: 'exit', code: 1, authFailed: false }));
    await sessions.reconnect('S1');
    expect(sessions.state('S1')?.status).toEqual({
      kind: 'exited',
      code: null,
      authFailed: false,
      error: 'The ssh program is not installed.',
    });
  });

  it.each<[ClearAction, boolean, boolean]>([
    ['none', false, false],
    ['localOnly', true, false],
    ['localAndRedraw', true, true],
  ])('clear: %s', async (decision, clears, redraws) => {
    const { sessions, bridge, terminal } = setup({ ssh_clear_decision: decision });
    await sessions.open(connection);
    await sessions.clear('S1');
    expect(terminal.made[0]!.handle.clearLocal).toHaveBeenCalledTimes(clears ? 1 : 0);
    const redraw = bridge.invoke.mock.calls.some(
      ([c, a]) => c === 'ssh_input' && (a as { data: string }).data === '\x0c',
    );
    expect(redraw).toBe(redraws);
  });

  it('toggles and cancels "copy next output", toasting the result', async () => {
    const { sessions, bridge, toast, terminal } = setup();
    await sessions.open(connection);
    await sessions.toggleRecording('S1');
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_record_next_output', { sessionId: 'S1' });
    expect(sessions.state('S1')?.recording).toBe('armed');
    expect(terminal.made[0]!.options.keyContext().recording).toBe(true);

    terminal.made[0]!.options.cancelRecording();
    await Promise.resolve();
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_record_cancel', { sessionId: 'S1' });
    expect(sessions.state('S1')?.recording).toBe('idle');

    sessions.handleEvent(event({ kind: 'recorded', outcome: 'empty', text: '' }));
    expect(toast.current?.message).toBe('The command printed nothing');
  });

  it('copies, pastes and opens links through Rust', async () => {
    const { sessions, bridge, terminal } = setup();
    await sessions.open(connection);
    const { options } = terminal.made[0]!;
    options.copy('sel');
    expect(bridge.invoke).toHaveBeenCalledWith('copy_text', { text: 'sel' });
    expect(await options.paste()).toBe('pasted');
    options.openLink('https://example.com');
    expect(bridge.invoke).toHaveBeenCalledWith('terminal_open_url', { url: 'https://example.com' });
  });

  it('listens to ssh-event and follows the connection colours', async () => {
    const { sessions, bridge, tabs } = setup();
    await sessions.start();
    await sessions.open(connection);
    bridge.emit('ssh-event', event({ kind: 'title', title: 'htop' }));
    expect(tabs.session('S1')?.title).toBe('htop');
    sessions.syncConnections([{ id: 'c1', color: 'green' }]);
    expect(tabs.session('S1')?.color).toBe('green');
    sessions.stop();
    expect(bridge.listenerCount('ssh-event')).toBe(0);
  });

  it('turns channel payloads into bytes', () => {
    expect(toBytes(new Uint8Array([1]))).toEqual(new Uint8Array([1]));
    expect(toBytes([1, 2])).toEqual(new Uint8Array([1, 2]));
    expect(toBytes('x')).toEqual(new Uint8Array());
  });
});
