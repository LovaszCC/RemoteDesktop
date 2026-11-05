// Connecting serial consoles: which port (the only device right away, else the picker), taking a port
// over from another tab (after asking), the ad-hoc "Serial Console" dialog, Reconnect when the device
// was renamed or is held, the tab's "Settings…", and "Copy output" / "Save to file…".

import { api as defaultApi, isCommandError, type Api } from '../api';
import { errorMessage } from '../errors';
import { t } from '../i18n/index.svelte';
import { alerts as defaultAlerts, type AlertStore } from '../stores/alert.svelte';
import { dialogs as defaultDialogs, type DialogStore } from '../stores/dialogs.svelte';
import { toast as defaultToast, type ToastStore } from '../stores/toast.svelte';
import type { ConnectionDto, SerialPortDto, SerialSettings } from '../types';
import {
  DEFAULT_SERIAL_SETTINGS,
  SERIAL_CONSOLE,
  choosePort,
  isSerialConsole,
  outputFileName,
} from './serial';
import { sessions as defaultSessions, type SessionsStore } from './sessions.svelte';

/** What the connect dialog answers. */
export interface ConnectAnswer {
  port: SerialPortDto;
  /** Ad hoc only: the settings typed. */
  settings: SerialSettings | null;
}

export type SerialDialog =
  | {
      kind: 'connect';
      /** Ad hoc: the settings fields are shown too. */
      adHoc: boolean;
      /** The connection's name, for the title. */
      name: string;
      ports: SerialPortDto[];
      resolve: (answer: ConnectAnswer | null) => void;
    }
  | {
      kind: 'settings';
      sessionId: string;
      settings: SerialSettings;
      /** A saved connection's tab: offer "Save to the connection". */
      canSave: boolean;
      resolve: (answer: { settings: SerialSettings; save: boolean } | null) => void;
    };

export interface SerialConsolesDeps {
  api: Api;
  sessions: Pick<SessionsStore, 'open' | 'reconnect' | 'state'>;
  dialogs: Pick<DialogStore, 'confirm'>;
  alerts: Pick<AlertStore, 'show'>;
  toast: Pick<ToastStore, 'show'>;
}

const code = (error: unknown): string | null => (isCommandError(error) ? error.code : null);

export function createSerialConsoles(deps: Partial<SerialConsolesDeps> = {}) {
  const api = deps.api ?? defaultApi;
  const sessions = deps.sessions ?? defaultSessions;
  const dialogs = deps.dialogs ?? defaultDialogs;
  const alerts = deps.alerts ?? defaultAlerts;
  const toast = deps.toast ?? defaultToast;

  let dialog = $state<SerialDialog | null>(null);

  async function listPorts(): Promise<SerialPortDto[] | null> {
    try {
      return await api.serialPorts();
    } catch (error) {
      alerts.show(errorMessage(error));
      return null;
    }
  }

  /** Asks before taking `holder`'s port. */
  function confirmTakeOver(holder: string): Promise<boolean> {
    return dialogs.confirm({
      title: t('“%@” is using this port', holder),
      message: t('Disconnect it and connect here? Its output stays in its tab.'),
      confirmLabel: t('Disconnect and connect'),
    });
  }

  function ask(request: Omit<Extract<SerialDialog, { kind: 'connect' }>, 'resolve' | 'kind'>) {
    return new Promise<ConnectAnswer | null>((resolve) => {
      dialog = { kind: 'connect', ...request, resolve };
    });
  }

  /** The port to use (and whether to take it over); `null` when cancelled or none could be listed. */
  async function pickPort(name: string): Promise<{ port: string; takeOver: boolean } | null> {
    const ports = await listPorts();
    if (!ports) return null;
    const choice = choosePort(ports);
    if (choice.kind === 'open') return { port: choice.port, takeOver: false };
    if (choice.kind === 'takeOver') {
      return (await confirmTakeOver(choice.holder)) ? { port: choice.port, takeOver: true } : null;
    }
    const answer = await ask({ adHoc: false, name, ports });
    if (!answer) return null;
    if (answer.port.inUseBy === null) return { port: answer.port.path, takeOver: false };
    return (await confirmTakeOver(answer.port.inUseBy)) ? { port: answer.port.path, takeOver: true } : null;
  }

  async function open(
    connection: Parameters<SessionsStore['open']>[0],
    port: string,
    settings: SerialSettings | null,
    takeOver: boolean,
  ) {
    try {
      await sessions.open(connection, { serial: { port, settings, takeOver } });
    } catch (error) {
      alerts.show(errorMessage(error));
    }
  }

  const store = {
    /** The open connect or settings dialog. */
    get dialog(): SerialDialog | null {
      return dialog;
    },
    /** Answers the open dialog (`null`: cancelled). */
    answer(value: unknown) {
      const open = dialog;
      dialog = null;
      if (open?.kind === 'connect') open.resolve(value as ConnectAnswer | null);
      else if (open?.kind === 'settings') {
        open.resolve(value as { settings: SerialSettings; save: boolean } | null);
      }
    },
    /** The ports again, for the picker's Refresh. */
    async refresh() {
      if (dialog?.kind !== 'connect') return;
      const ports = await listPorts();
      if (ports && dialog?.kind === 'connect') dialog = { ...dialog, ports };
    },

    /** Connect on a saved serial connection: its settings, the port picked now. */
    async connect(connection: ConnectionDto) {
      const choice = await pickPort(connection.name);
      if (choice) await open(connection, choice.port, null, choice.takeOver);
    },

    /** The "Serial Console" row: port and settings typed in the dialog, nothing saved. */
    async openAdHoc() {
      const ports = await listPorts();
      if (!ports) return;
      const answer = await ask({ adHoc: true, name: t('Serial Console'), ports });
      if (!answer) return;
      let takeOver = false;
      if (answer.port.inUseBy !== null) {
        if (!(await confirmTakeOver(answer.port.inUseBy))) return;
        takeOver = true;
      }
      await open(SERIAL_CONSOLE, answer.port.path, answer.settings ?? DEFAULT_SERIAL_SETTINGS, takeOver);
    },

    /** Reconnect on a serial tab: the same port; a renamed or missing device → the picker; a held one →
     * asks to take it over. */
    async reconnect(id: string) {
      const name = sessions.state(id)?.connectionName ?? t('Serial Console');
      const error = await sessions.reconnect(id);
      if (code(error) === 'portInUse') {
        const holder = isCommandError(error) ? (error.detail ?? '') : '';
        if (await confirmTakeOver(holder)) await sessions.reconnect(id, { port: null, takeOver: true });
      } else if (code(error) === 'portMissing') {
        const choice = await pickPort(name);
        if (choice) await sessions.reconnect(id, choice);
      }
    },

    /** "Settings…": the tab's settings, applied to the open port; optionally saved to the connection. */
    async settings(id: string) {
      const state = sessions.state(id);
      if (!state) return;
      let current: SerialSettings;
      try {
        current = await api.serialSessionSettings(id);
      } catch (error) {
        alerts.show(errorMessage(error));
        return;
      }
      const answer = await new Promise<{ settings: SerialSettings; save: boolean } | null>((resolve) => {
        dialog = {
          kind: 'settings',
          sessionId: id,
          settings: current,
          canSave: !isSerialConsole(state.connectionId),
          resolve,
        };
      });
      if (!answer) return;
      try {
        await api.serialApplySettings(id, answer.settings, answer.save);
      } catch (error) {
        alerts.show(errorMessage(error));
      }
    },

    /** "Copy output": the whole output so far, as plain text. */
    async copyOutput(id: string) {
      try {
        await api.serialCopyOutput(id);
        toast.show(t('Output copied'));
      } catch (error) {
        alerts.show(errorMessage(error));
      }
    },

    /** "Save to file…": the whole output so far, as plain text. */
    async saveOutput(id: string) {
      const name = sessions.state(id)?.connectionName ?? 'serial';
      try {
        if (await api.serialSaveOutput(id, outputFileName(name))) toast.show(t('Output saved'));
      } catch (error) {
        alerts.show(errorMessage(error));
      }
    },
  };
  return store;
}

export type SerialConsolesStore = ReturnType<typeof createSerialConsoles>;

/** The app-wide instance. */
export const serialConsoles = createSerialConsoles();
