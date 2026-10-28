// Split tabs (Swift `SplitSession` and the split parts of `AppState`): 2–4 embedded SSH panes in one tab.
// Each pane is an ordinary session of the sessions store opened without a tab; this store is its
// `PaneHost`. Typing into a broadcasting pane goes to the other broadcasting, running panes through
// `ssh_broadcast_input`. A pane closes on exit 0 (the tab with the last pane); other exits keep the pane
// with its exit bar. "Split terminal" turns a single terminal tab into a split, keeping its live session.
// Local terminal panes can be added and split like the others, but are left out of saved splits.
// "Split tabs…" moves 2–4 open terminal tabs, with their live sessions, into one new split tab; "Move to
// its own tab" takes a pane back out (the last pane left becomes a tab too).

import { api as defaultApi, type Api } from '../api';
import { errorMessage } from '../errors';
import { t } from '../i18n/index.svelte';
import { alerts as defaultAlerts, type AlertStore } from '../stores/alert.svelte';
import { dialogs as defaultDialogs, type DialogStore } from '../stores/dialogs.svelte';
import { library as defaultLibrary, type LibraryStore } from '../stores/library.svelte';
import { toast as defaultToast, type ToastStore } from '../stores/toast.svelte';
import { tabs as defaultTabs, type TabSession, type TabsStore } from '../tabs.svelte';
import type { ConnectionDto } from '../types';
import { LOCAL_TERMINAL, isLocalTerminal } from './local';
import { sessions as defaultSessions, type SessionsStore } from './sessions.svelte';
import {
  MAX_PANES,
  broadcastTargets,
  commonColor,
  isAllBroadcasting,
  isFull,
  newSplit,
  savedSplitConnections,
  splitTitle,
  withAllBroadcastToggled,
  withBroadcast,
  withoutPane,
  withPane,
  type SplitPane,
  type SplitState,
} from './split';

type PaneConnection = Pick<ConnectionDto, 'id' | 'name' | 'type' | 'color'>;

export interface SplitsDeps {
  api: Pick<Api, 'saveSavedSplit'>;
  sessions: SessionsStore;
  tabs: TabsStore;
  library: Pick<LibraryStore, 'connection' | 'split'>;
  dialogs: Pick<DialogStore, 'prompt'>;
  toast: Pick<ToastStore, 'show'>;
  alerts: Pick<AlertStore, 'show'>;
  newId: () => string;
}

export function createSplitsStore(deps: Partial<SplitsDeps> = {}) {
  const api = deps.api ?? defaultApi;
  const sessions = deps.sessions ?? defaultSessions;
  const tabs = deps.tabs ?? defaultTabs;
  const library = deps.library ?? defaultLibrary;
  const dialogs = deps.dialogs ?? defaultDialogs;
  const toast = deps.toast ?? defaultToast;
  const alerts = deps.alerts ?? defaultAlerts;
  const newId = deps.newId ?? (() => `split-${crypto.randomUUID()}`);

  let splits = $state<Record<string, SplitState>>({});

  const byPane = (paneId: string): SplitState | undefined =>
    Object.values(splits).find((s) => s.panes.some((p) => p.id === paneId));
  const isRunning = (id: string) => sessions.state(id)?.status.kind === 'running';
  /** What a pane runs, to open it again: its library connection, or a local terminal. */
  const paneConnection = (connectionId: string): PaneConnection | undefined =>
    isLocalTerminal(connectionId) ? LOCAL_TERMINAL : library.connection(connectionId);
  /** The connections a saved split keeps: local terminals are left out. */
  const savedIds = (split: SplitState) =>
    split.panes.map((p) => p.connectionId).filter((id) => !isLocalTerminal(id));

  /** The automatic title, colour and flags of a split's tab. */
  function tabInfo(split: SplitState): Omit<TabSession, 'id'> {
    const states = split.panes.map((p) => sessions.state(p.id));
    return {
      title: splitTitle(states.map((s) => s?.title ?? '')),
      type: 'ssh',
      color: commonColor(split.panes.map((p) => library.connection(p.connectionId)?.color ?? null)),
      root: states.some((s) => s?.root === true),
      split: true,
      broadcasting: split.broadcasting.length > 0,
    };
  }

  /** The tab a pane's session gets of its own, as `sessions.open` would give it; `undefined` when gone. */
  function paneTab(pane: SplitPane): TabSession | undefined {
    const state = sessions.state(pane.id);
    if (!state) return undefined;
    const local = isLocalTerminal(pane.connectionId);
    const connection = local ? undefined : library.connection(pane.connectionId);
    return {
      id: pane.id,
      title: state.title,
      root: state.root,
      type: connection?.type ?? 'ssh',
      color: connection?.color ?? null,
      ...(local ? {} : { group: connection?.entryId ?? pane.connectionId }),
    };
  }

  function put(split: SplitState) {
    splits = { ...splits, [split.id]: split };
    tabs.updateSession(split.id, tabInfo(split));
  }

  function drop(id: string) {
    if (!(id in splits)) return;
    const rest = { ...splits };
    delete rest[id];
    splits = rest;
  }

  /** Opens a pane session; `null` (with the alert) when it fails. */
  async function openPane(connection: PaneConnection): Promise<SplitPane | null> {
    try {
      const id = await sessions.open(connection, { pane: true });
      // It may have ended (exit 0) before its split knew it.
      return sessions.has(id) ? { id, connectionId: connection.id } : null;
    } catch (error) {
      alerts.show(errorMessage(error));
      return null;
    }
  }

  function closePane(paneId: string) {
    const split = byPane(paneId);
    sessions.end(paneId);
    if (!split) return;
    const next = withoutPane(split, paneId);
    if (next.panes.length === 0) {
      drop(split.id);
      tabs.closeSession(split.id);
    } else put(next);
  }

  sessions.setPaneHost({
    targets: (id) => {
      const split = byPane(id);
      return split ? broadcastTargets(split, id, isRunning) : [];
    },
    closePane,
    changed: (id) => {
      const split = byPane(id);
      if (split) tabs.updateSession(split.id, tabInfo(split));
    },
    focusedPane: (tabId) => splits[tabId]?.focused ?? null,
  });

  const offClosed = tabs.onSessionClosed((id) => {
    const split = splits[id];
    if (!split) return;
    drop(id);
    for (const pane of split.panes) sessions.end(pane.id);
  });

  const store = {
    get all(): Readonly<Record<string, SplitState>> {
      return splits;
    },
    get(id: string | null): SplitState | undefined {
      return id === null ? undefined : splits[id];
    },
    has(id: string): boolean {
      return id in splits;
    },
    /** The active tab's split. */
    get active(): SplitState | undefined {
      return store.get(tabs.active);
    },
    isFull: (id: string) => (splits[id] ? isFull(splits[id]) : true),
    isAllBroadcasting: (id: string) => (splits[id] ? isAllBroadcasting(splits[id]) : false),
    isBroadcasting: (paneId: string) => byPane(paneId)?.broadcasting.includes(paneId) ?? false,

    /**
     * Opens the connections (at most 4; a toast when more were given) as panes of a new split tab; the
     * tab id, or `null` when no pane could start.
     */
    async open(
      connections: readonly PaneConnection[],
      savedSplitId: string | null = null,
    ): Promise<string | null> {
      if (connections.length === 0) return null;
      const opened: SplitPane[] = [];
      for (const connection of connections.slice(0, MAX_PANES)) {
        const pane = await openPane(connection);
        if (pane) opened.push(pane);
      }
      const live = opened.filter((p) => sessions.has(p.id));
      if (live.length === 0) return null;
      if (connections.length > MAX_PANES)
        toast.show(t('A split view holds at most %lld terminals.', MAX_PANES));
      const split = newSplit(newId(), live, savedSplitId);
      splits = { ...splits, [split.id]: split };
      tabs.openSession({ id: split.id, ...tabInfo(split) });
      return split.id;
    },

    /** Opens a saved split's existing connections; broadcast always starts off. */
    async openSaved(savedId: string): Promise<string | null> {
      const saved = library.split(savedId);
      if (!saved) return null;
      const connections = savedSplitConnections(saved.connectionIds, (id) => library.connection(id));
      if (connections.length === 0) {
        toast.show(t("None of this split's connections exist any more."));
        return null;
      }
      return store.open(connections, saved.id);
    },

    /** "Add terminal": one more pane (the same connection may run in several). */
    async addPane(splitId: string, connection: PaneConnection) {
      const split = splits[splitId];
      if (!split || isFull(split)) return;
      const pane = await openPane(connection);
      if (!pane) return;
      const current = splits[splitId];
      const next = current ? withPane(current, pane) : null;
      if (!next) {
        sessions.end(pane.id);
        return;
      }
      put(next);
    },

    /** Closes one pane (and the tab with the last one). */
    closePane,

    /**
     * "Move to its own tab": the pane's session leaves the split, live, for a tab of its own right after the
     * split. When one pane would be left, it takes the split tab's place as a plain tab and the split ends.
     */
    detachPane(paneId: string) {
      const split = byPane(paneId);
      const pane = split?.panes.find((p) => p.id === paneId);
      const tab = pane && paneTab(pane);
      if (!split || !tab) return;
      const rest = withoutPane(split, paneId);
      sessions.release(paneId);
      if (rest.panes.length > 1) {
        put(rest);
        tabs.openSession(tab, split.id);
        return;
      }
      const last = rest.panes[0];
      const lastTab = last && paneTab(last);
      if (last) sessions.release(last.id);
      drop(split.id);
      // The split's name and colour were the split's: the terminal left in its place starts without them.
      const inPlace = lastTab ?? tab;
      tabs.replaceSession(split.id, inPlace);
      tabs.rename(inPlace.id, '');
      tabs.resetColor(inPlace.id);
      if (lastTab) tabs.openSession(tab, lastTab.id);
      else tabs.select(tab.id);
    },

    /** The pane the keyboard goes to (clicked or focused). */
    focus(paneId: string) {
      const split = byPane(paneId);
      if (split && split.focused !== paneId)
        splits = { ...splits, [split.id]: { ...split, focused: paneId } };
    },

    toggleBroadcast(paneId: string) {
      const split = byPane(paneId);
      if (split) put(withBroadcast(split, paneId, !split.broadcasting.includes(paneId)));
    },

    toggleBroadcastAll(splitId: string) {
      const split = splits[splitId];
      if (split) put(withAllBroadcastToggled(split));
    },

    /** The broadcastAll command: on the active split tab only. */
    toggleBroadcastActive() {
      const split = store.active;
      if (split) store.toggleBroadcastAll(split.id);
    },

    /** Whether "Split terminal" does something on the active tab. */
    get canSplitActive(): boolean {
      const active = tabs.active;
      if (active === null) return false;
      const split = splits[active];
      if (split) return !isFull(split);
      return sessions.has(active) && !sessions.isPane(active);
    },

    /**
     * "Split terminal": a terminal tab becomes a split whose first pane is its running session (same
     * terminal, no reconnect) plus a second pane on the same server; in a split, the focused pane's server
     * is opened once more.
     */
    async splitActive() {
      const active = tabs.active;
      if (active === null) return;
      const split = splits[active];
      if (split) {
        if (isFull(split)) return;
        const pane = split.panes.find((p) => p.id === split.focused) ?? split.panes.at(-1);
        const connection = pane && paneConnection(pane.connectionId);
        if (connection) await store.addPane(split.id, connection);
        return;
      }
      const state = sessions.state(active);
      const tab = tabs.session(active);
      if (!state || !tab || sessions.isPane(active)) return;
      const connection: PaneConnection = paneConnection(state.connectionId) ?? {
        id: state.connectionId,
        name: state.connectionName,
        type: tab.type,
        color: tab.color,
      };
      const next = newSplit(newId(), [{ id: active, connectionId: state.connectionId }]);
      sessions.adopt(active);
      splits = { ...splits, [next.id]: next };
      tabs.replaceSession(active, { id: next.id, ...tabInfo(next) });
      await store.addPane(next.id, connection);
    },

    /** Tabs "Split tabs…" can put side by side: single terminal tabs (not splits), in tab bar order. */
    get mergeCandidates(): string[] {
      return tabs.ids.filter((id) => sessions.has(id) && !sessions.isPane(id) && !(id in splits));
    },

    /**
     * "Split tabs…": the given candidate tabs (in tab bar order, at most 4) become the panes of one new split
     * tab, keeping their sessions and terminals. The split takes the first tab's place; the active tab's
     * pane gets the focus. The tab id, or `null` (nothing changes) when fewer than 2 are candidates.
     */
    mergeTabs(ids: readonly string[]): string | null {
      const chosen = store.mergeCandidates.filter((id) => ids.includes(id)).slice(0, MAX_PANES);
      const [first, ...rest] = chosen;
      if (first === undefined || rest.length === 0) return null;
      const panes = chosen.map((id) => ({ id, connectionId: sessions.state(id)?.connectionId ?? '' }));
      const active = tabs.active;
      const split = newSplit(newId(), panes);
      if (active !== null && chosen.includes(active)) split.focused = active;
      for (const id of chosen) sessions.adopt(id);
      splits = { ...splits, [split.id]: split };
      tabs.replaceSession(first, { id: split.id, ...tabInfo(split) });
      for (const id of rest) tabs.detachSession(id);
      tabs.select(split.id);
      return split.id;
    },

    /** "Save split…": asks the name (the tab's title by default) and saves the panes' connections. */
    async save(splitId: string) {
      const split = splits[splitId];
      const tab = tabs.session(splitId);
      if (!split || !tab) return;
      const ids = savedIds(split);
      if (ids.length === 0) {
        toast.show(t("Local terminals can't be saved in a split."));
        return;
      }
      const name = await dialogs.prompt({
        title: t('Save split'),
        label: t('Name'),
        initial: tabs.displayTitle(tab),
        confirmLabel: t('Save'),
      });
      if (name === null) return;
      try {
        const id = await api.saveSavedSplit(null, name, ids);
        const current = splits[splitId];
        if (current) splits = { ...splits, [splitId]: { ...current, savedSplitId: id } };
        toast.show(t('Saved split: %@', name.trim()));
      } catch (error) {
        alerts.show(errorMessage(error));
      }
    },

    /** The tab came from a saved split whose panes differ from the tab's now (local terminals aside). */
    canUpdateSaved(splitId: string): boolean {
      const split = splits[splitId];
      const saved = split?.savedSplitId ? library.split(split.savedSplitId) : undefined;
      if (!split || !saved) return false;
      const ids = savedIds(split);
      if (ids.length === 0) return false;
      return ids.length !== saved.connectionIds.length || ids.some((id, i) => saved.connectionIds[i] !== id);
    },

    /** "Update saved split": the saved split gets the tab's current panes. */
    async updateSaved(splitId: string) {
      const split = splits[splitId];
      const saved = split?.savedSplitId ? library.split(split.savedSplitId) : undefined;
      if (!split || !saved) return;
      try {
        await api.saveSavedSplit(saved.id, saved.name, savedIds(split));
        toast.show(t('Saved split updated: %@', saved.name));
      } catch (error) {
        alerts.show(errorMessage(error));
      }
    },

    /** Keeps the automatic tab colours in line with the connections. */
    syncConnections() {
      for (const split of Object.values(splits)) tabs.updateSession(split.id, tabInfo(split));
    },

    /** Stops listening (tests). */
    dispose() {
      offClosed();
      sessions.setPaneHost(null);
    },
  };
  return store;
}

export type SplitsStore = ReturnType<typeof createSplitsStore>;

/** The app-wide instance. */
export const splits = createSplitsStore();
