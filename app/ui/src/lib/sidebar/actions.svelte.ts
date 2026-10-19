// What the sidebar rows, its context menus and the toolbar "New" menu do (Swift `AppState`: move,
// delete, duplicate, link, saved splits, copy). Errors show the alert, as Swift's `perform` does.

import { api as defaultApi, isCommandError, type Api } from '../api';
import { errorMessage } from '../errors';
import { t } from '../i18n/index.svelte';
import { alerts as defaultAlerts, type AlertStore } from '../stores/alert.svelte';
import { dialogs as defaultDialogs, type DialogStore } from '../stores/dialogs.svelte';
import { editor as defaultEditor, type EditorStore } from '../stores/editor.svelte';
import { library as defaultLibrary, parseTag, type LibraryStore } from '../stores/library.svelte';
import { settings as defaultSettings, type SettingsStore } from '../stores/settings.svelte';
import { toast as defaultToast, type ToastStore } from '../stores/toast.svelte';
import { rdpSessions as defaultRdpSessions, type RdpSessionsStore } from '../rdp/sessions.svelte';
import { sessions as defaultSessions, type SessionsStore } from '../terminal/sessions.svelte';
import { MAX_PANES } from '../terminal/split';
import { splits as defaultSplits, type SplitsStore } from '../terminal/splits.svelte';
import type { ConnectionDto, ConnectionType, LabelColor, LaunchOutcomeDto } from '../types';
import { connectRoute, type ConnectOptions } from './connect';
import {
  bulkDeleteMessage,
  bulkDeleteTitle,
  credentialDeleteMessage,
  deletionCounts,
  toggledExpansion,
} from './model';

export { MAX_PANES };

export interface SidebarDeps {
  api: Api;
  library: LibraryStore;
  settings: SettingsStore;
  toast: ToastStore;
  alerts: AlertStore;
  editor: EditorStore;
  dialogs: DialogStore;
  sessions: Pick<SessionsStore, 'open' | 'openLocal'>;
  rdpSessions: Pick<RdpSessionsStore, 'open'>;
  splits: Pick<SplitsStore, 'open' | 'openSaved'>;
}

export function createSidebarActions(deps: Partial<SidebarDeps> = {}) {
  const api = deps.api ?? defaultApi;
  const library = deps.library ?? defaultLibrary;
  const settings = deps.settings ?? defaultSettings;
  const toast = deps.toast ?? defaultToast;
  const alerts = deps.alerts ?? defaultAlerts;
  const editor = deps.editor ?? defaultEditor;
  const dialogs = deps.dialogs ?? defaultDialogs;
  const sessions = deps.sessions ?? defaultSessions;
  const rdpSessions = deps.rdpSessions ?? defaultRdpSessions;
  const splits = deps.splits ?? defaultSplits;

  /** Expansion changed here but not yet confirmed by Rust (keeps the tree from flickering). */
  let pendingExpansion = $state<string[] | null>(null);
  /** Connections whose SSH key setup is running. */
  let keySetups = $state<string[]>([]);
  /** The entry the merge sheet merges into, while it is open. */
  let mergeTarget = $state<string | null>(null);

  async function perform<T>(work: () => Promise<T>): Promise<T | undefined> {
    try {
      return await work();
    } catch (error) {
      alerts.show(errorMessage(error));
      return undefined;
    }
  }

  /** The toast after a launch (Swift `AppState.connect`). */
  function launched(connection: ConnectionDto, outcome: LaunchOutcomeDto | undefined) {
    if (!outcome) return;
    switch (outcome.kind) {
      case 'launched':
        toast.show(t('Connecting: %@', connection.name));
        return;
      case 'loginCopied':
        toast.show(t('Opened: %@ · password, then username on the clipboard', connection.name));
        return;
      case 'passwordCopied':
        toast.show(t('Password on the clipboard (%@)', t(outcome.reason)));
        return;
    }
  }

  const actions = {
    /** Expanded folder and entry ids (the `expandedFolders` setting). */
    get expanded(): ReadonlySet<string> {
      // A snapshot: it is rebuilt from reactive state on every read.
      // eslint-disable-next-line svelte/prefer-svelte-reactivity
      return new Set(pendingExpansion ?? settings.settings?.expandedFolders ?? []);
    },

    /** Opens, closes (or with `open` sets) a folder or entry in the tree and saves the expansion. */
    async setExpanded(id: string, open?: boolean) {
      const next = toggledExpansion(actions.expanded, id, open);
      pendingExpansion = next;
      try {
        await settings.update({ expandedFolders: next });
      } catch (error) {
        alerts.show(errorMessage(error));
      } finally {
        if (pendingExpansion === next) pendingExpansion = null;
      }
    },

    /** The folder new items go into: the selected folder, or the selected entry's (or connection's). */
    selectedFolderId(): string | null {
      const parsed = library.single ? parseTag(library.single) : null;
      if (parsed?.kind === 'folder') return parsed.id;
      return library.connectionForTag(library.single)?.folderId ?? null;
    },

    /**
     * Connects: an embedded SSH session, the external terminal, the browser, WinBox, an embedded RDP tab
     * or the system RDP client. `options` are a menu item's MikroTik mode, SSH terminal and RDP client.
     */
    async connect(connection?: ConnectionDto, options: ConnectOptions = {}) {
      if (!connection) return;
      switch (connectRoute(connection, options)) {
        case 'embedded':
          await perform(() => sessions.open(connection));
          return;
        case 'external':
          if ((await perform(() => api.openExternalTerminal(connection.id))) !== undefined) {
            toast.show(t('Connecting: %@', connection.name));
          }
          return;
        case 'web':
          launched(connection, await perform(() => api.connectWeb(connection.id)));
          return;
        case 'winbox':
          launched(connection, await perform(() => api.connectWinbox(connection.id)));
          return;
        case 'rdpExternal':
          launched(connection, await perform(() => api.connectRdpExternal(connection.id)));
          return;
        case 'rdpEmbedded':
          await perform(() => rdpSessions.open(connection));
          return;
      }
    },

    /** The "Local Terminal" entry: this machine's shell in a new tab. */
    async openLocalTerminal() {
      await perform(() => sessions.openLocal());
    },

    // Editors
    newConnection(type: ConnectionType, folderId: string | null = actions.selectedFolderId()) {
      editor.openConnectionEditor({ type, folderId });
    },
    newFolder(parentId: string | null = actions.selectedFolderId()) {
      editor.openFolderEditor({ parentId });
    },
    newCredential() {
      editor.openCredentialEditor();
    },
    editConnection(id: string) {
      editor.openConnectionEditor({ id });
    },
    editCredential(id: string) {
      editor.openCredentialEditor(id);
    },
    renameFolder(id: string) {
      editor.openFolderEditor({ id });
    },

    /** The double-click / Return action of a single row. */
    primaryAction(tag: string) {
      const parsed = parseTag(tag);
      if (!parsed) return;
      switch (parsed.kind) {
        case 'credential':
          actions.editCredential(parsed.id);
          break;
        case 'folder':
          void actions.setExpanded(parsed.id);
          break;
        case 'entry':
        case 'favorite':
        case 'connection':
          // An entry connects its default connection, a child row its own.
          void actions.connect(library.connectionForTag(tag));
          break;
        case 'split':
          actions.openSavedSplit(parsed.id);
          break;
      }
    },

    // Connections
    /** Duplicates an entry with all its connections. */
    duplicate: (entryId: string) => perform(() => api.duplicateEntry(entryId, t(' (copy)'))),
    /** The entry the open merge sheet merges into; `null` when it is closed. */
    get mergeTarget(): string | null {
      return mergeTarget;
    },
    /** "Merge…": opens the merge sheet for the entry. */
    merge(targetId: string) {
      mergeTarget = targetId;
    },
    closeMerge() {
      mergeTarget = null;
    },
    /** Moves the connections of `sources` into `target` (the emptied entries go away). */
    async mergeEntries(target: string, sources: string[]) {
      const result = await perform(() => api.mergeEntries(target, sources));
      if (result) toast.show(t('Merged — %lld connections moved', result.movedConnections));
      return result;
    },
    /** Stars or unstars the entry of a connection. */
    setFavorite: (connectionId: string, isFavorite: boolean) =>
      perform(() => api.setFavorite(`c:${connectionId}`, isFavorite)),
    /** Deletes one connection (its entry too when it was the last). */
    deleteConnection: (id: string) => perform(() => api.deleteItems([`c:${id}`])),
    /** Deletes an entry with all its connections. */
    deleteEntry: (id: string) => perform(() => api.deleteItems([`e:${id}`])),
    async copyPassword(id: string) {
      try {
        await api.copyPassword(id);
        toast.show(t('Password on the clipboard (30 s)'));
      } catch (error) {
        if (isCommandError(error) && error.code === 'noPassword') toast.show(t('No saved password'));
        else alerts.show(errorMessage(error));
      }
    },
    copyHost: (connection: ConnectionDto) => perform(() => api.copyText(connection.host)),
    /** Whether "Set up SSH key login" runs for the connection. */
    settingUpKey: (connectionId: string) => keySetups.includes(connectionId),
    /**
     * "Set up SSH key login": installs the connection's key on its server (a new key pair is generated
     * and saved first when it has none). Progress and success are toasts, problems stay up as alerts;
     * a second click while it runs is ignored.
     */
    async setupSshKey(connection: ConnectionDto) {
      if (keySetups.includes(connection.id)) return;
      keySetups = [...keySetups, connection.id];
      toast.show(t('Setting up SSH key login on %@…', connection.host));
      try {
        const result = await api.sshKeySetup(connection.id);
        toast.dismiss();
        if (result.verified) toast.show(t('SSH key installed on %@', result.host));
        else alerts.show(t('SSH key copied to %@, but logging in with it failed', result.host));
      } catch (error) {
        toast.dismiss();
        alerts.show(errorMessage(error));
      } finally {
        keySetups = keySetups.filter((id) => id !== connection.id);
      }
    },
    /** Opens (or focuses) the connection's Files window; `sessionId` starts in that shell's folder. */
    openFiles: (connectionId: string, options: { sessionId?: string } = {}) =>
      perform(() => api.filesOpen(connectionId, options)),

    /** Moves rows into a folder (`null`: the root); refused folder cycles toast, the rest still moves. */
    async move(tags: string[], folderId: string | null) {
      const result = await perform(() => api.moveItems(tags, folderId));
      if (result && result.cycleRefused > 0) toast.show(t("A folder can't be moved into itself"));
      return result;
    },

    // Folders
    setFolderColor(id: string, color: LabelColor | null) {
      const folder = library.folder(id);
      if (!folder) return Promise.resolve(undefined);
      return perform(() => api.saveFolder({ id, name: folder.name, color }));
    },
    deleteFolder: (id: string) => perform(() => api.deleteItems([`f:${id}`])),

    // Credentials
    /** Deletes at once when unused, otherwise after "Delete credential?". */
    async deleteCredential(id: string) {
      const credential = library.credential(id);
      if (!credential) return;
      if (credential.usageCount > 0) {
        const ok = await dialogs.confirm({
          title: t('Delete credential?'),
          message: credentialDeleteMessage(credential.usageCount, credential.name),
          confirmLabel: t('Delete'),
          destructive: true,
        });
        if (!ok) return;
      }
      await perform(() => api.deleteItems([`k:${id}`]));
    },

    /** Links `credentialId` (or with `null` the own login) to the connections behind `tags`. */
    async setCredential(credentialId: string | null, tags: string[]) {
      const changed = await perform(() => api.setCredential(credentialId, { tags }));
      if (changed === undefined) return;
      toast.show(
        credentialId === null
          ? t('Linked credential removed — %lld connections', changed)
          : t('Linked credential set — %lld connections', changed),
      );
    },

    /** "Delete N items…": confirms, clears the selection, then deletes. */
    async deleteItems(tags: string[]) {
      const counts = deletionCounts(tags);
      if (counts.tags.length === 0) return;
      const ok = await dialogs.confirm({
        title: bulkDeleteTitle(counts),
        message: bulkDeleteMessage(counts),
        confirmLabel: t('Delete'),
        destructive: true,
      });
      if (!ok) return;
      library.clearSelection();
      await perform(() => api.deleteItems(counts.tags));
    },

    // Saved splits
    /** Opens a saved split's existing connections in a split tab (broadcast off). */
    openSavedSplit(id: string) {
      void perform(() => splits.openSaved(id));
    },
    /** "Open in split view": 2–4 SSH connections side by side (more are left out with a toast). */
    openSplit(connections: ConnectionDto[]) {
      if (connections.length === 0) return;
      void perform(() => splits.open(connections));
    },
    /** "Save as split…": asks the name (the panes' names by default) and saves up to 4 panes. */
    async saveAsSplit(connections: ConnectionDto[]) {
      const picked = connections.slice(0, MAX_PANES);
      const name = await dialogs.prompt({
        title: t('Save split'),
        label: t('Name'),
        initial: picked.map((c) => c.name).join(' + '),
        confirmLabel: t('Save'),
      });
      if (name === null) return;
      const id = await perform(() =>
        api.saveSavedSplit(
          null,
          name,
          picked.map((c) => c.id),
        ),
      );
      if (id !== undefined) toast.show(t('Saved split: %@', name));
    },
    async renameSavedSplit(id: string) {
      const split = library.split(id);
      if (!split) return;
      const name = await dialogs.prompt({
        title: t('Rename saved split'),
        label: t('Name'),
        initial: split.name,
        confirmLabel: t('Rename'),
      });
      if (name !== null) await perform(() => api.renameSavedSplit(id, name));
    },
    deleteSavedSplit: (id: string) => perform(() => api.deleteSavedSplit(id)),
    reorderSavedSplits: (ids: string[]) => perform(() => api.reorderSavedSplits(ids)),
  };

  return actions;
}

export type SidebarActions = ReturnType<typeof createSidebarActions>;

export const sidebarActions = createSidebarActions();
