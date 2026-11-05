import { describe, expect, it } from 'vitest';
import type { SerialPortDto } from '../types';
import { exitText } from './model';
import {
  DEFAULT_SERIAL_SETTINGS,
  choosePort,
  isSerialConsole,
  outputFileName,
  serialConsoleName,
  serialForm,
  serialSummary,
  settingsFromForm,
  validSerialSettings,
} from './serial';

const port = (path: string, inUseBy: string | null = null): SerialPortDto => ({
  path,
  label: 'FT232R',
  inUseBy,
});

describe('serial helpers', () => {
  it('connects to the only free device, asks for a held one, else picks', () => {
    expect(choosePort([port('/dev/cu.A')])).toEqual({ kind: 'open', port: '/dev/cu.A' });
    expect(choosePort([port('/dev/cu.A', 'OPNsense A')])).toEqual({
      kind: 'takeOver',
      port: '/dev/cu.A',
      holder: 'OPNsense A',
    });
    expect(choosePort([])).toEqual({ kind: 'pick' });
    expect(choosePort([port('/dev/cu.A'), port('/dev/cu.B')])).toEqual({ kind: 'pick' });
  });

  it('summarises settings like 115200 8N1', () => {
    expect(serialSummary(DEFAULT_SERIAL_SETTINGS)).toBe('115200 8N1');
    expect(
      serialSummary({ baud: 9600, dataBits: 7, parity: 'even', stopBits: '2', flowControl: 'hardware' }),
    ).toBe('9600 7E2 RTS/CTS');
    expect(serialSummary({ ...DEFAULT_SERIAL_SETTINGS, parity: 'odd', flowControl: 'software' })).toBe(
      '115200 8O1 XON/XOFF',
    );
  });

  it('reads a typed baud rate; rejects zero, decimals and text', () => {
    const form = serialForm(DEFAULT_SERIAL_SETTINGS);
    expect(form.baud).toBe('115200');
    expect(settingsFromForm({ ...form, baud: ' 250000 ' })?.baud).toBe(250000);
    for (const bad of ['0', '96.5', 'fast', '', '-9600']) {
      expect(settingsFromForm({ ...form, baud: bad }), bad).toBeNull();
    }
    expect(validSerialSettings({ ...DEFAULT_SERIAL_SETTINGS, dataBits: 9 })).toBe(false);
  });

  it('numbers ad-hoc consoles and names output files', () => {
    expect(serialConsoleName([])).toBe('Serial Console');
    expect(serialConsoleName(['Serial Console'])).toBe('Serial Console 2');
    expect(isSerialConsole('serial-console')).toBe(true);
    expect(isSerialConsole('C1')).toBe(false);
    const at = new Date(2026, 10, 5, 9, 7);
    expect(outputFileName('OPNsense', at)).toBe('OPNsense-2026-11-05-0907.log');
    expect(outputFileName('a/b: c', at)).toBe('a-b- c-2026-11-05-0907.log');
    expect(outputFileName('  ', at)).toBe('serial-2026-11-05-0907.log');
  });

  it('says "Connection lost" for a serial console that exited', () => {
    const status = { kind: 'exited' as const, code: null, authFailed: false, error: null };
    expect(exitText(status, true)).toBe('Connection lost');
    expect(exitText(status)).toBe('Connection closed');
    expect(exitText({ ...status, error: 'The serial device is not connected.' }, true)).toBe(
      'The serial device is not connected.',
    );
  });
});
