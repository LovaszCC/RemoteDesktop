import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { createApi } from '../api';
import { createAlertStore } from '../stores/alert.svelte';
import { createDialogStore } from '../stores/dialogs.svelte';
import type { SettingsStore } from '../stores/settings.svelte';
import { createToastStore } from '../stores/toast.svelte';
import { TabsStore } from '../tabs.svelte';
import { fakeBridge } from '../testing/bridge';
import type { RdpEvent, RdpEventDto, ShortcutDto } from '../types';
import RdpSessionView from './RdpSessionView.svelte';
import { createRdpSessionsStore, type RdpSessionsStore } from './sessions.svelte';

let mounted: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
});

// jsdom has no ResizeObserver: this one reports the size it is given.
let observed: ((entries: { contentRect: { width: number; height: number } }[]) => void) | null = null;
globalThis.ResizeObserver = class {
  constructor(callback: typeof observed) {
    observed = callback;
  }
  observe() {}
  disconnect() {}
  unobserve() {}
} as unknown as typeof ResizeObserver;

const row = (
  command: ShortcutDto['command'],
  key: string,
  modifiers: number,
  display: string,
): ShortcutDto => ({
  command,
  group: 'connection',
  shortcut: { key, modifiers },
  display,
  defaultDisplay: display,
  customized: false,
});
const rows = [
  row('closeTab', 'w', 1, '⌘W'),
  row('reconnect', 'r', 1, '⌘R'),
  row('typeClipboard', 'v', 3, '⇧⌘V'),
];
const settings = {
  shortcuts: rows,
  shortcut: (command: string) => rows.find((r) => r.command === command),
} as unknown as SettingsStore;

async function render() {
  const bridge = fakeBridge({
    open_rdp_session: { sessionId: 'S1' },
    rdp_ack_frame: null,
    rdp_input: null,
    rdp_release_all_keys: null,
    rdp_resize: null,
    rdp_close: null,
    rdp_reconnect: null,
    rdp_capture_keyboard: { mode: 'none', active: false },
    rdp_type_clipboard: { status: 'typing', chars: 2 },
    rdp_cancel_file_transfer: null,
  });
  const tabs = new TabsStore();
  const sessions = createRdpSessionsStore({
    api: createApi(bridge.invoke),
    listen: bridge.listen,
    tabs,
    toast: createToastStore(),
    alerts: createAlertStore(),
    dialogs: createDialogStore(),
    createChannel: () => ({ onmessage: () => {} }),
    defaultSize: () => ({ width: 1024, height: 768 }),
  });
  await sessions.open({ id: 'c1', name: 'Desk', type: 'rdp', color: null });
  mounted = mount(RdpSessionView, {
    target: document.body,
    props: { sessions, id: 'S1', active: true, settings, app: { platform: 'macos' } },
  });
  flushSync();
  const emit = (e: RdpEvent) => {
    sessions.handleEvent({ sessionId: 'S1', ...e } as RdpEventDto);
    flushSync();
  };
  const canvas = document.querySelector('canvas')!;
  const inputs = () =>
    bridge.invoke.mock.calls
      .filter(([c]) => c === 'rdp_input')
      .flatMap(([, a]) => (a as { events: unknown[] }).events);
  return { bridge, sessions, tabs, emit, canvas, inputs };
}

const button = (label: string) =>
  [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === label);
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));
const keydown = (target: EventTarget, init: KeyboardEventInit) => {
  const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
};
const keyup = (target: EventTarget, init: KeyboardEventInit) => {
  const event = new KeyboardEvent('keyup', { bubbles: true, cancelable: true, ...init });
  target.dispatchEvent(event);
  return event;
};

describe('RdpSessionView', () => {
  it('shows the session canvas with the connecting overlay and keeps the canvas on unmount', async () => {
    const { canvas, sessions, emit } = await render();
    expect(canvas).toBe(sessions.surface('S1')!.element);
    expect(canvas.closest('[data-rdp]')).not.toBeNull();
    expect(document.querySelector('[role="status"]')?.textContent).toContain('Connecting: Desk…');
    emit({ kind: 'connected', width: 1024, height: 768 });
    expect(document.querySelector('[role="status"]')).toBeNull();
    emit({ kind: 'reconnecting', attempt: 2 });
    expect(document.querySelector('[role="status"]')?.textContent).toContain('Reconnecting… (attempt 2)');
    unmount(mounted!);
    mounted = undefined;
    expect(sessions.has('S1')).toBe(true);
    expect(canvas.isConnected).toBe(false);
  });

  it('reports the view size', async () => {
    const { bridge } = await render();
    observed?.([{ contentRect: { width: 900.6, height: 600.2 } }]);
    expect(bridge.invoke).toHaveBeenCalledWith('rdp_resize', { sessionId: 'S1', width: 900, height: 600 });
  });

  it('sends keys and mouse input once connected, and keeps app shortcuts out', async () => {
    const { canvas, emit, inputs, bridge } = await render();
    emit({ kind: 'connected', width: 1024, height: 768 });
    const pressed = keydown(canvas, { code: 'KeyA', key: 'a' });
    keyup(canvas, { code: 'KeyA', key: 'a' });
    // A release whose press the session never saw is dropped.
    keyup(canvas, { code: 'MetaLeft', key: 'Meta' });
    expect(pressed.defaultPrevented).toBe(true);
    canvas.dispatchEvent(new MouseEvent('mousedown', { button: 2, clientX: 10, clientY: 20, bubbles: true }));
    window.dispatchEvent(new MouseEvent('mousemove', { clientX: 30, clientY: 40 }));
    window.dispatchEvent(new MouseEvent('mouseup', { button: 2, clientX: 30, clientY: 40 }));
    window.dispatchEvent(new MouseEvent('mousemove', { clientX: 50, clientY: 60 }));
    canvas.dispatchEvent(
      new WheelEvent('wheel', { deltaY: 3, deltaMode: 1, clientX: 5, clientY: 5, cancelable: true }),
    );
    expect(inputs()).toEqual([
      { type: 'key', code: 'KeyA', key: 'a', down: true },
      { type: 'key', code: 'KeyA', key: 'a', down: false },
      { type: 'mouseButton', button: 2, down: true, x: 10, y: 20 },
      { type: 'mouseMove', x: 30, y: 40 },
      { type: 'mouseButton', button: 2, down: false, x: 30, y: 40 },
      { type: 'wheel', deltaX: 0, deltaY: 3, deltaMode: 1, x: 5, y: 5 },
    ]);

    // ⌘W closes the tab through the window dispatcher: not sent, not prevented, keys released.
    bridge.invoke.mockClear();
    const close = keydown(canvas, { code: 'KeyW', key: 'w', metaKey: true });
    expect(close.defaultPrevented).toBe(false);
    expect(inputs()).toEqual([]);
    expect(bridge.invoke).toHaveBeenCalledWith('rdp_release_all_keys', { sessionId: 'S1' });
    // ⌘R goes to the session while it is connected (Ctrl+R there).
    const reload = keydown(canvas, { code: 'KeyR', key: 'r', metaKey: true });
    expect(reload.defaultPrevented).toBe(true);
    expect(inputs()).toEqual([{ type: 'key', code: 'KeyR', key: 'r', down: true }]);
  });

  it('captures the keyboard on focus and releases everything on blur', async () => {
    const { canvas, emit, bridge, inputs } = await render();
    emit({ kind: 'connected', width: 1024, height: 768 });
    canvas.dispatchEvent(new FocusEvent('focus'));
    expect(bridge.invoke).toHaveBeenCalledWith('rdp_capture_keyboard', { sessionId: 'S1', active: true });
    canvas.dispatchEvent(new MouseEvent('mousedown', { button: 0, clientX: 1, clientY: 1, bubbles: true }));
    bridge.invoke.mockClear();
    canvas.dispatchEvent(new FocusEvent('blur'));
    expect(bridge.invoke).toHaveBeenCalledWith('rdp_release_all_keys', { sessionId: 'S1' });
    expect(bridge.invoke).toHaveBeenCalledWith('rdp_capture_keyboard', { sessionId: 'S1', active: false });
    expect(inputs()).toEqual([{ type: 'mouseButton', button: 0, down: false, x: 0, y: 0 }]);
  });

  it('types the clipboard from the toolbar while connected', async () => {
    const { emit, bridge } = await render();
    expect(button('Type clipboard')?.disabled).toBe(true);
    emit({ kind: 'connected', width: 1024, height: 768 });
    button('Type clipboard')!.click();
    await flush();
    flushSync();
    expect(bridge.invoke).toHaveBeenCalledWith('rdp_type_clipboard', { sessionId: 'S1', confirmed: false });
    expect(button('Typing…')?.disabled).toBe(true);
  });

  it('shows a download of remote files with its progress and Cancel', async () => {
    const { emit, bridge } = await render();
    emit({ kind: 'connected', width: 1024, height: 768 });
    expect(document.querySelector('[role="progressbar"]')).toBeNull();
    emit({ kind: 'fileProgress', done: 250, total: 1000 });
    const bar = document.querySelector('[role="progressbar"]')!;
    expect(bar.getAttribute('aria-valuenow')).toBe('25');
    expect(document.body.textContent).toContain('Copying files from the remote computer…');
    expect(document.body.textContent).toContain('250 B of 1 KB');
    button('Cancel')!.click();
    expect(bridge.invoke).toHaveBeenCalledWith('rdp_cancel_file_transfer', { sessionId: 'S1' });
    emit({ kind: 'fileTransferCancelled' });
    expect(document.querySelector('[role="progressbar"]')).toBeNull();
    expect(button('Cancel')).toBeUndefined();
  });

  it('shows the exit bar with the reason, Reconnect and Close', async () => {
    const { bridge, emit, tabs } = await render();
    expect(document.querySelector('[role="alert"]')).toBeNull();
    emit({ kind: 'disconnected', clean: false, code: 0, message: '' });
    expect(document.querySelector('[role="alert"]')?.textContent).toContain(
      'The connection dropped unexpectedly (the server gave no reason)',
    );
    button('Reconnect')!.click();
    await flush();
    flushSync();
    expect(bridge.invoke).toHaveBeenCalledWith('rdp_reconnect', {
      sessionId: 'S1',
      width: 1024,
      height: 768,
    });
    expect(document.querySelector('[role="alert"]')).toBeNull();
    emit({ kind: 'disconnected', clean: false, code: 7, message: 'The server shut down' });
    expect(document.querySelector('[role="alert"]')?.textContent).toContain('The server shut down');
    button('Close')!.click();
    expect(tabs.hasSessions).toBe(false);
    expect(bridge.invoke).toHaveBeenCalledWith('rdp_close', { sessionId: 'S1' });
  });

  it('does not fail without a surface', () => {
    const sessions = { state: () => undefined, surface: () => undefined } as unknown as RdpSessionsStore;
    mounted = mount(RdpSessionView, {
      target: document.body,
      props: { sessions, id: 'X', active: false, settings, app: { platform: 'macos' } },
    });
    flushSync();
    expect(document.body.textContent).toBe('');
  });
});
