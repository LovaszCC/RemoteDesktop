// Serial consoles: the pseudo connection of the sidebar's fixed "Serial Console" row (ad hoc), the
// settings defaults and summary, the tab names, and which port to connect to.

import { t } from '../i18n/index.svelte';
import type {
  ConnectionDto,
  SerialFlowControl,
  SerialParity,
  SerialPortDto,
  SerialSettings,
  SerialStopBits,
} from '../types';

/** The connection id of ad-hoc serial sessions (never a library id: those are UUIDs). */
export const SERIAL_CONSOLE_ID = 'serial-console';

export function isSerialConsole(connectionId: string): boolean {
  return connectionId === SERIAL_CONSOLE_ID;
}

/** What opens an ad-hoc serial console as a session (the name is chosen when it opens). */
export const SERIAL_CONSOLE: Pick<ConnectionDto, 'id' | 'name' | 'type' | 'color'> = {
  id: SERIAL_CONSOLE_ID,
  name: 'Serial Console',
  type: 'serial',
  color: null,
};

/** 115200 8N1, no flow control. */
export const DEFAULT_SERIAL_SETTINGS: SerialSettings = {
  baud: 115200,
  dataBits: 8,
  parity: 'none',
  stopBits: '1',
  flowControl: 'none',
};

/** The baud rates the pickers offer (any other positive number can be typed). */
export const BAUD_RATES: readonly number[] = [
  1200, 2400, 4800, 9600, 19200, 38400, 57600, 115200, 230400, 460800, 921600,
];
export const DATA_BITS: readonly number[] = [5, 6, 7, 8];
export const PARITIES: readonly SerialParity[] = ['none', 'odd', 'even'];
export const STOP_BITS: readonly SerialStopBits[] = ['1', '2'];
export const FLOW_CONTROLS: readonly SerialFlowControl[] = ['none', 'software', 'hardware'];

/** A saved serial connection's settings. */
export function connectionSerialSettings(
  connection: Pick<
    ConnectionDto,
    'serialBaud' | 'serialDataBits' | 'serialParity' | 'serialStopBits' | 'serialFlowControl'
  >,
): SerialSettings {
  return {
    baud: connection.serialBaud,
    dataBits: connection.serialDataBits,
    parity: connection.serialParity,
    stopBits: connection.serialStopBits,
    flowControl: connection.serialFlowControl,
  };
}

/** `115200 8N1`, plus ` RTS/CTS` or ` XON/XOFF`. */
export function serialSummary(settings: SerialSettings): string {
  const parity = { none: 'N', odd: 'O', even: 'E' }[settings.parity];
  const flow = { none: '', software: ' XON/XOFF', hardware: ' RTS/CTS' }[settings.flowControl];
  return `${settings.baud} ${settings.dataBits}${parity}${settings.stopBits}${flow}`;
}

export function parityName(parity: SerialParity): string {
  return { none: t('None'), odd: t('Odd'), even: t('Even') }[parity];
}

export function flowControlName(flow: SerialFlowControl): string {
  return { none: t('None'), software: 'XON/XOFF', hardware: 'RTS/CTS' }[flow];
}

/** Usable settings: a positive whole baud rate and 5–8 data bits. */
export function validSerialSettings(settings: SerialSettings): boolean {
  return (
    Number.isInteger(settings.baud) &&
    settings.baud > 0 &&
    Number.isInteger(settings.dataBits) &&
    settings.dataBits >= 5 &&
    settings.dataBits <= 8
  );
}

/** The tab title of a new ad-hoc console: "Serial Console", then "Serial Console 2", 3… (lowest free). */
export function serialConsoleName(taken: readonly string[]): string {
  const base = t('Serial Console');
  for (let n = 1; ; n++) {
    const name = n === 1 ? base : `${base} ${n}`;
    if (!taken.includes(name)) return name;
  }
}

/** What connecting does with the ports plugged in now. */
export type PortChoice =
  /** The only device, free: connect right away. */
  | { kind: 'open'; port: string }
  /** The only device is held by another tab: ask before taking it over. */
  | { kind: 'takeOver'; port: string; holder: string }
  /** None or several: show the picker. */
  | { kind: 'pick' };

export function choosePort(ports: readonly SerialPortDto[]): PortChoice {
  const only = ports.length === 1 ? ports[0] : undefined;
  if (!only) return { kind: 'pick' };
  return only.inUseBy === null
    ? { kind: 'open', port: only.path }
    : { kind: 'takeOver', port: only.path, holder: only.inUseBy };
}

/** `<name>-YYYY-MM-DD-HHMM.log`, with characters file systems refuse replaced. */
export function outputFileName(name: string, now: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  const stamp = `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}`;
  const safe = name.replace(/[\\/:*?"<>|]+/g, '-').trim() || 'serial';
  return `${safe}-${stamp}.log`;
}

/** The settings fields as edited: the baud rate as typed. */
export interface SerialForm {
  baud: string;
  dataBits: number;
  parity: SerialParity;
  stopBits: SerialStopBits;
  flowControl: SerialFlowControl;
}

export function serialForm(settings: SerialSettings): SerialForm {
  return { ...settings, baud: String(settings.baud) };
}

/** The settings of a form, or `null` while the baud rate is not a positive whole number. */
export function settingsFromForm(form: SerialForm): SerialSettings | null {
  const text = form.baud.trim();
  if (!/^\d+$/.test(text)) return null;
  const settings: SerialSettings = { ...form, baud: Number(text) };
  return validSerialSettings(settings) ? settings : null;
}
