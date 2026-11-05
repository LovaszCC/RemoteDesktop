// The serial type across the sidebar, editor and detail models, the connect route and the fixed row.
import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { connectionFields, connectionSubtitle } from '../detail/model';
import {
  DEFAULT_PORTS,
  buildConnectionSave,
  connectionFormFrom,
  invalidBaudKey,
  invalidPortKey,
  needsHost,
  newConnectionForm,
  newEntryForm,
} from '../editors/model';
import { connection } from '../testing/fixtures';
import type { ConnectionDto } from '../types';
import { createSidebarActions } from './actions.svelte';
import { connectRoute } from './connect';
import { CONNECTION_TYPES, NEW_TITLES, connectionTooltip, typeName, usesSSH } from './model';
import { serialConsoleMenu } from './menus';
import SerialConsoleRow from './SerialConsoleRow.svelte';

const serial = (overrides: Partial<ConnectionDto> = {}): ConnectionDto => ({
  ...connection('s1', 'OPNsense'),
  type: 'serial',
  host: '',
  serialBaud: 9600,
  serialParity: 'even',
  serialStopBits: '2',
  serialFlowControl: 'hardware',
  ...overrides,
});

let mounted: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
});

describe('the serial connection type', () => {
  it('is a type of its own: no SSH, no port, its own titles', () => {
    expect(CONNECTION_TYPES).toContain('serial');
    expect(NEW_TITLES.serial).toBe('New serial connection');
    expect(typeName('serial')).toBe('Serial');
    expect(usesSSH('serial')).toBe(false);
    expect(DEFAULT_PORTS.serial).toBe(0);
    expect(connectRoute(serial())).toBe('serial');
    expect(connectionTooltip(serial())).toBe('Serial · 9600 8E2 RTS/CTS');
  });

  it('details show the settings and that the port is chosen when connecting', () => {
    expect(connectionSubtitle(serial(), 'macos')).toBe('Serial · 9600 8E2 RTS/CTS');
    const { before, after } = connectionFields(serial());
    expect(before.map((f) => [f.label, f.value])).toEqual([
      ['Baud rate', '9600'],
      ['Data bits', '8'],
      ['Parity', 'Even'],
      ['Stop bits', '2'],
      ['Flow control', 'RTS/CTS'],
      ['Port', 'Chosen when connecting'],
    ]);
    expect(after.map((f) => f.label)).toEqual(['Folder']);
  });

  it('the editor saves the settings without port, login or credential; host is optional', () => {
    const form = { ...connectionFormFrom(serial({ username: 'x', credentialId: 'K' })), port: '22' };
    expect(form.serialBaud).toBe('9600');
    const save = buildConnectionSave({ ...form, serialBaud: ' 57600 ' }, true);
    expect(save.draft).toMatchObject({
      type: 'serial',
      port: '',
      username: '',
      credentialId: null,
      serialBaud: 57600,
      serialDataBits: 8,
      serialParity: 'even',
      serialStopBits: '2',
      serialFlowControl: 'hardware',
    });
    expect(save.password).toBeNull();
    expect(buildConnectionSave(newConnectionForm('ssh'), false).draft.serialBaud).toBeUndefined();

    const entry = newEntryForm('serial', null);
    expect(entry.connections[0]!.serialBaud).toBe('115200');
    expect(needsHost(entry)).toBe(false);
    expect(needsHost({ connections: [...entry.connections, newConnectionForm('ssh')] })).toBe(true);
    expect(invalidPortKey({ ...entry, connections: [{ ...entry.connections[0]!, port: 'x' }] })).toBeNull();
    expect(invalidBaudKey(entry)).toBeNull();
    const bad = { ...entry, connections: [{ ...entry.connections[0]!, serialBaud: '0' }] };
    expect(invalidBaudKey(bad)).toBe(entry.connections[0]!.key);
  });

  it('connecting a serial connection and the fixed row go to the serial consoles', async () => {
    const serialConsoles = { connect: vi.fn(async () => {}), openAdHoc: vi.fn(async () => {}) };
    const actions = createSidebarActions({ serialConsoles });
    await actions.connect(serial());
    expect(serialConsoles.connect).toHaveBeenCalledWith(serial());
    await actions.openSerialConsole();
    expect(serialConsoles.openAdHoc).toHaveBeenCalled();

    const openSerialConsole = vi.fn(async () => {});
    const [open] = serialConsoleMenu({ openSerialConsole });
    expect(open).toMatchObject({ label: 'Open' });
    mounted = mount(SerialConsoleRow, {
      target: document.body,
      props: { actions: { openSerialConsole }, sessionOpen: true },
    });
    flushSync();
    const row = document.querySelector<HTMLElement>('[data-serial-console]')!;
    expect(row.textContent).toContain('Serial Console');
    expect(document.querySelector('.session-dot')).not.toBeNull();
    row.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(openSerialConsole).toHaveBeenCalledTimes(2);
  });
});
