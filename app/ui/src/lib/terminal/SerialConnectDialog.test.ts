import { flushSync, mount, unmount } from 'svelte';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { buttonByText, click, type } from '../testing/views';
import SerialConnectDialog from './SerialConnectDialog.svelte';
import SerialSettingsDialog from './SerialSettingsDialog.svelte';
import { DEFAULT_SERIAL_SETTINGS } from './serial';

let mounted: ReturnType<typeof mount> | undefined;
afterEach(() => {
  if (mounted) unmount(mounted);
  mounted = undefined;
  document.body.innerHTML = '';
});

const ports = [
  { path: '/dev/cu.A', label: 'FT232R', inUseBy: 'OPNsense A' },
  { path: '/dev/cu.B', label: 'CP2102', inUseBy: null },
];

describe('SerialConnectDialog', () => {
  it('lists ports, preselects the first free one and marks held ones', () => {
    const onanswer = vi.fn();
    mounted = mount(SerialConnectDialog, {
      target: document.body,
      props: { adHoc: false, name: 'OPNsense', ports, onrefresh: vi.fn(), onanswer },
    });
    flushSync();
    expect(document.body.textContent).toContain('Connect “OPNsense”');
    expect(document.body.textContent).toContain('in use: OPNsense A');
    const radios = [...document.querySelectorAll<HTMLInputElement>('input[type="radio"]')];
    expect(radios.map((r) => r.checked)).toEqual([false, true]);
    expect(document.querySelector('#serial-adhoc-baud')).toBeNull();
    click(buttonByText('Connect'));
    expect(onanswer).toHaveBeenCalledWith({ port: ports[1], settings: null });
  });

  it('an ad-hoc console also asks for the settings; a bad baud rate blocks Connect', () => {
    const onanswer = vi.fn();
    const onrefresh = vi.fn();
    mounted = mount(SerialConnectDialog, {
      target: document.body,
      props: { adHoc: true, name: 'Serial Console', ports: [ports[1]!], onrefresh, onanswer },
    });
    flushSync();
    const baud = document.querySelector<HTMLInputElement>('#serial-adhoc-baud')!;
    expect(baud.value).toBe('115200');
    type(baud, '0');
    expect(buttonByText('Connect').disabled).toBe(true);
    type(baud, '9600');
    click(buttonByText('Connect'));
    expect(onanswer).toHaveBeenCalledWith({
      port: ports[1],
      settings: { ...DEFAULT_SERIAL_SETTINGS, baud: 9600 },
    });
    click(document.querySelector('button[title="Refresh"]')!);
    expect(onrefresh).toHaveBeenCalled();
  });

  it('says when no device is plugged in', () => {
    mounted = mount(SerialConnectDialog, {
      target: document.body,
      props: { adHoc: false, name: 'X', ports: [], onrefresh: vi.fn(), onanswer: vi.fn() },
    });
    flushSync();
    expect(document.body.textContent).toContain('No USB serial device found.');
    expect(buttonByText('Connect').disabled).toBe(true);
  });
});

describe('SerialSettingsDialog', () => {
  it('applies the edited settings, saving them only when ticked', () => {
    const onanswer = vi.fn();
    mounted = mount(SerialSettingsDialog, {
      target: document.body,
      props: { settings: DEFAULT_SERIAL_SETTINGS, canSave: true, onanswer },
    });
    flushSync();
    type(document.querySelector<HTMLInputElement>('#serial-tab-baud')!, '57600');
    const save = document.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    save.click();
    flushSync();
    click(buttonByText('Apply'));
    expect(onanswer).toHaveBeenCalledWith({
      settings: { ...DEFAULT_SERIAL_SETTINGS, baud: 57600 },
      save: true,
    });
  });
});
