import { describe, expect, it, vi } from 'vitest';
import { createApi } from '../api';
import { fakeBridge } from '../testing/bridge';
import type { SerialPortDto } from '../types';
import { DEFAULT_SERIAL_SETTINGS, SERIAL_CONSOLE } from './serial';
import { createSerialConsoles } from './serialConsoles.svelte';

const port = (path: string, inUseBy: string | null = null): SerialPortDto => ({
  path,
  label: 'FT232R',
  inUseBy,
});

const opnsense = {
  id: 's1',
  entryId: 'e1',
  name: 'OPNsense',
  type: 'serial' as const,
  color: null,
} as unknown as Parameters<ReturnType<typeof createSerialConsoles>['connect']>[0];

function setup(ports: SerialPortDto[], replies: Record<string, unknown> = {}, confirm = true) {
  let current = ports;
  const bridge = fakeBridge({
    serial_ports: () => current,
    serial_session_settings: DEFAULT_SERIAL_SETTINGS,
    serial_apply_settings: null,
    serial_copy_output: null,
    serial_save_output: true,
    ...replies,
  });
  const sessions = {
    open: vi.fn(async () => 'P1'),
    reconnect: vi.fn(async (): Promise<unknown> => null),
    state: vi.fn((id: string | null) =>
      id === null ? undefined : ({ connectionName: 'OPNsense', connectionId: 's1' } as never),
    ),
  };
  const dialogs = { confirm: vi.fn(async () => confirm) };
  const alerts = { show: vi.fn() };
  const toast = { show: vi.fn() };
  const store = createSerialConsoles({ api: createApi(bridge.invoke), sessions, dialogs, alerts, toast });
  return {
    store,
    sessions,
    dialogs,
    alerts,
    toast,
    bridge,
    setPorts: (next: SerialPortDto[]) => (current = next),
  };
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('serial consoles', () => {
  it('connects a saved connection to the only free device without asking', async () => {
    const { store, sessions, dialogs } = setup([port('/dev/cu.A')]);
    await store.connect(opnsense);
    expect(sessions.open).toHaveBeenCalledWith(opnsense, {
      serial: { port: '/dev/cu.A', settings: null, takeOver: false },
    });
    expect(dialogs.confirm).not.toHaveBeenCalled();
    expect(store.dialog).toBeNull();
  });

  it('asks before taking the only device over from another tab', async () => {
    const yes = setup([port('/dev/cu.A', 'OPNsense A')]);
    await yes.store.connect(opnsense);
    expect(yes.dialogs.confirm).toHaveBeenCalledWith(
      expect.objectContaining({ title: '“OPNsense A” is using this port' }),
    );
    expect(yes.sessions.open).toHaveBeenCalledWith(opnsense, {
      serial: { port: '/dev/cu.A', settings: null, takeOver: true },
    });

    const no = setup([port('/dev/cu.A', 'OPNsense A')], {}, false);
    await no.store.connect(opnsense);
    expect(no.sessions.open).not.toHaveBeenCalled();
  });

  it('shows the picker for none or several devices; Refresh lists them again', async () => {
    const { store, sessions, setPorts } = setup([]);
    const done = store.connect(opnsense);
    await tick();
    expect(store.dialog).toMatchObject({ kind: 'connect', adHoc: false, name: 'OPNsense', ports: [] });
    setPorts([port('/dev/cu.A'), port('/dev/cu.B')]);
    await store.refresh();
    expect(store.dialog).toMatchObject({ ports: [{ path: '/dev/cu.A' }, { path: '/dev/cu.B' }] });
    store.answer({ port: port('/dev/cu.B'), settings: null });
    await done;
    expect(sessions.open).toHaveBeenCalledWith(opnsense, {
      serial: { port: '/dev/cu.B', settings: null, takeOver: false },
    });
  });

  it('opens an ad-hoc console with the typed settings; Cancel opens nothing', async () => {
    const { store, sessions } = setup([port('/dev/cu.A')]);
    const settings = { ...DEFAULT_SERIAL_SETTINGS, baud: 9600 };
    let done = store.openAdHoc();
    await tick();
    expect(store.dialog).toMatchObject({ kind: 'connect', adHoc: true });
    store.answer({ port: port('/dev/cu.A'), settings });
    await done;
    expect(sessions.open).toHaveBeenCalledWith(SERIAL_CONSOLE, {
      serial: { port: '/dev/cu.A', settings, takeOver: false },
    });

    done = store.openAdHoc();
    await tick();
    store.answer(null);
    await done;
    expect(sessions.open).toHaveBeenCalledTimes(1);
  });

  it('reconnect: a missing device shows the picker, a held one asks to take it over', async () => {
    const missing = { code: 'portMissing', message: 'The serial device is not connected.', detail: null };
    const { store, sessions, setPorts } = setup([port('/dev/cu.B')]);
    sessions.reconnect.mockResolvedValueOnce(missing);
    await store.reconnect('P1');
    // The only device (renamed): straight to it.
    expect(sessions.reconnect).toHaveBeenLastCalledWith('P1', { port: '/dev/cu.B', takeOver: false });

    const held = { code: 'portInUse', message: 'Another tab is using the serial port.', detail: 'B' };
    sessions.reconnect.mockResolvedValueOnce(held);
    await store.reconnect('P1');
    expect(sessions.reconnect).toHaveBeenLastCalledWith('P1', { port: null, takeOver: true });

    setPorts([]);
    sessions.reconnect.mockResolvedValueOnce(missing);
    const done = store.reconnect('P1');
    await tick();
    await tick();
    expect(store.dialog?.kind).toBe('connect');
    store.answer(null);
    await done;
  });

  it('applies tab settings, optionally saving them; copies and saves the output', async () => {
    const { store, bridge, toast } = setup([]);
    const done = store.settings('P1');
    await tick();
    expect(store.dialog).toMatchObject({
      kind: 'settings',
      canSave: true,
      settings: DEFAULT_SERIAL_SETTINGS,
    });
    const settings = { ...DEFAULT_SERIAL_SETTINGS, baud: 57600 };
    store.answer({ settings, save: true });
    await done;
    expect(bridge.invoke).toHaveBeenCalledWith('serial_apply_settings', {
      sessionId: 'P1',
      settings,
      save: true,
    });

    await store.copyOutput('P1');
    expect(bridge.invoke).toHaveBeenCalledWith('serial_copy_output', { sessionId: 'P1' });
    expect(toast.show).toHaveBeenCalledWith('Output copied');
    await store.saveOutput('P1');
    expect(bridge.invoke).toHaveBeenCalledWith(
      'serial_save_output',
      expect.objectContaining({
        sessionId: 'P1',
        fileName: expect.stringMatching(/^OPNsense-\d{4}-.*\.log$/),
      }),
    );
    expect(toast.show).toHaveBeenCalledWith('Output saved');
  });

  it('shows errors as alerts', async () => {
    const { store, alerts, sessions } = setup([port('/dev/cu.A')]);
    sessions.open.mockRejectedValueOnce({
      code: 'portBusy',
      message: 'Another program is using the serial port.',
      detail: '/dev/cu.A',
    });
    await store.connect(opnsense);
    expect(alerts.show).toHaveBeenCalledWith(
      expect.stringContaining('Another program is using the serial port.'),
    );
  });
});
