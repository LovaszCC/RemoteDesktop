import { describe, expect, it, vi } from 'vitest';
import { createApi } from '../api';
import { createAlertStore } from '../stores/alert.svelte';
import { createDialogStore } from '../stores/dialogs.svelte';
import { createToastStore } from '../stores/toast.svelte';
import { TabsStore } from '../tabs.svelte';
import { fakeBridge } from '../testing/bridge';
import { entry } from '../testing/fixtures';
import { encodeFrame } from '../testing/rdpFrames';
import type { RdpEvent, RdpEventDto, TypeClipboardDto, EntryDto } from '../types';
import type { Frame } from './frame';
import { createRdpSessionsStore, type FrameChannel } from './sessions.svelte';

const connection = { id: 'c1', name: 'Desk', type: 'rdp' as const, color: 'blue' as const };

function setup(replies: Parameters<typeof fakeBridge>[0] = {}, entries: EntryDto[] = []) {
  let n = 0;
  const bridge = fakeBridge({
    open_rdp_session: () => ({ sessionId: `S${++n}` }),
    rdp_ack_frame: null,
    rdp_input: null,
    rdp_release_all_keys: null,
    rdp_resize: null,
    rdp_close: null,
    rdp_reconnect: null,
    rdp_capture_keyboard: { mode: 'none', active: false },
    rdp_type_clipboard: { status: 'typing', chars: 3 },
    rdp_cancel_file_transfer: null,
    ...replies,
  });
  const tabs = new TabsStore();
  const toast = createToastStore();
  const alerts = createAlertStore();
  const dialogs = createDialogStore();
  const channels: FrameChannel[] = [];
  const surfaces: { paint: ReturnType<typeof vi.fn>; dispose: ReturnType<typeof vi.fn> }[] = [];
  const sessions = createRdpSessionsStore({
    api: createApi(bridge.invoke),
    listen: bridge.listen,
    tabs,
    toast,
    alerts,
    dialogs,
    createSurface: () => {
      const surface = { element: {} as HTMLCanvasElement, paint: vi.fn(), dispose: vi.fn() };
      surfaces.push(surface);
      return surface;
    },
    createChannel: () => {
      const channel = { onmessage: () => {} } as FrameChannel;
      channels.push(channel);
      return channel;
    },
    defaultSize: () => ({ width: 1200, height: 760 }),
    library: { entry: (id) => entries.find((e) => e.id === id) },
  });
  return { bridge, tabs, toast, alerts, dialogs, channels, surfaces, sessions };
}

const event = (e: RdpEvent, sessionId = 'S1') => ({ sessionId, ...e }) as RdpEventDto;
const calls = (bridge: ReturnType<typeof fakeBridge>, command: string) =>
  bridge.invoke.mock.calls.filter(([c]) => c === command).map(([, args]) => args);
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('rdp sessions store', () => {
  it('a multi-connection entry titles RDP tabs with the label and groups them by entry', async () => {
    const { sessions, tabs } = setup({}, [entry('e1', [], { connectionIds: ['h1', 'h2'] })]);
    await sessions.open({
      ...connection,
      id: 'h2',
      entryId: 'e1',
      name: 'Home Server',
      displayLabel: 'Console',
    });
    await sessions.open(connection);
    expect(tabs.session('S1')).toMatchObject({ title: 'Home Server · Console', group: 'e1' });
    expect(tabs.session('S2')).toMatchObject({ title: 'Desk', group: 'c1' });
  });

  it('opens a session in a new tab sized like the view', async () => {
    const { sessions, tabs, bridge, channels } = setup();
    const id = await sessions.open(connection);
    expect(id).toBe('S1');
    expect(bridge.invoke).toHaveBeenCalledWith('open_rdp_session', {
      connectionId: 'c1',
      width: 1200,
      height: 760,
      onFrame: channels[0],
    });
    expect(tabs.active).toBe('S1');
    expect(tabs.session('S1')).toMatchObject({ title: 'Desk', type: 'rdp', color: 'blue' });
    expect(sessions.active?.status).toEqual({ kind: 'connecting' });
    expect(sessions.surface('S1')).toBeDefined();
  });

  it('opens a second session of a connection next to the first, numbered', async () => {
    const { sessions, tabs } = setup();
    await sessions.open(connection);
    await sessions.open({ ...connection, id: 'c2', name: 'Other' });
    await sessions.open(connection);
    expect(tabs.ids).toEqual(['S1', 'S3', 'S2']);
    expect(tabs.sessions.map((s) => tabs.displayTitle(s))).toEqual(['Desk', 'Desk (2)', 'Other']);
  });

  it('paints each frame and acknowledges it, also frames that came before the open answered', async () => {
    let resolveOpen: (value: { sessionId: string }) => void = () => {};
    const { sessions, bridge, channels, surfaces } = setup({
      open_rdp_session: () => new Promise((resolve) => (resolveOpen = resolve)),
    });
    const opening = sessions.open(connection);
    await flush();
    const frame = encodeFrame({
      seq: 1,
      width: 800,
      height: 600,
      rects: [{ x: 0, y: 0, width: 1, height: 1 }],
    });
    channels[0]!.onmessage(frame.buffer);
    expect(surfaces[0]!.paint).toHaveBeenCalledTimes(1);
    expect(calls(bridge, 'rdp_ack_frame')).toEqual([]);
    resolveOpen({ sessionId: 'S1' });
    await opening;
    expect(calls(bridge, 'rdp_ack_frame')).toEqual([{ sessionId: 'S1' }]);
    channels[0]!.onmessage(frame.buffer);
    const painted = surfaces[0]!.paint.mock.calls[1]![0] as Frame;
    expect([painted.seq, painted.width, painted.height, painted.rects.length]).toEqual([1, 800, 600, 1]);
    expect(calls(bridge, 'rdp_ack_frame')).toHaveLength(2);
    // A malformed message is still acknowledged so the stream doesn't stall.
    channels[0]!.onmessage(new ArrayBuffer(3));
    expect(surfaces[0]!.paint).toHaveBeenCalledTimes(2);
    expect(calls(bridge, 'rdp_ack_frame')).toHaveLength(3);
  });

  it('disposes the surface when opening fails', async () => {
    const { sessions, tabs, surfaces } = setup({
      open_rdp_session: () => {
        throw { code: 'emptyHost', message: 'The connection has no host.', detail: null };
      },
    });
    await expect(sessions.open(connection)).rejects.toMatchObject({ code: 'emptyHost' });
    expect(surfaces[0]!.dispose).toHaveBeenCalled();
    expect(tabs.hasSessions).toBe(false);
  });

  it('follows the events, including those that arrive before the open answers', async () => {
    const { sessions, bridge, toast } = setup();
    await sessions.start();
    bridge.emit('rdp-event', event({ kind: 'connected', width: 1200, height: 760 }));
    await sessions.open(connection);
    expect(sessions.state('S1')).toMatchObject({
      status: { kind: 'connected' },
      desktop: { width: 1200, height: 760 },
    });
    bridge.emit('rdp-event', event({ kind: 'remoteClipboard', chars: 12 }));
    expect(toast.current?.message).toBe('Remote clipboard copied (12 characters)');
    bridge.emit('rdp-event', event({ kind: 'reconnecting', attempt: 1 }));
    expect(sessions.state('S1')?.status).toEqual({ kind: 'reconnecting', attempt: 1 });
    sessions.stop();
    expect(bridge.listenerCount('rdp-event')).toBe(0);
  });

  it('closes the tab on a clean disconnect and ends the session when the tab closes', async () => {
    const { sessions, tabs, bridge, surfaces } = setup();
    await sessions.open(connection);
    await sessions.open(connection);
    sessions.handleEvent(event({ kind: 'disconnected', clean: true, code: 12, message: '' }));
    expect(tabs.session('S1')).toBeUndefined();
    expect(sessions.has('S1')).toBe(false);
    expect(surfaces[0]!.dispose).toHaveBeenCalled();
    expect(calls(bridge, 'rdp_close')).toEqual([{ sessionId: 'S1' }]);
    sessions.handleEvent(event({ kind: 'disconnected', clean: false, code: 0, message: '' }, 'S2'));
    expect(tabs.session('S2')).toBeDefined();
    sessions.close('S2');
    expect(calls(bridge, 'rdp_close')).toEqual([{ sessionId: 'S1' }, { sessionId: 'S2' }]);
  });

  it('sends input only while connected, and changed view sizes', async () => {
    const { sessions, bridge } = setup();
    await sessions.open(connection);
    sessions.input('S1', [{ type: 'mouseMove', x: 1, y: 2 }]);
    expect(calls(bridge, 'rdp_input')).toEqual([]);
    sessions.handleEvent(event({ kind: 'connected', width: 1200, height: 760 }));
    sessions.input('S1', [{ type: 'mouseMove', x: 1, y: 2 }]);
    sessions.input('S1', []);
    expect(calls(bridge, 'rdp_input')).toEqual([
      { sessionId: 'S1', events: [{ type: 'mouseMove', x: 1, y: 2 }] },
    ]);

    sessions.resize('S1', { width: 0, height: 0 });
    sessions.resize('S1', { width: 1000, height: 700 });
    sessions.resize('S1', { width: 1000, height: 700 });
    expect(calls(bridge, 'rdp_resize')).toEqual([{ sessionId: 'S1', width: 1000, height: 700 }]);
    // The next session starts at the last view size.
    await sessions.open(connection);
    expect(calls(bridge, 'open_rdp_session')[1]).toMatchObject({ width: 1000, height: 700 });

    sessions.releaseAllKeys('S1');
    sessions.releaseAllKeys('nope');
    expect(calls(bridge, 'rdp_release_all_keys')).toEqual([{ sessionId: 'S1' }]);
  });

  it('reports a keyboard grab, which bypasses the canvas', async () => {
    const { sessions, bridge } = setup({ rdp_capture_keyboard: { mode: 'grab', active: true } });
    await sessions.open(connection);
    expect(await sessions.captureKeyboard('S1', true)).toBe(true);
    expect(calls(bridge, 'rdp_capture_keyboard')).toEqual([{ sessionId: 'S1', active: true }]);
    expect(await sessions.captureKeyboard('nope', true)).toBe(false);
    const failing = setup({
      rdp_capture_keyboard: () => {
        throw { code: 'captureFailed', message: "Couldn't capture the keyboard.", detail: null };
      },
    });
    await failing.sessions.open(connection);
    expect(await failing.sessions.captureKeyboard('S1', true)).toBe(false);
  });

  it('reconnects an ended session with the view size and keeps a failure in the exit bar', async () => {
    let fail = false;
    const { sessions, bridge } = setup({
      rdp_reconnect: () => {
        if (fail) throw { code: 'notFound', message: 'The connection no longer exists.', detail: null };
        return null;
      },
    });
    await sessions.open(connection);
    await sessions.reconnect('S1');
    expect(calls(bridge, 'rdp_reconnect')).toEqual([]);
    sessions.resize('S1', { width: 900, height: 640 });
    sessions.handleEvent(event({ kind: 'disconnected', clean: false, code: 0, message: '' }));
    sessions.resize('S1', { width: 902, height: 640 });
    expect(calls(bridge, 'rdp_resize')).toHaveLength(1);
    await sessions.reconnect('S1');
    expect(calls(bridge, 'rdp_reconnect')).toEqual([{ sessionId: 'S1', width: 902, height: 640 }]);
    expect(sessions.state('S1')?.status).toEqual({ kind: 'connecting' });
    sessions.handleEvent(event({ kind: 'disconnected', clean: false, code: 0, message: '' }));
    fail = true;
    await sessions.reconnect('S1');
    expect(sessions.state('S1')?.status).toEqual({
      kind: 'disconnected',
      clean: false,
      code: 0,
      message: "The item doesn't exist any more.",
    });
  });

  describe('type clipboard', () => {
    async function connected(reply: TypeClipboardDto | ((args?: Record<string, unknown>) => unknown)) {
      const env = setup({ rdp_type_clipboard: reply });
      await env.sessions.open(connection);
      env.sessions.handleEvent(event({ kind: 'connected', width: 1200, height: 760 }));
      return env;
    }

    it('types and is busy until the typed event', async () => {
      const { sessions, bridge } = await connected({ status: 'typing', chars: 3 });
      await sessions.typeClipboard('S1');
      expect(calls(bridge, 'rdp_type_clipboard')).toEqual([{ sessionId: 'S1', confirmed: false }]);
      expect(sessions.state('S1')?.typing).toBe(true);
      await sessions.typeClipboard('S1');
      expect(calls(bridge, 'rdp_type_clipboard')).toHaveLength(1);
      sessions.handleEvent(event({ kind: 'typed', finished: true }));
      expect(sessions.state('S1')?.typing).toBe(false);
    });

    it('asks before a long text and types it confirmed', async () => {
      const { sessions, bridge, dialogs } = await connected((args) =>
        args?.confirmed
          ? { status: 'typing', chars: 2500 }
          : { status: 'needsConfirmation', chars: 2500, seconds: 6 },
      );
      const typing = sessions.typeClipboard('S1');
      await flush();
      expect(dialogs.current).toMatchObject({
        kind: 'confirm',
        title: 'Type a long text?',
        message: '2500 characters, typing takes about 6 s. Continue?',
        confirmLabel: 'Type',
      });
      dialogs.accept();
      await typing;
      expect(calls(bridge, 'rdp_type_clipboard')).toEqual([
        { sessionId: 'S1', confirmed: false },
        { sessionId: 'S1', confirmed: true },
      ]);
      expect(sessions.state('S1')?.typing).toBe(true);
    });

    it('does nothing when the long text is declined', async () => {
      const { sessions, bridge, dialogs } = await connected({
        status: 'needsConfirmation',
        chars: 2500,
        seconds: 6,
      });
      const typing = sessions.typeClipboard('S1');
      await flush();
      dialogs.cancel();
      await typing;
      expect(calls(bridge, 'rdp_type_clipboard')).toHaveLength(1);
    });

    it('toasts an empty clipboard and a busy session, alerts errors', async () => {
      const empty = await connected({ status: 'empty' });
      await empty.sessions.typeClipboard('S1');
      expect(empty.toast.current?.message).toBe("There's no text on the clipboard");
      const busy = await connected({ status: 'busy' });
      await busy.sessions.typeClipboard('S1');
      expect(busy.toast.current?.message).toBe("Can't type now: the session is busy or not connected");
      const failing = await connected(() => {
        throw { code: 'clipboard', message: 'The clipboard is not available.', detail: null };
      });
      await failing.sessions.typeClipboard('S1');
      expect(failing.alerts.current).toBe('The clipboard is not available.');
    });

    it('needs a connected session', async () => {
      const { sessions, bridge } = setup();
      await sessions.open(connection);
      await sessions.typeClipboard('S1');
      await sessions.typeClipboard('nope');
      expect(calls(bridge, 'rdp_type_clipboard')).toEqual([]);
    });
  });

  it('follows remote file downloads, toasts their end and cancels only a running one', async () => {
    const { sessions, bridge, toast } = setup();
    await sessions.start();
    await sessions.open(connection);
    sessions.cancelFileTransfer('S1');
    expect(calls(bridge, 'rdp_cancel_file_transfer')).toEqual([]);
    bridge.emit('rdp-event', event({ kind: 'fileProgress', done: 1, total: 2 }));
    expect(sessions.state('S1')?.transfer).toEqual({ done: 1, total: 2 });
    sessions.cancelFileTransfer('S1');
    expect(calls(bridge, 'rdp_cancel_file_transfer')).toEqual([{ sessionId: 'S1' }]);
    bridge.emit('rdp-event', event({ kind: 'filesReady', count: 2 }));
    expect(sessions.state('S1')?.transfer).toBeNull();
    expect(toast.current?.message).toBe('2 remote files copied to the clipboard');
    toast.dismiss();
    bridge.emit(
      'rdp-event',
      event({ kind: 'fileTransferFailed', message: 'the server stopped sending the file' }),
    );
    expect(toast.current?.message).toBe('Copying files failed: the server stopped sending the file');
    sessions.dispose();
  });

  it('follows the connection colours', async () => {
    const { sessions, tabs } = setup();
    await sessions.open(connection);
    sessions.syncConnections([{ id: 'c1', color: 'green' }]);
    expect(tabs.session('S1')?.color).toBe('green');
    sessions.dispose();
  });
});
