import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { createApi } from '../api';
import { createAlertStore } from '../stores/alert.svelte';
import { createSettingsStore } from '../stores/settings.svelte';
import { createToastStore } from '../stores/toast.svelte';
import { TabsStore } from '../tabs.svelte';
import { fakeBridge } from '../testing/bridge';
import type { SshEvent, SshEventDto } from '../types';
import { createSessionsStore } from './sessions.svelte';
import TerminalSessionView from './TerminalSessionView.svelte';
import type { TerminalHandle } from './xterm';

let mounted: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
});

// jsdom has no ResizeObserver.
globalThis.ResizeObserver ??= class {
  observe() {}
  disconnect() {}
  unobserve() {}
} as unknown as typeof ResizeObserver;

async function render() {
  const bridge = fakeBridge({
    open_ssh_session: { sessionId: 'S1' },
    ssh_record_next_output: 'armed',
    ssh_record_cancel: null,
    ssh_reconnect: null,
    ssh_close: null,
  });
  const handle: TerminalHandle = {
    write: vi.fn(),
    clearLocal: vi.fn(),
    attach: vi.fn((el: HTMLElement) => el.appendChild(document.createElement('canvas'))),
    detach: vi.fn(),
    fit: vi.fn(),
    size: () => ({ cols: 80, rows: 24 }),
    focus: vi.fn(),
    dispose: vi.fn(),
  };
  const tabs = new TabsStore();
  const sessions = createSessionsStore({
    api: createApi(bridge.invoke),
    listen: bridge.listen,
    tabs,
    toast: createToastStore(),
    alerts: createAlertStore(),
    createTerminal: () => handle,
    createChannel: () => ({ onmessage: () => {} }),
  });
  await sessions.open({ id: 'c1', name: 'web01', type: 'ssh', color: null });
  const settings = createSettingsStore({ ...bridge, onLanguage: () => {} });
  mounted = mount(TerminalSessionView, {
    target: document.body,
    props: { sessions, id: 'S1', active: true, settings },
  });
  flushSync();
  const emit = (e: SshEvent) => {
    sessions.handleEvent({ sessionId: 'S1', ...e } as SshEventDto);
    flushSync();
  };
  return { bridge, handle, sessions, tabs, emit };
}

const button = (label: string) =>
  [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === label);
const flush = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('TerminalSessionView', () => {
  it('attaches the terminal and detaches it without ending the session', async () => {
    const { handle } = await render();
    expect(handle.attach).toHaveBeenCalled();
    expect(document.querySelector('canvas')).not.toBeNull();
    unmount(mounted!);
    mounted = undefined;
    expect(handle.detach).toHaveBeenCalled();
    expect(handle.dispose).not.toHaveBeenCalled();
  });

  it('records the next output with status and Cancel', async () => {
    const { bridge, emit } = await render();
    button('Copy next output')!.click();
    await flush();
    flushSync();
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_record_next_output', { sessionId: 'S1' });
    expect(document.body.textContent).toContain('● Waiting for a command… (Esc: cancel)');
    expect(button('Stop & copy')).toBeDefined();
    emit({ kind: 'recording', phase: 'capturing' });
    expect(document.body.textContent).toContain('● Recording…');
    button('Cancel')!.click();
    await flush();
    flushSync();
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_record_cancel', { sessionId: 'S1' });
    expect(button('Copy next output')).toBeDefined();
  });

  it('shows the root warning', async () => {
    const { emit } = await render();
    expect(document.body.textContent).not.toContain('Root:');
    emit({ kind: 'root', root: true });
    expect(document.body.textContent).toContain('Root: commands run with full privileges');
  });

  it('shows the exit bar with Reconnect and Close', async () => {
    const { bridge, emit, tabs } = await render();
    expect(document.querySelector('[role="alert"]')).toBeNull();
    emit({ kind: 'exit', code: 130, authFailed: false });
    expect(document.querySelector('[role="alert"]')?.textContent).toContain('Connection closed (code: 130)');
    expect(button('Copy next output')?.disabled).toBe(true);
    button('Reconnect')!.click();
    await flush();
    flushSync();
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_reconnect', { sessionId: 'S1', cols: 80, rows: 24 });
    expect(document.querySelector('[role="alert"]')).toBeNull();

    emit({ kind: 'exit', code: 255, authFailed: true });
    expect(document.querySelector('[role="alert"]')?.textContent).toContain('Authentication failed');
    button('Close')!.click();
    expect(tabs.hasSessions).toBe(false);
    expect(bridge.invoke).toHaveBeenCalledWith('ssh_close', { sessionId: 'S1' });
  });
});
