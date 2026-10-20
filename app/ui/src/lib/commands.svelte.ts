// What each app command does (Swift menu actions and `AppDelegate`'s key monitor), and the listeners that
// feed the dispatcher: the window `keydown` and the native menu's `menu-command` event.

import { api as defaultApi, onMenuCommand, tauriBridge, type Listen, type Unlisten } from './api';
import { errorMessage } from './errors';
import { importer as defaultImporter, type ImportStore } from './importer/importer.svelte';
import { csvImporter as defaultCsvImporter, type CsvImportStore } from './transfer/csvImporter.svelte';
import { exporter as defaultExporter, type ExportStore } from './transfer/exporter.svelte';
import {
  transferImporter as defaultTransferImporter,
  type TransferImportStore,
} from './transfer/transferImporter.svelte';
import { createDispatcher, type AppCommand, type Dispatcher } from './shortcuts/dispatcher';
import { recorder as defaultRecorder, type RecorderStore } from './shortcuts/recorder.svelte';
import { sidebarActions as defaultActions, type SidebarActions } from './sidebar/actions.svelte';
import { alerts as defaultAlerts, type AlertStore } from './stores/alert.svelte';
import { app as defaultApp, type AppStore } from './stores/app.svelte';
import { library as defaultLibrary, parseTag, type LibraryStore } from './stores/library.svelte';
import { settings as defaultSettings, type SettingsStore } from './stores/settings.svelte';
import {
  sidebarLayout as defaultSidebarLayout,
  type SidebarLayoutStore,
} from './stores/sidebarLayout.svelte';
import {
  settingsWindow as defaultSettingsWindow,
  type SettingsWindowStore,
} from './stores/settingsWindow.svelte';
import { rdpSessions as defaultRdpSessions, type RdpSessionsStore } from './rdp/sessions.svelte';
import { openTestTab, tabs as defaultTabs, type TabsStore } from './tabs.svelte';
import { sessions as defaultSessions, type SessionsStore } from './terminal/sessions.svelte';
import { isLocalTerminal } from './terminal/local';
import { splits as defaultSplits, type SplitsStore } from './terminal/splits.svelte';
import { updates as defaultUpdates, type UpdateStore } from './updates/updates.svelte';
import { sshConnections } from './sidebar/model';
import type { ConnectionType } from './types';

export interface CommandDeps {
  app: Pick<AppStore, 'isUnlocked' | 'readOnly' | 'lock'>;
  library: Pick<LibraryStore, 'primary' | 'selection' | 'library'>;
  tabs: TabsStore;
  actions: Pick<
    SidebarActions,
    | 'newConnection'
    | 'newFolder'
    | 'newCredential'
    | 'connect'
    | 'primaryAction'
    | 'openSplit'
    | 'openFiles'
    | 'openLocalTerminal'
  >;
  alerts: Pick<AlertStore, 'show'>;
  settingsWindow: Pick<SettingsWindowStore, 'open'>;
  importer: Pick<ImportStore, 'start'>;
  exporter: Pick<ExportStore, 'open'>;
  transferImporter: Pick<TransferImportStore, 'start'>;
  csvImporter: Pick<CsvImportStore, 'start'>;
  sessions: Pick<SessionsStore, 'active' | 'clear' | 'toggleRecording' | 'reconnect'>;
  splits: Pick<SplitsStore, 'splitActive' | 'toggleBroadcastActive'>;
  rdpSessions: Pick<RdpSessionsStore, 'active' | 'reconnect' | 'typeClipboard'>;
  updates: Pick<UpdateStore, 'checkManually'>;
  sidebarLayout: Pick<SidebarLayoutStore, 'toggle'>;
  dev: boolean;
}

const NEW_CONNECTION: Partial<Record<AppCommand, ConnectionType>> = {
  newRDP: 'rdp',
  newSSH: 'ssh',
  newWeb: 'web',
  newMikrotik: 'mikrotik',
};

/**
 * What "Files (SCP)…" applies to (Swift `canOpenFiles` / `openFilesForActiveTab`): the active terminal
 * (a split's focused pane) with its session, unless it is a local terminal, else on the Details tab one
 * selected SSH/MikroTik connection; `null` otherwise.
 */
export function filesTarget(
  deps: Pick<CommandDeps, 'library' | 'tabs' | 'sessions'>,
): { connectionId: string; sessionId?: string } | null {
  const session = deps.sessions.active;
  if (session) {
    return isLocalTerminal(session.connectionId)
      ? null
      : { connectionId: session.connectionId, sessionId: session.id };
  }
  if (deps.tabs.active !== null || deps.library.selection.length !== 1 || !deps.library.primary) return null;
  if (!deps.library.library) return null;
  // An entry: its default connection when it uses SSH, else its first that does.
  const connection = sshConnections(deps.library.library, [deps.library.primary])[0];
  return connection ? { connectionId: connection.id } : null;
}

/** Runs app commands; anything unavailable in the current state does nothing. */
export function createCommandRunner(deps: Partial<CommandDeps> = {}) {
  const app = deps.app ?? defaultApp;
  const library = deps.library ?? defaultLibrary;
  const tabs = deps.tabs ?? defaultTabs;
  const actions = deps.actions ?? defaultActions;
  const alerts = deps.alerts ?? defaultAlerts;
  const settingsWindow = deps.settingsWindow ?? defaultSettingsWindow;
  const sidebarLayout = deps.sidebarLayout ?? defaultSidebarLayout;
  const importer = deps.importer ?? defaultImporter;
  const exporter = deps.exporter ?? defaultExporter;
  const transferImporter = deps.transferImporter ?? defaultTransferImporter;
  const csvImporter = deps.csvImporter ?? defaultCsvImporter;
  const sessions = deps.sessions ?? defaultSessions;
  const splits = deps.splits ?? defaultSplits;
  const rdpSessions = deps.rdpSessions ?? defaultRdpSessions;
  const updates = deps.updates ?? defaultUpdates;
  const dev = deps.dev ?? import.meta.env.DEV;

  const canWrite = () => app.isUnlocked && !app.readOnly;

  return function run(command: AppCommand) {
    const type = NEW_CONNECTION[command];
    if (type) {
      if (canWrite()) actions.newConnection(type);
      return;
    }
    switch (command) {
      case 'newFolder':
        if (canWrite()) actions.newFolder();
        return;
      case 'newCredential':
        if (canWrite()) actions.newCredential();
        return;
      case 'importRDM':
        if (canWrite()) void importer.start('rdm');
        return;
      case 'importWinbox':
        if (canWrite()) void importer.start('winbox');
        return;
      case 'importTransfer':
        if (canWrite()) void transferImporter.start();
        return;
      case 'importCsv':
        if (canWrite()) csvImporter.start();
        return;
      case 'exportLibrary':
        if (app.isUnlocked) exporter.open(library.library, library.selection);
        return;
      case 'connect': {
        if (!app.isUnlocked) return;
        // Several rows: their SSH connections side by side (Swift "Connect" with a multi-selection).
        if (library.selection.length > 1) {
          const connections = sshConnections(library.library, library.selection);
          if (connections.length > 1) actions.openSplit(connections);
          return;
        }
        if (!library.primary) return;
        const parsed = parseTag(library.primary);
        if (
          parsed?.kind === 'entry' ||
          parsed?.kind === 'connection' ||
          parsed?.kind === 'favorite' ||
          parsed?.kind === 'split'
        ) {
          actions.primaryAction(library.primary);
        }
        return;
      }
      case 'newLocalTerminal':
        if (app.isUnlocked) void actions.openLocalTerminal();
        return;
      case 'closeTab':
        tabs.closeActive();
        return;
      case 'nextTab':
      case 'previousTab':
        if (tabs.hasSessions) tabs.cycle(command === 'nextTab');
        return;
      case 'clearTerminal': {
        const session = sessions.active;
        if (session) void sessions.clear(session.id);
        return;
      }
      case 'recordOutput': {
        const session = sessions.active;
        if (session?.status.kind === 'running') void sessions.toggleRecording(session.id);
        return;
      }
      case 'reconnect': {
        // As in Swift, the exit bar's Reconnect button: only while the session has exited.
        const session = sessions.active;
        if (session?.status.kind === 'exited') void sessions.reconnect(session.id);
        const rdp = rdpSessions.active;
        if (rdp?.status.kind === 'disconnected') void rdpSessions.reconnect(rdp.id);
        return;
      }
      case 'typeClipboard': {
        // Swift `typeClipboardIntoActiveSession`: the active RDP tab, while connected.
        const rdp = rdpSessions.active;
        if (rdp?.status.kind === 'connected') void rdpSessions.typeClipboard(rdp.id);
        return;
      }
      case 'splitTerminal':
        void splits.splitActive();
        return;
      case 'broadcastAll':
        splits.toggleBroadcastActive();
        return;
      case 'lock':
        if (app.isUnlocked) app.lock().catch((error: unknown) => alerts.show(errorMessage(error)));
        return;
      case 'openSettings':
        settingsWindow.open();
        return;
      case 'toggleSidebar':
        sidebarLayout.toggle();
        return;
      case 'checkForUpdates':
        void updates.checkManually();
        return;
      case 'devOpenTestTab':
        if (dev) openTestTab(tabs);
        return;
      case 'openFiles': {
        if (!app.isUnlocked) return;
        const target = filesTarget({ library, tabs, sessions });
        if (target) void actions.openFiles(target.connectionId, { sessionId: target.sessionId });
        return;
      }
      default:
        return;
    }
  };
}

export interface InstallOptions {
  settings?: SettingsStore;
  app?: Pick<AppStore, 'platform'>;
  recorder?: RecorderStore;
  run?: (command: AppCommand) => void;
  listen?: Listen;
  target?: Window;
  dev?: boolean;
  /** Whether "Files (SCP)…" applies now (default: the app-wide stores). */
  canOpenFiles?: () => boolean;
  /** Tells Rust, which enables the menu item (default: `menu_files_target`). */
  syncFilesMenu?: (enabled: boolean) => void;
}

/** Wires the dispatcher to the window and the menu; returns the cleanup. */
export function installCommands(options: InstallOptions = {}): { dispatcher: Dispatcher; stop: () => void } {
  const settings = options.settings ?? defaultSettings;
  const appStore = options.app ?? defaultApp;
  const recorder = options.recorder ?? defaultRecorder;
  const target = options.target ?? window;
  const dispatcher = createDispatcher({
    shortcuts: () => settings.shortcuts,
    platform: () => appStore.platform,
    isRecording: () => recorder.active,
    run: options.run ?? createCommandRunner(),
    dev: options.dev ?? import.meta.env.DEV,
  });
  const canOpenFiles =
    options.canOpenFiles ??
    (() =>
      defaultApp.isUnlocked &&
      filesTarget({ library: defaultLibrary, tabs: defaultTabs, sessions: defaultSessions }) !== null);
  const syncFilesMenu =
    options.syncFilesMenu ?? ((enabled: boolean) => void defaultApi.menuFilesTarget(enabled).catch(() => {}));
  const stopMenuSync = $effect.root(() => {
    $effect(() => syncFilesMenu(canOpenFiles()));
  });
  const keydown = (event: KeyboardEvent) => void dispatcher.keydown(event);
  target.addEventListener('keydown', keydown);
  let unlisten: Unlisten | undefined;
  let stopped = false;
  void onMenuCommand(options.listen ?? tauriBridge.listen, ({ command }) => dispatcher.menuCommand(command))
    .then((off) => {
      if (stopped) off();
      else unlisten = off;
    })
    .catch(() => {});
  return {
    dispatcher,
    stop() {
      stopped = true;
      stopMenuSync();
      target.removeEventListener('keydown', keydown);
      unlisten?.();
    },
  };
}
